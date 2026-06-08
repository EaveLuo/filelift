//! Low-level helpers for reading and writing local secret files with
//! restrictive permissions. These are intentionally storage-agnostic so the
//! secret store, the fallback master key, and any future on-disk secret share
//! one hardened write path.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chacha20poly1305::aead::{OsRng, rand_core::RngCore};

/// Reads a file to a string, returning `None` when the file does not exist.
pub fn read_to_string(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

/// Reads a file to bytes, returning `None` when the file does not exist.
pub fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

/// Writes `bytes` to `path` with `0600` permissions, creating parent
/// directories as needed. On Windows the permission step is a no-op; the file
/// inherits the user-profile ACLs of `~/.filelift`.
pub fn write_secure(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
    restrict_permissions(path)?;
    Ok(())
}

/// Generates `N` cryptographically-random bytes.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0_u8; N];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set 0600 permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.txt");
        assert_eq!(read_to_string(&path).unwrap(), None);
    }

    #[test]
    fn write_secure_creates_parent_dirs_and_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("secret.bin");
        write_secure(&path, b"hello").unwrap();
        assert_eq!(read_to_string(&path).unwrap().as_deref(), Some("hello"));
    }

    #[cfg(unix)]
    #[test]
    fn write_secure_sets_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.bin");
        write_secure(&path, b"hello").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn random_bytes_are_not_all_zero() {
        let bytes = random_bytes::<32>();
        assert!(bytes.iter().any(|&byte| byte != 0));
    }
}
