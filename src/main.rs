mod cli;
mod completion_command;
mod diagnostic_log;
mod i18n;
mod interactive;
mod language_command;
mod log_command;
mod output;
mod secret;
mod storage;
mod target;
mod target_command;
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
    if run_internal_completion()? {
        return Ok(());
    }

    let command = i18n::localize_command(Cli::command());
    let matches = command.get_matches();
    let cli = Cli::from_arg_matches(&matches)?;

    let (command_name, result) = match cli.command {
        Some(Commands::Target(command)) => ("target", target_command::run(command).await),
        Some(Commands::Upload(command)) => ("upload", upload::run(command).await),
        Some(Commands::Log(command)) => ("log", log_command::run(command)),
        Some(Commands::Language(command)) => ("language", language_command::run(command)),
        Some(Commands::Completions(command)) => ("completions", completion_command::run(command)),
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

    result
}

fn run_internal_completion() -> anyhow::Result<bool> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).map(String::as_str) != Some("__complete") {
        return Ok(false);
    }

    match args.get(2).map(String::as_str) {
        Some("targets") => completion_command::complete_targets()?,
        _ => anyhow::bail!("unknown completion source"),
    }

    Ok(true)
}
