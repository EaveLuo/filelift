mod cli;
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
    let cli = Cli::parse();

    match cli.command {
        Commands::Target(command) => target_command::run(command)?,
        Commands::Upload(command) => upload::run(command).await?,
    }

    Ok(())
}
