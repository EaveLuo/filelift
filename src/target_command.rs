use std::io::{self, Write};

use anyhow::{Context, Result};

use crate::{
    cli::{TargetAddCommand, TargetCommands, TargetRemoveCommand, TargetUseCommand},
    diagnostic_log, secret,
    target::{TargetStore, UploadTarget},
};

pub fn run(command: TargetCommands) -> Result<()> {
    match command {
        TargetCommands::Add(command) => add(command),
        TargetCommands::List => list(),
        TargetCommands::Use(command) => use_target(command),
        TargetCommands::Remove(command) => remove(command),
    }
}

fn add(command: TargetAddCommand) -> Result<()> {
    let mut store = TargetStore::load()?;
    let name = command.name;
    let should_prompt_region =
        command.bucket.is_none() || command.endpoint.is_none() || command.public_base_url.is_none();
    let bucket = prompt_required(command.bucket, "Bucket")?;
    let endpoint = prompt_required(command.endpoint, "Endpoint")?;
    let region = if should_prompt_region {
        prompt_with_default(command.region, "Region", "auto")?
    } else {
        command.region.unwrap_or_else(|| "auto".to_string())
    };
    let public_base_url = prompt_required(command.public_base_url, "Public base URL")?;
    let credentials = prompt_credentials(
        command.access_key_id,
        command.secret_access_key,
        should_prompt_region,
    )?;

    store.targets.insert(
        name.clone(),
        UploadTarget {
            provider: command.provider,
            bucket,
            endpoint,
            region,
            public_base_url,
        },
    );

    if let Some((access_key_id, secret_access_key)) = credentials {
        secret::set_credentials(&name, &access_key_id, &secret_access_key)?;
    }

    if command.set_default || store.default_target.is_none() {
        store.default_target = Some(name.clone());
    }

    store.save()?;
    diagnostic_log::record_command_result("target add", Some(&name), "success");
    println!("Added target `{name}`.");
    Ok(())
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

        println!("{label} cannot be empty.");
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
            rpassword::prompt_password("Secret access key: ")
                .context("failed to read secret access key")?,
        ))),
        (None, Some(secret_access_key)) => Ok(Some((
            prompt_required(None, "Access key ID")?,
            secret_access_key,
        ))),
        (None, None) if should_prompt_empty_credentials => {
            if prompt_yes_no("Save access keys now? [y/N]: ")? {
                Ok(Some((
                    prompt_required(None, "Access key ID")?,
                    rpassword::prompt_password("Secret access key: ")
                        .context("failed to read secret access key")?,
                )))
            } else {
                Ok(None)
            }
        }
        (None, None) => Ok(None),
    }
}

fn prompt_yes_no(label: &str) -> Result<bool> {
    loop {
        let answer = prompt_line(label)?;
        match answer.to_ascii_lowercase().as_str() {
            "" | "n" | "no" => return Ok(false),
            "y" | "yes" => return Ok(true),
            _ => println!("Please answer y or n."),
        }
    }
}

fn prompt_line(label: &str) -> Result<String> {
    print!("{label}");
    io::stdout().flush().context("failed to flush prompt")?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("failed to read prompt input")?;
    Ok(answer.trim().to_string())
}

fn list() -> Result<()> {
    let store = TargetStore::load()?;
    if store.targets.is_empty() {
        diagnostic_log::record_command_result("target list", None, "success");
        println!("No targets configured.");
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
    println!("Using target `{}`.", command.name);
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
    println!("Removed target `{}`.", command.name);
    Ok(())
}
