use std::io::{self, Write};

use anyhow::{Context, Result};

use crate::{
    cli::{TargetAddCommand, TargetCommands, TargetRemoveCommand, TargetUseCommand},
    diagnostic_log, i18n, output, secret, storage,
    target::{TargetStore, UploadTarget},
};

pub async fn run(command: TargetCommands) -> Result<()> {
    match command {
        TargetCommands::Add(command) => add(command).await,
        TargetCommands::List => list(),
        TargetCommands::Use(command) => use_target(command),
        TargetCommands::Remove(command) => remove(command),
    }
}

async fn add(command: TargetAddCommand) -> Result<()> {
    let mut store = TargetStore::load()?;
    let name = command.name;
    let should_prompt_region =
        command.bucket.is_none() || command.endpoint.is_none() || command.public_base_url.is_none();
    let bucket = prompt_required(command.bucket, &i18n::t("prompt-bucket"))?;
    let endpoint = prompt_required(command.endpoint, &i18n::t("prompt-endpoint"))?;
    let region = if should_prompt_region {
        prompt_with_default(command.region, &i18n::t("prompt-region"), "auto")?
    } else {
        command.region.unwrap_or_else(|| "auto".to_string())
    };
    let public_base_url =
        prompt_required(command.public_base_url, &i18n::t("prompt-public-base-url"))?;
    let public_base_url = output::normalize_public_base_url(&public_base_url)?;
    let credentials = prompt_credentials(
        command.access_key_id,
        command.secret_access_key,
        should_prompt_region,
    )?;
    let connectivity_credentials =
        resolve_connectivity_credentials(credentials.clone(), || secret::credentials(&name))?;
    let target = UploadTarget {
        provider: command.provider,
        bucket,
        endpoint,
        region,
        public_base_url,
    };

    check_target_connectivity(&target, connectivity_credentials, command.skip_check).await?;

    store.targets.insert(name.clone(), target);

    if let Some((access_key_id, secret_access_key)) = credentials {
        secret::set_credentials(&name, &access_key_id, &secret_access_key)?;
    }

    if command.set_default || store.default_target.is_none() {
        store.default_target = Some(name.clone());
    }

    store.save()?;
    diagnostic_log::record_command_result("target add", Some(&name), "success");
    println!("{}", i18n::t_args("target-added", &[("name", &name)]));
    Ok(())
}

async fn check_target_connectivity(
    target: &UploadTarget,
    credentials: Option<secret::Credentials>,
    skip_check: bool,
) -> Result<()> {
    if skip_check {
        return Ok(());
    }

    let Some(credentials) = credentials else {
        println!("{}", i18n::t("target-connectivity-skipped-no-credentials"));
        return Ok(());
    };

    println!("{}", i18n::t("target-checking-connectivity"));
    let client = storage::s3::Client::new(target.clone(), credentials).await?;
    client.check_connectivity().await?;
    println!("{}", i18n::t("target-connectivity-passed"));
    Ok(())
}

fn resolve_connectivity_credentials(
    provided: Option<(String, String)>,
    load_saved: impl FnOnce() -> Result<secret::Credentials>,
) -> Result<Option<secret::Credentials>> {
    if let Some((access_key_id, secret_access_key)) = provided {
        return Ok(Some(secret::Credentials {
            access_key_id,
            secret_access_key,
        }));
    }

    Ok(load_saved().ok())
}

fn prompt_required(value: Option<String>, label: &str) -> Result<String> {
    if let Some(value) = value {
        return Ok(value);
    }

    loop {
        let answer = prompt_line(&format!("{label}: "))?;
        if !answer.is_empty() {
            return Ok(answer);
        }

        println!(
            "{}",
            i18n::t_args("prompt-cannot-be-empty", &[("label", label)])
        );
    }
}

fn prompt_with_default(value: Option<String>, label: &str, default: &str) -> Result<String> {
    if let Some(value) = value {
        return Ok(value);
    }

    let answer = prompt_line(&format!("{label} [{default}]: "))?;
    if answer.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(answer)
    }
}

fn prompt_credentials(
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
    should_prompt_empty_credentials: bool,
) -> Result<Option<(String, String)>> {
    match (access_key_id, secret_access_key) {
        (Some(access_key_id), Some(secret_access_key)) => {
            Ok(Some((access_key_id, secret_access_key)))
        }
        (Some(access_key_id), None) => Ok(Some((
            access_key_id,
            prompt_password(&i18n::t("prompt-secret-access-key"))?,
        ))),
        (None, Some(secret_access_key)) => Ok(Some((
            prompt_required(None, &i18n::t("prompt-access-key-id"))?,
            secret_access_key,
        ))),
        (None, None) if should_prompt_empty_credentials => {
            if prompt_yes_no(&i18n::t("prompt-save-access-keys-now"), true)? {
                Ok(Some((
                    prompt_required(None, &i18n::t("prompt-access-key-id"))?,
                    prompt_password(&i18n::t("prompt-secret-access-key"))?,
                )))
            } else {
                Ok(None)
            }
        }
        (None, None) => Ok(None),
    }
}

fn prompt_yes_no(label: &str, default: bool) -> Result<bool> {
    loop {
        let answer = prompt_line(label)?;
        if let Some(value) = parse_yes_no_answer(&answer, default) {
            return Ok(value);
        }

        println!("{}", i18n::t("prompt-please-answer-yes-no"));
    }
}

fn parse_yes_no_answer(answer: &str, default: bool) -> Option<bool> {
    match answer.to_ascii_lowercase().as_str() {
        "" => Some(default),
        "n" | "no" => Some(false),
        "y" | "yes" => Some(true),
        _ => None,
    }
}

fn prompt_password(label: &str) -> Result<String> {
    let config = rpassword::ConfigBuilder::new()
        .password_feedback_mask('*')
        .build();
    rpassword::prompt_password_with_config(format!("{label}: "), config)
        .with_context(|| format!("failed to read {label}"))
}

fn prompt_line(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush().context("failed to flush prompt")?;

    let mut answer = String::new();
    let bytes_read = io::stdin()
        .read_line(&mut answer)
        .context("failed to read prompt input")?;
    if bytes_read == 0 {
        anyhow::bail!(
            "prompt input ended before `{}` was provided",
            label.trim_end_matches(": ").trim()
        );
    }

    Ok(answer.trim().to_string())
}

fn list() -> Result<()> {
    let store = TargetStore::load()?;
    if store.targets.is_empty() {
        diagnostic_log::record_command_result("target list", None, "success");
        println!("{}", i18n::t("target-no-targets-configured"));
        return Ok(());
    }

    for name in store.targets.keys() {
        let marker = if store.default_target.as_deref() == Some(name.as_str()) {
            "*"
        } else {
            " "
        };
        println!("{marker} {name}");
    }

    diagnostic_log::record_command_result("target list", None, "success");
    Ok(())
}

fn use_target(command: TargetUseCommand) -> Result<()> {
    let mut store = TargetStore::load()?;
    store
        .targets
        .contains_key(&command.name)
        .then_some(())
        .with_context(|| format!("target `{}` does not exist", command.name))?;

    store.default_target = Some(command.name.clone());
    store.save()?;
    diagnostic_log::record_command_result("target use", Some(&command.name), "success");
    println!(
        "{}",
        i18n::t_args("target-using", &[("name", &command.name)])
    );
    Ok(())
}

fn remove(command: TargetRemoveCommand) -> Result<()> {
    let mut store = TargetStore::load()?;
    store
        .targets
        .remove(&command.name)
        .with_context(|| format!("target `{}` does not exist", command.name))?;

    if store.default_target.as_deref() == Some(command.name.as_str()) {
        store.default_target = None;
    }

    secret::delete_credentials(&command.name)?;
    store.save()?;
    diagnostic_log::record_command_result("target remove", Some(&command.name), "success");
    println!(
        "{}",
        i18n::t_args("target-removed", &[("name", &command.name)])
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provided_credentials_are_used_for_connectivity_check() {
        let credentials = resolve_connectivity_credentials(
            Some(("new-id".to_string(), "new-secret".to_string())),
            || anyhow::bail!("saved credentials should not be loaded"),
        )
        .unwrap()
        .unwrap();

        assert_eq!(credentials.access_key_id, "new-id");
        assert_eq!(credentials.secret_access_key, "new-secret");
    }

    #[test]
    fn saved_credentials_are_reused_for_connectivity_check() {
        let credentials = resolve_connectivity_credentials(None, || {
            Ok(secret::Credentials {
                access_key_id: "saved-id".to_string(),
                secret_access_key: "saved-secret".to_string(),
            })
        })
        .unwrap()
        .unwrap();

        assert_eq!(credentials.access_key_id, "saved-id");
        assert_eq!(credentials.secret_access_key, "saved-secret");
    }

    #[test]
    fn missing_saved_credentials_skip_connectivity_check() {
        let credentials =
            resolve_connectivity_credentials(None, || anyhow::bail!("missing saved credentials"))
                .unwrap();

        assert!(credentials.is_none());
    }

    #[test]
    fn empty_yes_no_answer_uses_default() {
        assert_eq!(parse_yes_no_answer("", true), Some(true));
        assert_eq!(parse_yes_no_answer("", false), Some(false));
    }
}
