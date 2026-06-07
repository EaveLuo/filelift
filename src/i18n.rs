use std::{collections::HashMap, env, fs};

use anyhow::{Context, Result, bail};
use clap::Command;
use i18n_embed::{LanguageLoader, fluent::FluentLanguageLoader, unic_langid::LanguageIdentifier};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};

use crate::target::filelift_home_dir;

#[derive(RustEmbed)]
#[folder = "i18n"]
struct Localizations;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Zh,
}

impl Language {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "en" | "english" => Ok(Self::En),
            "zh" | "cn" | "chinese" | "zh-cn" | "zh_cn" => Ok(Self::Zh),
            _ => bail!("unsupported language `{value}`; use `en` or `zh`"),
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Zh => "zh",
        }
    }

    fn fluent_id(self) -> LanguageIdentifier {
        match self {
            Self::En => "en".parse().expect("valid fallback language id"),
            Self::Zh => "zh-CN".parse().expect("valid Chinese language id"),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LanguageConfig {
    language: Language,
}

pub fn current() -> Language {
    env::var("FILELIFT_LANG")
        .ok()
        .and_then(|value| Language::parse(&value).ok())
        .or_else(|| load().ok())
        .unwrap_or_default()
}

pub fn save(language: Language) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create language config directory at {}",
                parent.display()
            )
        })?;
    }

    let content = toml::to_string_pretty(&LanguageConfig { language })
        .context("failed to serialize language config")?;
    fs::write(&path, content)
        .with_context(|| format!("failed to write language config at {}", path.display()))
}

pub fn t(message_id: &str) -> String {
    loader().get(message_id)
}

pub fn t_args(message_id: &str, args: &[(&str, &str)]) -> String {
    let args = args.iter().copied().collect::<HashMap<_, _>>();
    loader().get_args(message_id, args)
}

pub fn localize_command(command: Command) -> Command {
    command
        .about(t("cli-about"))
        .mut_subcommand("target", |command| {
            command
                .about(t("cmd-target-about"))
                .mut_subcommand("add", |command| command.about(t("cmd-target-add-about")))
                .mut_subcommand("update", |command| {
                    command.about(t("cmd-target-update-about"))
                })
                .mut_subcommand("list", |command| command.about(t("cmd-target-list-about")))
                .mut_subcommand("use", |command| command.about(t("cmd-target-use-about")))
                .mut_subcommand("remove", |command| {
                    command.about(t("cmd-target-remove-about"))
                })
        })
        .mut_subcommand("upload", |command| command.about(t("cmd-upload-about")))
        .mut_subcommand("log", |command| {
            command
                .about(t("cmd-log-about"))
                .mut_subcommand("export", |command| command.about(t("cmd-log-export-about")))
                .mut_subcommand("clear", |command| command.about(t("cmd-log-clear-about")))
        })
        .mut_subcommand("language", |command| {
            command
                .about(t("cmd-language-about"))
                .mut_subcommand("show", |command| {
                    command.about(t("cmd-language-show-about"))
                })
                .mut_subcommand("use", |command| command.about(t("cmd-language-use-about")))
        })
        .mut_subcommand("completions", |command| {
            command.about(t("cmd-completions-about"))
        })
}

fn loader() -> FluentLanguageLoader {
    let fallback_language: LanguageIdentifier = "en".parse().expect("valid fallback language id");
    let loader = FluentLanguageLoader::new("filelift", fallback_language.clone());
    let selected_language = current().fluent_id();
    let languages = if selected_language == fallback_language {
        vec![fallback_language]
    } else {
        vec![selected_language, fallback_language]
    };

    loader
        .load_languages(&Localizations, &languages)
        .expect("failed to load embedded localization resources");
    loader.set_use_isolating(false);
    loader
}

fn load() -> Result<Language> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Language::En);
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read language config at {}", path.display()))?;
    let config: LanguageConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse language config at {}", path.display()))?;
    Ok(config.language)
}

fn config_path() -> Result<std::path::PathBuf> {
    Ok(filelift_home_dir()?.join("language.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_english() {
        assert_eq!(Language::default(), Language::En);
    }

    #[test]
    fn accepts_chinese_aliases() {
        assert_eq!(Language::parse("zh-CN").unwrap(), Language::Zh);
        assert_eq!(Language::parse("chinese").unwrap(), Language::Zh);
    }
}
