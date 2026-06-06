use std::{collections::BTreeMap, fs};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub default_config: Option<String>,
    #[serde(default)]
    pub configs: BTreeMap<String, StorageConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub provider: String,
    pub bucket: String,
    pub endpoint: String,
    pub region: String,
    pub public_base_url: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {path}"))?;
        toml::from_str(&content).with_context(|| format!("failed to parse config at {path}"))
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create config directory at {parent}"))?;
        }

        let content = toml::to_string_pretty(self).context("failed to serialize config")?;
        fs::write(&path, content).with_context(|| format!("failed to write config at {path}"))
    }

    pub fn active_config_name(&self, override_name: Option<&str>) -> Result<String> {
        if let Some(name) = override_name {
            return Ok(name.to_string());
        }

        self.default_config
            .clone()
            .context("no config selected; run `filelift config use <name>` or pass --config")
    }
}

pub fn config_path() -> Result<Utf8PathBuf> {
    let config_dir = dirs::config_dir().context("could not resolve user config directory")?;
    let path = config_dir.join("filelift").join("config.toml");
    Utf8PathBuf::from_path_buf(path).map_err(|path| {
        anyhow::anyhow!(
            "config path contains non-UTF-8 characters: {}",
            path.display()
        )
    })
}
