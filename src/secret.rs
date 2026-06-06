use anyhow::{Context, Result};
use keyring_core::{Entry, Error, set_default_store};

const SERVICE: &str = "filelift";
const DIAGNOSTIC_LOG_KEY_ACCOUNT: &str = "diagnostic_log_key";

pub fn set_credentials(
    target_name: &str,
    access_key_id: &str,
    secret_access_key: &str,
) -> Result<()> {
    set_secret(&secret_account(target_name, "access_key_id"), access_key_id)?;
    set_secret(
        &secret_account(target_name, "secret_access_key"),
        secret_access_key,
    )?;
    Ok(())
}

pub fn credentials(target_name: &str) -> Result<Credentials> {
    Ok(Credentials {
        access_key_id: get_secret(&secret_account(target_name, "access_key_id"))?,
        secret_access_key: get_secret(&secret_account(target_name, "secret_access_key"))?,
    })
}

pub fn delete_credentials(target_name: &str) -> Result<()> {
    delete_secret(&secret_account(target_name, "access_key_id"))?;
    delete_secret(&secret_account(target_name, "secret_access_key"))?;
    Ok(())
}

pub fn diagnostic_log_key() -> Result<String> {
    get_secret(DIAGNOSTIC_LOG_KEY_ACCOUNT)
}

pub fn set_diagnostic_log_key(value: &str) -> Result<()> {
    set_secret(DIAGNOSTIC_LOG_KEY_ACCOUNT, value)
}

#[derive(Debug, Clone)]
pub struct Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
}

fn set_secret(account: &str, value: &str) -> Result<()> {
    ensure_native_store()?;
    Entry::new(SERVICE, account)
        .context("failed to access system keyring")?
        .set_password(value)
        .with_context(|| format!("failed to store secret for {account}"))
}

fn get_secret(account: &str) -> Result<String> {
    ensure_native_store()?;
    Entry::new(SERVICE, account)
        .context("failed to access system keyring")?
        .get_password()
        .with_context(|| format!("missing keyring secret for {account}"))
}

fn delete_secret(account: &str) -> Result<()> {
    ensure_native_store()?;
    match Entry::new(SERVICE, account)
        .context("failed to access system keyring")?
        .delete_credential()
    {
        Ok(()) => Ok(()),
        Err(Error::NoEntry) => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to delete secret for {account}")),
    }
}

fn ensure_native_store() -> Result<()> {
    set_native_store().context("failed to initialize native keyring store")
}

#[cfg(target_os = "linux")]
fn set_native_store() -> keyring_core::Result<()> {
    set_default_store(linux_keyutils_keyring_store::Store::new()?);
    Ok(())
}

#[cfg(target_os = "macos")]
fn set_native_store() -> keyring_core::Result<()> {
    set_default_store(apple_native_keyring_store::keychain::Store::new()?);
    Ok(())
}

#[cfg(target_os = "windows")]
fn set_native_store() -> keyring_core::Result<()> {
    set_default_store(windows_native_keyring_store::Store::new()?);
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn set_native_store() -> keyring_core::Result<()> {
    set_default_store(keyring_core::sample::Store::new()?);
    Ok(())
}

fn secret_account(target_name: &str, key: &str) -> String {
    format!("{target_name}:{key}")
}
