use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::IsTerminal;
use url::Url;

use crate::target::UploadTarget;

pub fn success(message: &str) -> String {
    paint(message, anstyle::AnsiColor::Green)
}

pub fn warning(message: &str) -> String {
    paint(message, anstyle::AnsiColor::Yellow)
}

pub fn error(message: &str) -> String {
    paint(message, anstyle::AnsiColor::Red)
}

pub fn info(message: &str) -> String {
    paint(message, anstyle::AnsiColor::Cyan)
}

pub fn spinner(message: impl Into<String>) -> ProgressBar {
    if !std::io::stderr().is_terminal() {
        return ProgressBar::hidden();
    }

    let progress = ProgressBar::new_spinner();
    progress.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .expect("valid spinner template")
            .tick_chars("|/-\\ "),
    );
    progress.set_message(message.into());
    progress.enable_steady_tick(std::time::Duration::from_millis(120));
    progress
}

pub fn upload_progress(total: u64) -> ProgressBar {
    if !std::io::stderr().is_terminal() {
        return ProgressBar::hidden();
    }

    if total <= 1 {
        return spinner("Uploading file...");
    }

    let progress = ProgressBar::new(total);
    progress.set_style(
        ProgressStyle::with_template("{bar:28.cyan/blue} {pos}/{len} {msg}")
            .expect("valid progress template")
            .progress_chars("=>-"),
    );
    progress.set_message("Uploading files...");
    progress
}

fn paint(message: &str, color: anstyle::AnsiColor) -> String {
    let style = anstyle::Style::new().fg_color(Some(color.into()));
    format!("{style}{message}{style:#}")
}

pub fn public_url(target: &UploadTarget, key: &str) -> Result<String> {
    let public_base_url = normalize_public_base_url(&target.public_base_url)?;
    let mut base = Url::parse(&public_base_url)
        .with_context(|| format!("invalid public_base_url `{}`", target.public_base_url))?;
    let path = format!(
        "{}/{}",
        base.path().trim_end_matches('/'),
        key.trim_start_matches('/')
    );
    base.set_path(&path);
    Ok(base.to_string())
}

pub fn normalize_public_base_url(value: &str) -> Result<String> {
    let trimmed = value.trim();
    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };

    let url =
        Url::parse(&candidate).with_context(|| format!("invalid public_base_url `{value}`"))?;
    Ok(url.to_string().trim_end_matches('/').to_string())
}

pub fn markdown_image(label: &str, url: &str) -> String {
    format!("![{label}]({url})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_public_url_without_double_slashes() {
        let target = UploadTarget {
            provider: "s3".to_string(),
            bucket: "bucket".to_string(),
            endpoint: "https://example.com".to_string(),
            region: "auto".to_string(),
            public_base_url: "https://assets.example.com/base/".to_string(),
        };

        assert_eq!(
            public_url(&target, "/blog/cover.webp").unwrap(),
            "https://assets.example.com/base/blog/cover.webp"
        );
    }

    #[test]
    fn treats_public_base_url_without_scheme_as_https_domain() {
        let target = UploadTarget {
            provider: "s3".to_string(),
            bucket: "bucket".to_string(),
            endpoint: "https://example.com".to_string(),
            region: "auto".to_string(),
            public_base_url: "img.eaveluo.com".to_string(),
        };

        assert_eq!(
            public_url(&target, "README.md").unwrap(),
            "https://img.eaveluo.com/README.md"
        );
    }

    #[test]
    fn normalizes_public_base_url_without_trailing_slash() {
        assert_eq!(
            normalize_public_base_url("img.eaveluo.com/").unwrap(),
            "https://img.eaveluo.com"
        );
        assert_eq!(
            normalize_public_base_url("https://img.eaveluo.com/base/").unwrap(),
            "https://img.eaveluo.com/base"
        );
    }

    #[test]
    fn formats_markdown_image_link() {
        assert_eq!(
            markdown_image("cover", "https://assets.example.com/cover.webp"),
            "![cover](https://assets.example.com/cover.webp)"
        );
    }
}
