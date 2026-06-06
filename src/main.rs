mod cli;
mod config;
mod output;
mod profile;
mod secret;
mod storage;
mod upload;

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Profile(command) => profile::run(command)?,
        Commands::Upload(command) => upload::run(command).await?,
    }

    Ok(())
}
