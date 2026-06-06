mod cli;
mod diagnostic_log;
mod log_command;
mod output;
mod secret;
mod storage;
mod target;
mod target_command;
mod upload;

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    diagnostic_log::init();
    let cli = Cli::parse();

    let (command_name, result) = match cli.command {
        Commands::Target(command) => ("target", target_command::run(command)),
        Commands::Upload(command) => ("upload", upload::run(command).await),
        Commands::Log(command) => ("log", log_command::run(command)),
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
