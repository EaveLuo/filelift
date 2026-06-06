use anyhow::Result;

use crate::{
    cli::{LogCommands, LogExportCommand},
    diagnostic_log,
};

pub fn run(command: LogCommands) -> Result<()> {
    match command {
        LogCommands::Export(command) => export(command),
        LogCommands::Clear => clear(),
    }
}

fn export(command: LogExportCommand) -> Result<()> {
    let count = diagnostic_log::export_to(command.output.as_std_path())?;
    println!(
        "Exported diagnostic log to {} ({count} events). Review it before sharing.",
        command.output
    );
    Ok(())
}

fn clear() -> Result<()> {
    diagnostic_log::clear()?;
    println!("Cleared diagnostic logs.");
    Ok(())
}
