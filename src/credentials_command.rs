//! `filelift credentials export` — materialize stored credentials as
//! environment-variable assignments so they can be injected into CI, containers,
//! or AI-agent sandboxes that cannot reach the local secret store.

use anyhow::{Context, Result};

use crate::{
    cli::{CredentialsCommands, CredentialsExportCommand, ExportFormat},
    diagnostic_log, i18n, output, secret,
    target::TargetStore,
};

pub fn run(command: CredentialsCommands) -> Result<()> {
    match command {
        CredentialsCommands::Export(command) => export(command),
    }
}

fn export(command: CredentialsExportCommand) -> Result<()> {
    let store = TargetStore::load()?;
    let target_name = store.active_target_name(command.name.as_deref())?;

    let credentials = secret::stored_credentials(&target_name)?
        .with_context(|| i18n::t_args("credentials-export-missing", &[("target", &target_name)]))?;

    let (access_key_var, secret_key_var) = secret::target_env_var_names(&target_name);
    let prefix = match command.format {
        ExportFormat::Dotenv => "",
        ExportFormat::Shell => "export ",
    };

    println!("{prefix}{access_key_var}={}", credentials.access_key_id);
    println!("{prefix}{secret_key_var}={}", credentials.secret_access_key);

    anstream::eprintln!(
        "{}",
        output::warning(&i18n::t_args(
            "credentials-export-warning",
            &[("target", &target_name)]
        ))
    );

    diagnostic_log::record_command_result("credentials export", Some(&target_name), "success");
    Ok(())
}
