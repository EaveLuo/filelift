use anyhow::{Context, Result};
use keyring_core::{Entry, Error};

const SERVICE: &str = "filelift";

pub fn set_credentials(
    config_name: &str,
    access_key_id: &str,
    secret_access_key: &str,
) -> Result<()> {
    set_secret(&secret_account(config_name, "access_key_id"), access_key_id)?;
    set_secret(
        &secret_account(config_name, "secret_access_key"),
        secret_access_key,
    )?;
    Ok(())
}

pub fn credentials(config_name: &str) -> Result<Credentials> {
    Ok(Credentials {
        access_key_id: get_secret(&secret_account(config_name, "access_key_id"))?,
        secret_access_key: get_secret(&secret_account(config_name, "secret_access_key"))?,
    })
}

pub fn delete_credentials(config_name: &str) -> Result<()> {
    delete_secret(&secret_account(config_name, "access_key_id"))?;
    delete_secret(&secret_account(config_name, "secret_access_key"))?;
    Ok(())
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
    keyring::use_native_store(false).context("failed to initialize native keyring store")
}

fn secret_account(config_name: &str, key: &str) -> String {
    format!("{config_name}:{key}")
}
