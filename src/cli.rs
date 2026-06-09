use camino::Utf8PathBuf;
use clap::{Args, Parser, Subcommand, ValueEnum};

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
    #[command(about = "Upload one or more files or directories")]
    Upload(UploadCommand),
    #[command(about = "Materialize stored credentials for injection")]
    #[command(subcommand)]
    Credentials(CredentialsCommands),
    #[command(about = "Manage diagnostic logs")]
    #[command(subcommand)]
    Log(LogCommands),
    #[command(about = "Manage CLI language")]
    #[command(subcommand)]
    Language(LanguageCommands),
    #[command(
        about = "Update filelift to the latest release",
        visible_alias = "update"
    )]
    Upgrade(UpgradeCommand),
}

#[derive(Debug, Args)]
pub struct UpgradeCommand {
    /// Install a specific version (e.g. `0.3.0` or `v0.3.0`); defaults to latest.
    #[arg(long)]
    pub version: Option<String>,
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
pub enum CredentialsCommands {
    #[command(about = "Print stored credentials as environment variables")]
    Export(CredentialsExportCommand),
}

#[derive(Debug, Args)]
pub struct CredentialsExportCommand {
    /// Target whose credentials to export; defaults to the active target.
    pub name: Option<String>,
    /// Output format for the exported variables.
    #[arg(long, value_enum, default_value_t = ExportFormat::Dotenv)]
    pub format: ExportFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ExportFormat {
    /// `KEY=value` lines suitable for a `.env` file.
    Dotenv,
    /// `export KEY=value` lines suitable for `eval` in a POSIX shell.
    Shell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Human-friendly text output.
    Text,
    /// Machine-readable JSON output.
    Json,
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
    /// Read the secret access key from stdin (one secret per invocation).
    #[arg(long, conflicts_with = "secret_access_key")]
    pub secret_access_key_stdin: bool,
    /// Never prompt; fail fast when a required value is missing.
    #[arg(long)]
    pub non_interactive: bool,
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
    /// Read the secret access key from stdin (one secret per invocation).
    #[arg(long, conflicts_with = "secret_access_key")]
    pub secret_access_key_stdin: bool,
    /// Never prompt; fail fast when a required value is missing.
    #[arg(long)]
    pub non_interactive: bool,
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
    /// Files or directories to upload (one or more).
    #[arg(required = true, num_args = 1.., value_name = "PATH")]
    pub paths: Vec<Utf8PathBuf>,
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
    /// Output format for the resulting URLs.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub output: OutputFormat,
}
