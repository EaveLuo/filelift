//! Local secret store.
//!
//! filelift is AI-first and must run non-interactively from CI, containers, and
//! isolated agent sandboxes, so it deliberately does **not** use the OS keyring.
//! Persistent secrets live in a single encrypted file,
//! `~/.filelift/secrets.enc`, written with `0600` permissions and encrypted with
//! ChaCha20-Poly1305 using a machine-derived key (see [`crate::machine_key`]).
//!
//! Credentials are resolved through a layered chain so injection always wins
//! over stored values:
//!
//! 1. Environment variables (`FILELIFT_<TARGET>_*` then global `FILELIFT_*`).
//! 2. The encrypted file store.
//!
//! Explicit CLI/stdin input is handled one level up, in the command layer, and
//! takes precedence over everything here.

use std::{collections::BTreeMap, env, path::PathBuf};

use anyhow::{Context, Result};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use serde::{Deserialize, Serialize};

use crate::machine_key::{self, KEY_LEN};
use crate::secret_file;
use crate::target::filelift_home_dir;

const SECRETS_FILE: &str = "secrets.enc";
const NONCE_LEN: usize = 12;
const ENV_PREFIX: &str = "FILELIFT";
const ACCESS_KEY_ID_SUFFIX: &str = "ACCESS_KEY_ID";
const SECRET_ACCESS_KEY_SUFFIX: &str = "SECRET_ACCESS_KEY";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredCredentials {
    access_key_id: String,
    secret_access_key: String,
}

impl From<StoredCredentials> for Credentials {
    fn from(value: StoredCredentials) -> Self {
        Self {
            access_key_id: value.access_key_id,
            secret_access_key: value.secret_access_key,
        }
    }
}

/// The full decrypted contents of the secret store.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SecretData {
    #[serde(default)]
    credentials: BTreeMap<String, StoredCredentials>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diagnostic_log_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interactive_history_key: Option<String>,
}

/// An encrypted-at-rest secret store bound to a file path and encryption key.
///
/// The struct is intentionally decoupled from environment and home-directory
/// resolution so it can be unit tested directly with a temp path and a fixed
/// key, without touching global process state.
struct SecretStore {
    path: PathBuf,
    key: [u8; KEY_LEN],
}

impl SecretStore {
    /// Opens the store at the user's `~/.filelift/secrets.enc` using the
    /// machine-derived key.
    fn open() -> Result<Self> {
        Ok(Self {
            path: secrets_path()?,
            key: machine_key::store_key()?,
        })
    }

    #[cfg(test)]
    fn with(path: PathBuf, key: [u8; KEY_LEN]) -> Self {
        Self { path, key }
    }

    fn load(&self) -> Result<SecretData> {
        let Some(bytes) = secret_file::read_bytes(&self.path)? else {
            return Ok(SecretData::default());
        };

        anyhow::ensure!(
            bytes.len() > NONCE_LEN,
            "secret store at {} is corrupt (too short)",
            self.path.display()
        );
        let (nonce, ciphertext) = bytes.split_at(NONCE_LEN);

        let cipher =
            ChaCha20Poly1305::new_from_slice(&self.key).context("invalid secret store key")?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| {
                anyhow::anyhow!(
                    "failed to decrypt secret store at {} (wrong machine key or corrupt file)",
                    self.path.display()
                )
            })?;

        toml::from_str(&String::from_utf8(plaintext).context("secret store is not valid UTF-8")?)
            .context("failed to parse decrypted secret store")
    }

    fn save(&self, data: &SecretData) -> Result<()> {
        let plaintext = toml::to_string(data).context("failed to serialize secret store")?;

        let cipher =
            ChaCha20Poly1305::new_from_slice(&self.key).context("invalid secret store key")?;
        let mut nonce = [0_u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
            .map_err(|_| anyhow::anyhow!("failed to encrypt secret store"))?;

        let mut bytes = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        bytes.extend_from_slice(&nonce);
        bytes.extend_from_slice(&ciphertext);

        secret_file::write_secure(&self.path, &bytes)
    }

    fn mutate(&self, change: impl FnOnce(&mut SecretData)) -> Result<()> {
        let mut data = self.load()?;
        change(&mut data);
        self.save(&data)
    }
}

fn secrets_path() -> Result<PathBuf> {
    Ok(filelift_home_dir()?.join(SECRETS_FILE))
}

pub fn set_credentials(
    target_name: &str,
    access_key_id: &str,
    secret_access_key: &str,
) -> Result<()> {
    SecretStore::open()?.mutate(|data| {
        data.credentials.insert(
            target_name.to_string(),
            StoredCredentials {
                access_key_id: access_key_id.to_string(),
                secret_access_key: secret_access_key.to_string(),
            },
        );
    })
}

/// Resolves credentials for a target through the env → file chain.
pub fn credentials(target_name: &str) -> Result<Credentials> {
    let data = SecretStore::open()?.load()?;
    resolve_credentials(target_name, env_lookup, data.credentials.get(target_name))
        .with_context(|| format!("no credentials found for target `{target_name}`"))
}

/// Returns credentials stored on disk for a target, ignoring environment
/// overrides. Used by `credentials export` to materialize what is persisted.
pub fn stored_credentials(target_name: &str) -> Result<Option<Credentials>> {
    let data = SecretStore::open()?.load()?;
    Ok(data
        .credentials
        .get(target_name)
        .cloned()
        .map(Credentials::from))
}

pub fn delete_credentials(target_name: &str) -> Result<()> {
    SecretStore::open()?.mutate(|data| {
        data.credentials.remove(target_name);
    })
}

pub fn diagnostic_log_key() -> Result<String> {
    SecretStore::open()?
        .load()?
        .diagnostic_log_key
        .context("diagnostic log key is not set")
}

pub fn set_diagnostic_log_key(value: &str) -> Result<()> {
    SecretStore::open()?.mutate(|data| {
        data.diagnostic_log_key = Some(value.to_string());
    })
}

pub fn interactive_history_key() -> Result<String> {
    SecretStore::open()?
        .load()?
        .interactive_history_key
        .context("interactive history key is not set")
}

pub fn set_interactive_history_key(value: &str) -> Result<()> {
    SecretStore::open()?.mutate(|data| {
        data.interactive_history_key = Some(value.to_string());
    })
}

/// Returns the per-target environment variable names for a target's credential
/// pair, as `(access_key_id_var, secret_access_key_var)`.
pub fn target_env_var_names(target_name: &str) -> (String, String) {
    (
        target_env_var(target_name, ACCESS_KEY_ID_SUFFIX),
        target_env_var(target_name, SECRET_ACCESS_KEY_SUFFIX),
    )
}

fn env_lookup(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Resolves a credential pair through the environment then the stored value.
/// The two halves are resolved independently, so an id from the environment can
/// combine with a secret from the file (or vice versa).
fn resolve_credentials(
    target_name: &str,
    get_env: impl Fn(&str) -> Option<String>,
    stored: Option<&StoredCredentials>,
) -> Option<Credentials> {
    let access_key_id = resolve_field(
        target_name,
        ACCESS_KEY_ID_SUFFIX,
        &get_env,
        stored.map(|stored| stored.access_key_id.as_str()),
    )?;
    let secret_access_key = resolve_field(
        target_name,
        SECRET_ACCESS_KEY_SUFFIX,
        &get_env,
        stored.map(|stored| stored.secret_access_key.as_str()),
    )?;

    Some(Credentials {
        access_key_id,
        secret_access_key,
    })
}

fn resolve_field(
    target_name: &str,
    suffix: &str,
    get_env: impl Fn(&str) -> Option<String>,
    stored: Option<&str>,
) -> Option<String> {
    get_env(&target_env_var(target_name, suffix))
        .or_else(|| get_env(&format!("{ENV_PREFIX}_{suffix}")))
        .or_else(|| stored.map(str::to_string))
}

/// Builds a per-target environment variable name, upper-casing the target and
/// replacing every non-alphanumeric character with `_`
/// (e.g. `r2-blog` → `FILELIFT_R2_BLOG_ACCESS_KEY_ID`).
fn target_env_var(target_name: &str, suffix: &str) -> String {
    let normalized: String = target_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("{ENV_PREFIX}_{normalized}_{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn fixed_key() -> [u8; KEY_LEN] {
        [42_u8; KEY_LEN]
    }

    fn make_env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        move |name: &str| map.get(name).cloned()
    }

    #[test]
    fn target_env_var_normalizes_name() {
        assert_eq!(
            target_env_var("r2-blog", ACCESS_KEY_ID_SUFFIX),
            "FILELIFT_R2_BLOG_ACCESS_KEY_ID"
        );
        assert_eq!(
            target_env_var("My.Bucket 1", SECRET_ACCESS_KEY_SUFFIX),
            "FILELIFT_MY_BUCKET_1_SECRET_ACCESS_KEY"
        );
    }

    #[test]
    fn store_roundtrips_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let store = SecretStore::with(dir.path().join("secrets.enc"), fixed_key());

        store
            .mutate(|data| {
                data.credentials.insert(
                    "r2".to_string(),
                    StoredCredentials {
                        access_key_id: "id".to_string(),
                        secret_access_key: "secret".to_string(),
                    },
                );
            })
            .unwrap();

        let loaded = store.load().unwrap();
        let stored = loaded.credentials.get("r2").unwrap();
        assert_eq!(stored.access_key_id, "id");
        assert_eq!(stored.secret_access_key, "secret");
    }

    #[test]
    fn store_file_is_encrypted_at_rest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.enc");
        let store = SecretStore::with(path.clone(), fixed_key());

        store
            .mutate(|data| {
                data.credentials.insert(
                    "r2".to_string(),
                    StoredCredentials {
                        access_key_id: "plain-access-id".to_string(),
                        secret_access_key: "plain-secret-key".to_string(),
                    },
                );
            })
            .unwrap();

        let raw = std::fs::read(&path).unwrap();
        let haystack = String::from_utf8_lossy(&raw);
        assert!(!haystack.contains("plain-access-id"));
        assert!(!haystack.contains("plain-secret-key"));
        assert!(!haystack.contains("r2"));
    }

    #[test]
    fn store_cannot_be_decrypted_with_wrong_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.enc");

        SecretStore::with(path.clone(), [1_u8; KEY_LEN])
            .mutate(|data| {
                data.diagnostic_log_key = Some("deadbeef".to_string());
            })
            .unwrap();

        let result = SecretStore::with(path, [2_u8; KEY_LEN]).load();
        assert!(result.is_err());
    }

    #[test]
    fn loading_missing_store_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let store = SecretStore::with(dir.path().join("absent.enc"), fixed_key());
        let data = store.load().unwrap();
        assert!(data.credentials.is_empty());
        assert!(data.diagnostic_log_key.is_none());
    }

    #[test]
    fn delete_then_load_drops_entry() {
        let dir = tempfile::tempdir().unwrap();
        let store = SecretStore::with(dir.path().join("secrets.enc"), fixed_key());
        store
            .mutate(|data| {
                data.credentials
                    .insert("r2".to_string(), StoredCredentials::default());
            })
            .unwrap();
        store
            .mutate(|data| {
                data.credentials.remove("r2");
            })
            .unwrap();
        assert!(store.load().unwrap().credentials.is_empty());
    }

    #[test]
    fn resolve_prefers_per_target_env_over_global_and_file() {
        let stored = StoredCredentials {
            access_key_id: "file-id".to_string(),
            secret_access_key: "file-secret".to_string(),
        };
        let env = make_env(&[
            ("FILELIFT_R2_BLOG_ACCESS_KEY_ID", "target-id"),
            ("FILELIFT_R2_BLOG_SECRET_ACCESS_KEY", "target-secret"),
            ("FILELIFT_ACCESS_KEY_ID", "global-id"),
            ("FILELIFT_SECRET_ACCESS_KEY", "global-secret"),
        ]);

        let resolved = resolve_credentials("r2-blog", env, Some(&stored)).unwrap();
        assert_eq!(resolved.access_key_id, "target-id");
        assert_eq!(resolved.secret_access_key, "target-secret");
    }

    #[test]
    fn resolve_falls_back_to_global_env_then_file() {
        let stored = StoredCredentials {
            access_key_id: "file-id".to_string(),
            secret_access_key: "file-secret".to_string(),
        };
        let env = make_env(&[("FILELIFT_ACCESS_KEY_ID", "global-id")]);

        // id comes from the global env, secret falls through to the file.
        let resolved = resolve_credentials("r2-blog", env, Some(&stored)).unwrap();
        assert_eq!(resolved.access_key_id, "global-id");
        assert_eq!(resolved.secret_access_key, "file-secret");
    }

    #[test]
    fn resolve_uses_file_when_no_env_is_present() {
        let stored = StoredCredentials {
            access_key_id: "file-id".to_string(),
            secret_access_key: "file-secret".to_string(),
        };
        let resolved = resolve_credentials("r2-blog", make_env(&[]), Some(&stored)).unwrap();
        assert_eq!(resolved.access_key_id, "file-id");
        assert_eq!(resolved.secret_access_key, "file-secret");
    }

    #[test]
    fn resolve_returns_none_when_secret_is_missing_everywhere() {
        let env = make_env(&[("FILELIFT_ACCESS_KEY_ID", "global-id")]);
        assert!(resolve_credentials("r2-blog", env, None).is_none());
    }

    #[test]
    fn resolve_returns_none_with_no_sources() {
        assert!(resolve_credentials("r2-blog", make_env(&[]), None).is_none());
    }
}
