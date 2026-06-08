mod cli;
mod credentials_command;
mod diagnostic_log;
mod i18n;
mod interactive;
mod interactive_completion;
mod language_command;
mod log_command;
mod machine_key;
mod output;
mod secret;
mod secret_file;
mod storage;
mod target;
mod target_command;
mod update_check;
mod upgrade_command;
mod upload;

use clap::{CommandFactory, FromArgMatches};
use std::process::ExitCode;

use crate::cli::{Cli, Commands};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            anstream::eprintln!("{}", output::error(&format!("Error: {error:#}")));
            ExitCode::FAILURE
        }
    }
}

async fn run() -> anyhow::Result<()> {
    diagnostic_log::init();
    let command = i18n::localize_command(Cli::command());
    let matches = command.get_matches();
    let cli = Cli::from_arg_matches(&matches)?;

    let is_upgrade = matches!(cli.command, Some(Commands::Upgrade(_)));

    let (command_name, result) = match cli.command {
        Some(Commands::Target(command)) => ("target", target_command::run(command).await),
        Some(Commands::Upload(command)) => ("upload", upload::run(command).await),
        Some(Commands::Credentials(command)) => ("credentials", credentials_command::run(command)),
        Some(Commands::Log(command)) => ("log", log_command::run(command)),
        Some(Commands::Language(command)) => ("language", language_command::run(command)),
        Some(Commands::Upgrade(command)) => ("upgrade", upgrade_command::run(command)),
        None => ("interactive", interactive::run().await),
    };

    if let Err(error) = &result {
        tracing::error!(
            command = command_name,
            result = "error",
            error = %format!("{error:#}"),
            "command failed"
        );
    }

    // Skip the notice right after an upgrade run, where it would be redundant.
    if !is_upgrade {
        update_check::maybe_print_notice(env!("CARGO_PKG_VERSION"));
    }

    result
}
