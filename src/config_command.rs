use anyhow::{Context, Result};

use crate::{
    cli::{ConfigAddCommand, ConfigCommands, ConfigRemoveCommand, ConfigUseCommand},
    config::{Config, StorageConfig},
    secret,
};

pub fn run(command: ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::Add(command) => add(command),
        ConfigCommands::List => list(),
        ConfigCommands::Use(command) => use_config(command),
        ConfigCommands::Remove(command) => remove(command),
    }
}

fn add(command: ConfigAddCommand) -> Result<()> {
    let mut config = Config::load()?;
    let name = command.name;

    config.configs.insert(
        name.clone(),
        StorageConfig {
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

    if command.set_default || config.default_config.is_none() {
        config.default_config = Some(name.clone());
    }

    config.save()?;
    println!("Added config `{name}`.");
    Ok(())
}

fn list() -> Result<()> {
    let config = Config::load()?;
    if config.configs.is_empty() {
        println!("No configs configured.");
        return Ok(());
    }

    for name in config.configs.keys() {
        let marker = if config.default_config.as_deref() == Some(name.as_str()) {
            "*"
        } else {
            " "
        };
        println!("{marker} {name}");
    }

    Ok(())
}

fn use_config(command: ConfigUseCommand) -> Result<()> {
    let mut config = Config::load()?;
    config
        .configs
        .contains_key(&command.name)
        .then_some(())
        .with_context(|| format!("config `{}` does not exist", command.name))?;

    config.default_config = Some(command.name.clone());
    config.save()?;
    println!("Using config `{}`.", command.name);
    Ok(())
}

fn remove(command: ConfigRemoveCommand) -> Result<()> {
    let mut config = Config::load()?;
    config
        .configs
        .remove(&command.name)
        .with_context(|| format!("config `{}` does not exist", command.name))?;

    if config.default_config.as_deref() == Some(command.name.as_str()) {
        config.default_config = None;
    }

    secret::delete_credentials(&command.name)?;
    config.save()?;
    println!("Removed config `{}`.", command.name);
    Ok(())
}
