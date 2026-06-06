use anyhow::{Context, Result};

use crate::{
    cli::{ProfileAddCommand, ProfileCommands, ProfileRemoveCommand, ProfileUseCommand},
    config::{Config, StorageProfile},
    secret,
};

pub fn run(command: ProfileCommands) -> Result<()> {
    match command {
        ProfileCommands::Add(command) => add(command),
        ProfileCommands::List => list(),
        ProfileCommands::Use(command) => use_profile(command),
        ProfileCommands::Remove(command) => remove(command),
    }
}

fn add(command: ProfileAddCommand) -> Result<()> {
    let mut config = Config::load()?;
    let name = command.name;

    config.profiles.insert(
        name.clone(),
        StorageProfile {
            provider: command.provider,
            bucket: command.bucket,
            endpoint: command.endpoint,
            region: command.region,
            public_base_url: command.public_base_url,
        },
    );

    if let (Some(access_key_id), Some(secret_access_key)) =
        (command.access_key_id, command.secret_access_key)
    {
        secret::set_credentials(&name, &access_key_id, &secret_access_key)?;
    }

    if command.set_default || config.default_profile.is_none() {
        config.default_profile = Some(name.clone());
    }

    config.save()?;
    println!("Added profile `{name}`.");
    Ok(())
}

fn list() -> Result<()> {
    let config = Config::load()?;
    if config.profiles.is_empty() {
        println!("No profiles configured.");
        return Ok(());
    }

    for name in config.profiles.keys() {
        let marker = if config.default_profile.as_deref() == Some(name.as_str()) {
            "*"
        } else {
            " "
        };
        println!("{marker} {name}");
    }

    Ok(())
}

fn use_profile(command: ProfileUseCommand) -> Result<()> {
    let mut config = Config::load()?;
    config
        .profiles
        .contains_key(&command.name)
        .then_some(())
        .with_context(|| format!("profile `{}` does not exist", command.name))?;

    config.default_profile = Some(command.name.clone());
    config.save()?;
    println!("Using profile `{}`.", command.name);
    Ok(())
}

fn remove(command: ProfileRemoveCommand) -> Result<()> {
    let mut config = Config::load()?;
    config
        .profiles
        .remove(&command.name)
        .with_context(|| format!("profile `{}` does not exist", command.name))?;

    if config.default_profile.as_deref() == Some(command.name.as_str()) {
        config.default_profile = None;
    }

    secret::delete_credentials(&command.name)?;
    config.save()?;
    println!("Removed profile `{}`.", command.name);
    Ok(())
}
