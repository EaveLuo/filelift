use anyhow::Result;

use crate::{
    cli::{LanguageCommands, LanguageUseCommand},
    i18n::{self, Language},
};

pub fn run(command: LanguageCommands) -> Result<()> {
    match command {
        LanguageCommands::Show => show(),
        LanguageCommands::Use(command) => use_language(command),
    }
}

fn show() -> Result<()> {
    println!(
        "{}",
        i18n::t_args("language-current", &[("language", i18n::current().code())])
    );
    Ok(())
}

fn use_language(command: LanguageUseCommand) -> Result<()> {
    let language = Language::parse(&command.language)?;
    i18n::save(language)?;
    println!(
        "{}",
        i18n::t_args("language-current", &[("language", language.code())])
    );
    Ok(())
}
