use anyhow::Result;

use crate::{
    cli::{LogCommands, LogExportCommand},
    diagnostic_log, i18n,
};

pub fn run(command: LogCommands) -> Result<()> {
    match command {
        LogCommands::Export(command) => export(command),
        LogCommands::Clear => clear(),
    }
}

fn export(command: LogExportCommand) -> Result<()> {
    let count = diagnostic_log::export_to(command.output.as_std_path())?;
    let count = count.to_string();
    println!(
        "{}",
        i18n::t_args(
            "log-exported",
            &[("path", command.output.as_str()), ("count", &count)]
        )
    );
    Ok(())
}

fn clear() -> Result<()> {
    diagnostic_log::clear()?;
    println!("{}", i18n::t("log-cleared"));
    Ok(())
}
