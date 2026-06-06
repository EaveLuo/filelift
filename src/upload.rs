use std::fs;

use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};

use crate::{cli::UploadCommand, config::Config, output, secret, storage};

pub async fn run(command: UploadCommand) -> Result<()> {
    let config = Config::load()?;
    let profile_name = config.active_profile_name(command.profile.as_deref())?;
    let profile = config
        .profiles
        .get(&profile_name)
        .with_context(|| format!("profile `{profile_name}` does not exist"))?;

    let items = plan_uploads(
        &command.path,
        command.prefix.as_deref(),
        command.name.as_deref(),
        command.recursive,
    )?;
    if items.is_empty() {
        bail!("no files matched upload input");
    }

    let credentials = if command.dry_run {
        None
    } else {
        Some(secret::credentials(&profile_name)?)
    };

    let client = if let Some(credentials) = credentials {
        Some(storage::s3::Client::new(profile.clone(), credentials).await?)
    } else {
        None
    };

    for item in items {
        let url = output::public_url(profile, &item.key)?;

        if let Some(client) = &client {
            client.upload_file(&item.local_path, &item.key).await?;
        }

        if command.markdown {
            let label = item
                .local_path
                .file_stem()
                .unwrap_or_else(|| item.local_path.as_str());
            println!("{}", output::markdown_image(label, &url));
        } else {
            println!("{url}");
        }
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UploadItem {
    local_path: Utf8PathBuf,
    key: String,
}

fn plan_uploads(
    path: &Utf8Path,
    prefix: Option<&str>,
    name: Option<&str>,
    recursive: bool,
) -> Result<Vec<UploadItem>> {
    if path.is_file() {
        let filename = name
            .or_else(|| path.file_name())
            .context("could not infer file name")?;
        return Ok(vec![UploadItem {
            local_path: path.to_path_buf(),
            key: join_key(prefix, filename),
        }]);
    }

    if path.is_dir() {
        if name.is_some() {
            bail!("--name can only be used with a single file");
        }
        if !recursive {
            bail!("refusing to upload a directory without --recursive");
        }

        let mut items = Vec::new();
        collect_directory(path, path, prefix, &mut items)?;
        items.sort_by(|left, right| left.key.cmp(&right.key));
        return Ok(items);
    }

    bail!("path does not exist: {path}")
}

fn collect_directory(
    root: &Utf8Path,
    current: &Utf8Path,
    prefix: Option<&str>,
    items: &mut Vec<UploadItem>,
) -> Result<()> {
    for entry in fs::read_dir(current).with_context(|| format!("failed to read {current}"))? {
        let entry = entry?;
        let path = Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| {
            anyhow::anyhow!("path contains non-UTF-8 characters: {}", path.display())
        })?;

        if path.is_dir() {
            collect_directory(root, &path, prefix, items)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .with_context(|| format!("failed to make {path} relative to {root}"))?;
            let key = join_key(prefix, &relative.as_str().replace('\\', "/"));
            items.push(UploadItem {
                local_path: path,
                key,
            });
        }
    }
    Ok(())
}

fn join_key(prefix: Option<&str>, name: &str) -> String {
    match prefix.map(str::trim).filter(|prefix| !prefix.is_empty()) {
        Some(prefix) => format!(
            "{}/{}",
            prefix.trim_matches('/'),
            name.trim_start_matches('/')
        ),
        None => name.trim_start_matches('/').to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_key_with_clean_prefix() {
        assert_eq!(
            join_key(Some("/blog/post/"), "cover.webp"),
            "blog/post/cover.webp"
        );
        assert_eq!(join_key(None, "/cover.webp"), "cover.webp");
    }
}
