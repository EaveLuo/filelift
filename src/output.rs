use anyhow::{Context, Result};
use url::Url;

use crate::config::StorageConfig;

pub fn public_url(config: &StorageConfig, key: &str) -> Result<String> {
    let mut base = Url::parse(&config.public_base_url)
        .with_context(|| format!("invalid public_base_url `{}`", config.public_base_url))?;
    let path = format!(
        "{}/{}",
        base.path().trim_end_matches('/'),
        key.trim_start_matches('/')
    );
    base.set_path(&path);
    Ok(base.to_string())
}

pub fn markdown_image(label: &str, url: &str) -> String {
    format!("![{label}]({url})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_public_url_without_double_slashes() {
        let config = StorageConfig {
            provider: "s3".to_string(),
            bucket: "bucket".to_string(),
            endpoint: "https://example.com".to_string(),
            region: "auto".to_string(),
            public_base_url: "https://assets.example.com/base/".to_string(),
        };

        assert_eq!(
            public_url(&config, "/blog/cover.webp").unwrap(),
            "https://assets.example.com/base/blog/cover.webp"
        );
    }
}
