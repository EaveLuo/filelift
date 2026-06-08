//! Derivation of the local secret-store encryption key.
//!
//! filelift does not use the OS keyring. Persistent secrets are encrypted with a
//! 32-byte key resolved here, in priority order:
//!
//! 1. `FILELIFT_MASTER_KEY_HEX` — an explicit 64-hex-character override. This is
//!    the escape hatch for users who want to move `secrets.enc` between machines
//!    or manage the key with their own secret manager.
//! 2. A key derived (via SHA-256) from a stable per-machine identifier salted
//!    with a domain separator and the current username. No passphrase required,
//!    so the flow stays fully non-interactive.
//! 3. A persisted random key file (`~/.filelift/master.key`, `0600`) used only
//!    when the machine identifier cannot be read. This still works everywhere
//!    but weakens the "useless on another machine" property.

use std::env;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::secret_file;
use crate::target::filelift_home_dir;

/// Length of the secret-store encryption key in bytes.
pub const KEY_LEN: usize = 32;

const MASTER_KEY_ENV: &str = "FILELIFT_MASTER_KEY_HEX";
const KEY_DOMAIN: &str = "filelift:secret-store:v1";
const MASTER_KEY_FILE: &str = "master.key";

/// Resolves the 32-byte key used to encrypt the local secret store.
pub fn store_key() -> Result<[u8; KEY_LEN]> {
    if let Ok(value) = env::var(MASTER_KEY_ENV) {
        return decode_hex_key(&value)
            .with_context(|| format!("invalid {MASTER_KEY_ENV} (expected 64 hex characters)"));
    }

    match machine_uid::get() {
        Ok(machine_id) if !machine_id.trim().is_empty() => Ok(derive_key(&machine_id, &username())),
        _ => persisted_random_key(),
    }
}

/// Derives a key from a machine identifier and username with a domain separator.
fn derive_key(machine_id: &str, username: &str) -> [u8; KEY_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(KEY_DOMAIN.as_bytes());
    hasher.update([0]);
    hasher.update(machine_id.trim().as_bytes());
    hasher.update([0]);
    hasher.update(username.as_bytes());
    hasher.finalize().into()
}

fn username() -> String {
    env::var("USERNAME")
        .or_else(|_| env::var("USER"))
        .unwrap_or_else(|_| "filelift".to_string())
}

/// Last-resort key: a random key persisted with `0600` permissions.
fn persisted_random_key() -> Result<[u8; KEY_LEN]> {
    let path = filelift_home_dir()?.join(MASTER_KEY_FILE);

    if let Some(existing) = secret_file::read_to_string(&path)?
        && let Ok(key) = decode_hex_key(existing.trim())
    {
        return Ok(key);
    }

    let key = secret_file::random_bytes::<KEY_LEN>();
    secret_file::write_secure(&path, encode_hex_key(&key).as_bytes()).with_context(|| {
        format!(
            "failed to persist fallback master key at {}",
            path.display()
        )
    })?;
    Ok(key)
}

fn decode_hex_key(value: &str) -> Result<[u8; KEY_LEN]> {
    let value = value.trim();
    anyhow::ensure!(
        value.len() == KEY_LEN * 2,
        "key must be {} hex characters",
        KEY_LEN * 2
    );

    let mut key = [0_u8; KEY_LEN];
    for (index, byte) in key.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(&value[start..start + 2], 16)
            .context("key contains non-hex characters")?;
    }
    Ok(key)
}

fn encode_hex_key(bytes: &[u8; KEY_LEN]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_key_is_deterministic_for_same_inputs() {
        let first = derive_key("machine-abc", "alice");
        let second = derive_key("machine-abc", "alice");
        assert_eq!(first, second);
    }

    #[test]
    fn derive_key_differs_across_machines_and_users() {
        let base = derive_key("machine-abc", "alice");
        assert_ne!(base, derive_key("machine-xyz", "alice"));
        assert_ne!(base, derive_key("machine-abc", "bob"));
    }

    #[test]
    fn decode_hex_key_roundtrips_encoding() {
        let key = [7_u8; KEY_LEN];
        let decoded = decode_hex_key(&encode_hex_key(&key)).unwrap();
        assert_eq!(decoded, key);
    }

    #[test]
    fn decode_hex_key_rejects_wrong_length() {
        assert!(decode_hex_key("00ff").is_err());
    }

    #[test]
    fn decode_hex_key_rejects_non_hex() {
        let invalid = "zz".repeat(KEY_LEN);
        assert!(decode_hex_key(&invalid).is_err());
    }
}
