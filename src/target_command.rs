use std::io::{self, Write};

use anyhow::{Context, Result};

use crate::{
    cli::{
        TargetAddCommand, TargetCommands, TargetRemoveCommand, TargetUpdateCommand,
        TargetUseCommand,
    },
    diagnostic_log, i18n, output, secret, storage,
    target::{TargetStore, UploadTarget},
};

pub async fn run(command: TargetCommands) -> Result<()> {
    match command {
        TargetCommands::Add(command) => add(command).await,
        TargetCommands::Update(command) => update(command).await,
        TargetCommands::List => list(),
        TargetCommands::Use(command) => use_target(command),
        TargetCommands::Remove(command) => remove(command),
    }
}

async fn add(command: TargetAddCommand) -> Result<()> {
    let mut store = TargetStore::load()?;
    let name = command.name;
    anyhow::ensure!(
        !store.targets.contains_key(&name),
        "target `{name}` already exists; use `filelift target update {name}` to change it"
    );
    let draft = store.draft_targets.get(&name).cloned();
    if draft.is_some() {
        anstream::println!(
            "{}",
            output::info(&i18n::t_args("target-draft-resuming", &[("name", &name)]))
        );
    }
    let should_prompt_region = draft.is_some()
        || command.bucket.is_none()
        || command.endpoint.is_none()
        || command.public_base_url.is_none();
    let bucket = resolve_add_field(
        command.bucket,
        &i18n::t("prompt-bucket"),
        draft.as_ref().map(|target| target.bucket.as_str()),
    )?;
    let endpoint = resolve_add_field(
        command.endpoint,
        &i18n::t("prompt-endpoint"),
        draft.as_ref().map(|target| target.endpoint.as_str()),
    )?;
    let region = if should_prompt_region {
        prompt_with_default(
            command.region,
            &i18n::t("prompt-region"),
            draft
                .as_ref()
                .map(|target| target.region.as_str())
                .unwrap_or("auto"),
        )?
    } else {
        command
            .region
            .or_else(|| draft.as_ref().map(|target| target.region.clone()))
            .unwrap_or_else(|| "auto".to_string())
    };
    let public_base_url = resolve_add_field(
        command.public_base_url,
        &i18n::t("prompt-public-base-url"),
        draft.as_ref().map(|target| target.public_base_url.as_str()),
    )?;
    let public_base_url = output::normalize_public_base_url(&public_base_url)?;
    let saved_draft_credentials = draft.as_ref().and_then(|_| secret::credentials(&name).ok());
    let credentials = prompt_credentials(
        command.access_key_id,
        command.secret_access_key,
        CredentialPromptMode::RequireOrReuseSaved {
            saved_credentials: saved_draft_credentials,
        },
    )?;
    let target = UploadTarget {
        provider: command.provider,
        bucket,
        endpoint,
        region,
        public_base_url,
    };
    let connectivity_credentials =
        resolve_connectivity_credentials(credentials.clone(), || secret::credentials(&name))?;

    if let Err(error) =
        check_target_connectivity(&target, connectivity_credentials, command.skip_check).await
    {
        save_draft_target(&mut store, &name, target, credentials)?;
        anstream::println!(
            "{}",
            output::warning(&i18n::t_args("target-draft-saved", &[("name", &name)]))
        );
        return Err(error);
    }

    store.targets.insert(name.clone(), target);
    store.draft_targets.remove(&name);

    if let Some((access_key_id, secret_access_key)) = credentials {
        secret::set_credentials(&name, &access_key_id, &secret_access_key)?;
    }

    if command.set_default || store.default_target.is_none() {
        store.default_target = Some(name.clone());
    }

    store.save()?;
    diagnostic_log::record_command_result("target add", Some(&name), "success");
    anstream::println!(
        "{}",
        output::success(&i18n::t_args("target-added", &[("name", &name)]))
    );
    Ok(())
}

fn resolve_add_field(value: Option<String>, label: &str, draft: Option<&str>) -> Result<String> {
    match (value, draft) {
        (Some(value), _) => Ok(value),
        (None, Some(draft)) => prompt_existing_default(label, draft),
        (None, None) => prompt_required(None, label),
    }
}

fn save_draft_target(
    store: &mut TargetStore,
    name: &str,
    target: UploadTarget,
    credentials: Option<(String, String)>,
) -> Result<()> {
    store.draft_targets.insert(name.to_string(), target);
    store.save()?;
    if let Some((access_key_id, secret_access_key)) = credentials {
        let _ = secret::set_credentials(name, &access_key_id, &secret_access_key);
    }
    Ok(())
}

async fn update(command: TargetUpdateCommand) -> Result<()> {
    let mut store = TargetStore::load()?;
    let should_prompt_metadata = should_prompt_update_metadata(&command);
    let name = command.name;
    let is_draft_resume =
        !store.targets.contains_key(&name) && store.draft_targets.contains_key(&name);
    let existing = store
        .targets
        .get(&name)
        .or_else(|| store.draft_targets.get(&name))
        .with_context(|| format!("target `{name}` does not exist"))?;
    if is_draft_resume {
        anstream::println!(
            "{}",
            output::info(&i18n::t_args("target-draft-resuming", &[("name", &name)]))
        );
    }

    let provider = resolve_update_field(
        command.provider,
        &i18n::t("prompt-provider"),
        &existing.provider,
        should_prompt_metadata,
    )?;
    let bucket = resolve_update_field(
        command.bucket,
        &i18n::t("prompt-bucket"),
        &existing.bucket,
        should_prompt_metadata,
    )?;
    let endpoint = resolve_update_field(
        command.endpoint,
        &i18n::t("prompt-endpoint"),
        &existing.endpoint,
        should_prompt_metadata,
    )?;
    let region = resolve_update_field(
        command.region,
        &i18n::t("prompt-region"),
        &existing.region,
        should_prompt_metadata,
    )?;
    let public_base_url = resolve_update_field(
        command.public_base_url,
        &i18n::t("prompt-public-base-url"),
        &existing.public_base_url,
        should_prompt_metadata,
    )?;
    let public_base_url = output::normalize_public_base_url(&public_base_url)?;
    let credentials = prompt_credentials(
        command.access_key_id,
        command.secret_access_key,
        CredentialPromptMode::PromptWhenMissingSaved {
            has_saved_credentials: secret::credentials(&name).is_ok(),
        },
    )?;
    let connectivity_credentials =
        resolve_connectivity_credentials(credentials.clone(), || secret::credentials(&name))?;
    let target = UploadTarget {
        provider,
        bucket,
        endpoint,
        region,
        public_base_url,
    };

    check_target_connectivity(&target, connectivity_credentials, command.skip_check).await?;

    store.targets.insert(name.clone(), target);
    store.draft_targets.remove(&name);

    if let Some((access_key_id, secret_access_key)) = credentials {
        secret::set_credentials(&name, &access_key_id, &secret_access_key)?;
    }

    if command.set_default || (is_draft_resume && store.default_target.is_none()) {
        store.default_target = Some(name.clone());
    }

    store.save()?;
    diagnostic_log::record_command_result("target update", Some(&name), "success");
    let message = if is_draft_resume {
        i18n::t_args("target-added", &[("name", &name)])
    } else {
        i18n::t_args("target-updated", &[("name", &name)])
    };
    anstream::println!("{}", output::success(&message));
    Ok(())
}

fn should_prompt_update_metadata(command: &TargetUpdateCommand) -> bool {
    command.provider.is_none()
        && command.bucket.is_none()
        && command.endpoint.is_none()
        && command.region.is_none()
        && command.public_base_url.is_none()
        && command.access_key_id.is_none()
        && command.secret_access_key.is_none()
}

fn resolve_update_field(
    value: Option<String>,
    label: &str,
    current: &str,
    should_prompt: bool,
) -> Result<String> {
    if let Some(value) = value {
        return Ok(value);
    }

    if should_prompt {
        return prompt_existing_default(label, current);
    }

    Ok(current.to_string())
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
        anstream::println!(
            "{}",
            output::warning(&i18n::t("target-connectivity-skipped-no-credentials"))
        );
        return Ok(());
    };

    let progress = output::spinner(i18n::t("target-checking-connectivity"));
    let client = match storage::s3::Client::new(target.clone(), credentials).await {
        Ok(client) => client,
        Err(error) => {
            progress.finish_and_clear();
            return Err(error);
        }
    };
    if let Err(error) = client.check_connectivity().await {
        progress.finish_and_clear();
        return Err(error);
    }
    progress.finish_and_clear();
    anstream::println!(
        "{}",
        output::success(&i18n::t("target-connectivity-passed"))
    );
    Ok(())
}

fn resolve_connectivity_credentials(
    provided: Option<(String, String)>,
    load_saved: impl FnOnce() -> Result<secret::Credentials>,
) -> Result<Option<secret::Credentials>> {
    if let Some((access_key_id, secret_access_key)) = provided {
        return Ok(Some(credentials_from_pair((
            access_key_id,
            secret_access_key,
        ))));
    }

    Ok(load_saved().ok())
}

fn credentials_from_pair(
    (access_key_id, secret_access_key): (String, String),
) -> secret::Credentials {
    secret::Credentials {
        access_key_id,
        secret_access_key,
    }
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

fn prompt_existing_default(label: &str, current: &str) -> Result<String> {
    let answer = prompt_line(&format!("{label} [{current}]: "))?;
    if answer.is_empty() {
        Ok(current.to_string())
    } else {
        Ok(answer)
    }
}

fn prompt_credentials(
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
    mode: CredentialPromptMode,
) -> Result<Option<(String, String)>> {
    if let CredentialPromptMode::RequireOrReuseSaved {
        saved_credentials: Some(saved_credentials),
    } = mode
    {
        let access_key_id = match access_key_id {
            Some(access_key_id) => access_key_id,
            None => prompt_existing_default(
                &i18n::t("prompt-access-key-id"),
                &saved_credentials.access_key_id,
            )?,
        };
        let secret_access_key = match secret_access_key {
            Some(secret_access_key) => secret_access_key,
            None => prompt_password_with_default(
                &i18n::t("prompt-secret-access-key"),
                &saved_credentials.secret_access_key,
            )?,
        };
        return Ok(Some((access_key_id, secret_access_key)));
    }

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
        (None, None) if mode.should_prompt_for_missing_credentials() => Ok(Some((
            prompt_required(None, &i18n::t("prompt-access-key-id"))?,
            prompt_password(&i18n::t("prompt-secret-access-key"))?,
        ))),
        (None, None) => Ok(None),
    }
}

#[derive(Debug, Clone)]
enum CredentialPromptMode {
    RequireOrReuseSaved {
        saved_credentials: Option<secret::Credentials>,
    },
    PromptWhenMissingSaved {
        has_saved_credentials: bool,
    },
}

impl CredentialPromptMode {
    fn should_prompt_for_missing_credentials(&self) -> bool {
        match self {
            Self::RequireOrReuseSaved { .. } => true,
            Self::PromptWhenMissingSaved {
                has_saved_credentials,
            } => !has_saved_credentials,
        }
    }
}

fn prompt_password(label: &str) -> Result<String> {
    let config = rpassword::ConfigBuilder::new()
        .password_feedback_mask('*')
        .build();
    rpassword::prompt_password_with_config(format!("{label}: "), config)
        .with_context(|| format!("failed to read {label}"))
}

fn prompt_password_with_default(label: &str, current: &str) -> Result<String> {
    let config = rpassword::ConfigBuilder::new()
        .password_feedback_mask('*')
        .build();
    let answer = rpassword::prompt_password_with_config(format!("{label} [saved]: "), config)
        .with_context(|| format!("failed to read {label}"))?;
    if answer.is_empty() {
        Ok(current.to_string())
    } else {
        Ok(answer)
    }
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
    anstream::println!(
        "{}",
        output::success(&i18n::t_args("target-using", &[("name", &command.name)]))
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
    anstream::println!(
        "{}",
        output::success(&i18n::t_args("target-removed", &[("name", &command.name)]))
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
}
