use std::{collections::BTreeMap, fs};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TargetStore {
    pub default_target: Option<String>,
    #[serde(default)]
    pub targets: BTreeMap<String, UploadTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadTarget {
    pub provider: String,
    pub bucket: String,
    pub endpoint: String,
    pub region: String,
    pub public_base_url: String,
}

impl TargetStore {
    pub fn load() -> Result<Self> {
        let path = target_store_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read target store at {path}"))?;
        toml::from_str(&content).with_context(|| format!("failed to parse target store at {path}"))
    }

    pub fn save(&self) -> Result<()> {
        let path = target_store_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create target store directory at {parent}"))?;
        }

        let content = toml::to_string_pretty(self).context("failed to serialize target store")?;
        fs::write(&path, content).with_context(|| format!("failed to write target store at {path}"))
    }

    pub fn active_target_name(&self, override_name: Option<&str>) -> Result<String> {
        if let Some(name) = override_name {
            return Ok(name.to_string());
        }

        self.default_target
            .clone()
            .context("no target selected; run `filelift target use <name>` or pass --target")
    }
}

pub fn target_store_path() -> Result<Utf8PathBuf> {
    let target_dir = dirs::config_dir().context("could not resolve user target store directory")?;
    let path = target_dir.join("filelift").join("targets.toml");
    Utf8PathBuf::from_path_buf(path).map_err(|path| {
        anyhow::anyhow!(
            "target store path contains non-UTF-8 characters: {}",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_target_overrides_default_target() {
        let store = TargetStore {
            default_target: Some("r2-blog".to_string()),
            targets: BTreeMap::new(),
        };

        assert_eq!(
            store.active_target_name(Some("s3-backup")).unwrap(),
            "s3-backup"
        );
    }

    #[test]
    fn default_target_is_used_when_no_override_is_given() {
        let store = TargetStore {
            default_target: Some("r2-blog".to_string()),
            targets: BTreeMap::new(),
        };

        assert_eq!(store.active_target_name(None).unwrap(), "r2-blog");
    }

    #[test]
    fn missing_target_selection_returns_actionable_error() {
        let store = TargetStore::default();
        let error = store.active_target_name(None).unwrap_err().to_string();

        assert!(error.contains("filelift target use"));
        assert!(error.contains("--target"));
    }
}
