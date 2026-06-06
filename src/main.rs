mod cli;
mod diagnostic_log;
mod i18n;
mod language_command;
mod log_command;
mod output;
mod secret;
mod storage;
mod target;
mod target_command;
mod upload;

use anyhow::Result;
use clap::{CommandFactory, FromArgMatches};

use crate::cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    diagnostic_log::init();
    let command = i18n::localize_command(Cli::command());
    let matches = command.get_matches();
    let cli = Cli::from_arg_matches(&matches)?;

    let (command_name, result) = match cli.command {
        Commands::Target(command) => ("target", target_command::run(command).await),
        Commands::Upload(command) => ("upload", upload::run(command).await),
        Commands::Log(command) => ("log", log_command::run(command)),
        Commands::Language(command) => ("language", language_command::run(command)),
    };

    if let Err(error) = &result {
        tracing::error!(
            command = command_name,
            result = "error",
            error = %format!("{error:#}"),
            "command failed"
        );
    }

    result
}
