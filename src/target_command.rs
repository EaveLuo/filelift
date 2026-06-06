use anyhow::{Context, Result};

use crate::{
    cli::{TargetAddCommand, TargetCommands, TargetRemoveCommand, TargetUseCommand},
    secret,
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

    store.targets.insert(
        name.clone(),
        UploadTarget {
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

    if command.set_default || store.default_target.is_none() {
        store.default_target = Some(name.clone());
    }

    store.save()?;
    println!("Added target `{name}`.");
    Ok(())
}

fn list() -> Result<()> {
    let store = TargetStore::load()?;
    if store.targets.is_empty() {
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
    println!("Removed target `{}`.", command.name);
    Ok(())
}
