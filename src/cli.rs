use camino::Utf8PathBuf;
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "filelift", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(about = "Manage storage configs")]
    #[command(subcommand)]
    Config(ConfigCommands),
    #[command(about = "Upload a file or directory")]
    Upload(UploadCommand),
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    #[command(about = "Add or update a storage config")]
    Add(ConfigAddCommand),
    #[command(about = "List configured storage configs")]
    List,
    #[command(about = "Set the default storage config")]
    Use(ConfigUseCommand),
    #[command(about = "Remove a storage config")]
    Remove(ConfigRemoveCommand),
}

#[derive(Debug, Args)]
pub struct ConfigAddCommand {
    pub name: String,
    #[arg(long, default_value = "s3")]
    pub provider: String,
    #[arg(long)]
    pub bucket: String,
    #[arg(long)]
    pub endpoint: String,
    #[arg(long, default_value = "auto")]
    pub region: String,
    #[arg(long)]
    pub public_base_url: String,
    #[arg(long)]
    pub access_key_id: Option<String>,
    #[arg(long)]
    pub secret_access_key: Option<String>,
    #[arg(long)]
    pub set_default: bool,
}

#[derive(Debug, Args)]
pub struct ConfigUseCommand {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct ConfigRemoveCommand {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct UploadCommand {
    pub path: Utf8PathBuf,
    #[arg(long)]
    pub config: Option<String>,
    #[arg(long)]
    pub prefix: Option<String>,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub recursive: bool,
    #[arg(long)]
    pub markdown: bool,
    #[arg(long)]
    pub dry_run: bool,
}
