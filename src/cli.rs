use camino::Utf8PathBuf;
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "filelift", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    #[command(about = "Manage upload targets")]
    #[command(subcommand)]
    Target(TargetCommands),
    #[command(about = "Upload a file or directory")]
    Upload(UploadCommand),
    #[command(about = "Manage diagnostic logs")]
    #[command(subcommand)]
    Log(LogCommands),
    #[command(about = "Manage CLI language")]
    #[command(subcommand)]
    Language(LanguageCommands),
}

#[derive(Debug, Subcommand)]
pub enum TargetCommands {
    #[command(about = "Add an upload target")]
    Add(TargetAddCommand),
    #[command(about = "Update an upload target")]
    Update(TargetUpdateCommand),
    #[command(about = "List configured upload targets")]
    List,
    #[command(about = "Set the default upload target")]
    Use(TargetUseCommand),
    #[command(about = "Remove an upload target")]
    Remove(TargetRemoveCommand),
}

#[derive(Debug, Subcommand)]
pub enum LogCommands {
    #[command(about = "Export decrypted diagnostic logs")]
    Export(LogExportCommand),
    #[command(about = "Clear encrypted diagnostic logs")]
    Clear,
}

#[derive(Debug, Subcommand)]
pub enum LanguageCommands {
    #[command(about = "Show the current CLI language")]
    Show,
    #[command(about = "Set the CLI language")]
    Use(LanguageUseCommand),
}

#[derive(Debug, Args)]
pub struct LogExportCommand {
    #[arg(long, default_value = "filelift-debug-log.jsonl")]
    pub output: Utf8PathBuf,
}

#[derive(Debug, Args)]
pub struct TargetAddCommand {
    pub name: String,
    #[arg(long, default_value = "s3")]
    pub provider: String,
    #[arg(long)]
    pub bucket: Option<String>,
    #[arg(long)]
    pub endpoint: Option<String>,
    #[arg(long)]
    pub region: Option<String>,
    #[arg(long)]
    pub public_base_url: Option<String>,
    #[arg(long)]
    pub folder: Option<String>,
    #[arg(long)]
    pub access_key_id: Option<String>,
    #[arg(long)]
    pub secret_access_key: Option<String>,
    #[arg(long)]
    pub set_default: bool,
    #[arg(long)]
    pub skip_check: bool,
}

#[derive(Debug, Args)]
pub struct LanguageUseCommand {
    pub language: String,
}

#[derive(Debug, Args)]
pub struct TargetUpdateCommand {
    pub name: Option<String>,
    #[arg(long)]
    pub provider: Option<String>,
    #[arg(long)]
    pub bucket: Option<String>,
    #[arg(long)]
    pub endpoint: Option<String>,
    #[arg(long)]
    pub region: Option<String>,
    #[arg(long)]
    pub public_base_url: Option<String>,
    #[arg(long)]
    pub folder: Option<String>,
    #[arg(long)]
    pub access_key_id: Option<String>,
    #[arg(long)]
    pub secret_access_key: Option<String>,
    #[arg(long)]
    pub set_default: bool,
    #[arg(long)]
    pub skip_check: bool,
}

#[derive(Debug, Args)]
pub struct TargetUseCommand {
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub struct TargetRemoveCommand {
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub struct UploadCommand {
    pub path: Utf8PathBuf,
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long, alias = "prefix")]
    pub folder: Option<String>,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long, alias = "no-target-folder")]
    pub ignore_target_folder: bool,
    #[arg(long)]
    pub markdown: bool,
    #[arg(long)]
    pub dry_run: bool,
}
