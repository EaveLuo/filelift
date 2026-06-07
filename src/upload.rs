use std::fs;

use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};

use crate::{cli::UploadCommand, i18n, output, secret, storage, target::TargetStore};

pub async fn run(command: UploadCommand) -> Result<()> {
    let recursive = command.recursive;
    let dry_run = command.dry_run;
    let markdown = command.markdown;
    let store = TargetStore::load()?;
    let target_name = store.active_target_name(command.target.as_deref())?;
    let target = store
        .targets
        .get(&target_name)
        .with_context(|| format!("target `{target_name}` does not exist"))?;

    let items = plan_uploads(
        &command.path,
        command.prefix.as_deref(),
        command.name.as_deref(),
        command.recursive,
    )?;
    if items.is_empty() {
        bail!("no files matched upload input");
    }
    let upload_count = items.len() as u64;
    tracing::info!(
        command = "upload",
        target = %target_name,
        recursive,
        dry_run,
        markdown,
        upload_count,
        result = "planned",
        "upload planned"
    );

    let credentials = if command.dry_run {
        None
    } else {
        Some(secret::credentials(&target_name).with_context(|| {
            i18n::t_args("upload-missing-credentials", &[("target", &target_name)])
        })?)
    };

    let client = if let Some(credentials) = credentials {
        Some(storage::s3::Client::new(target.clone(), credentials).await?)
    } else {
        None
    };

    let progress = if command.dry_run {
        None
    } else {
        Some(output::upload_progress(upload_count))
    };

    for item in items {
        let url = output::public_url(target, &item.key)?;

        if let Some(client) = &client {
            if let Some(progress) = &progress {
                progress.set_message(format!("Uploading {}", item.key));
            }
            if let Err(error) = client.upload_file(&item.local_path, &item.key).await {
                if let Some(progress) = &progress {
                    progress.finish_and_clear();
                }
                return Err(error);
            }
            if let Some(progress) = &progress {
                progress.inc(1);
            }
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

    if let Some(progress) = &progress {
        progress.finish_and_clear();
        anstream::eprintln!(
            "{}",
            output::success(&format!("Uploaded {upload_count} file(s)."))
        );
    }

    tracing::info!(
        command = "upload",
        target = %target_name,
        recursive,
        dry_run,
        markdown,
        upload_count,
        result = "success",
        "upload finished"
    );

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
    use std::fs;

    #[test]
    fn joins_key_with_clean_prefix() {
        assert_eq!(
            join_key(Some("/blog/post/"), "cover.webp"),
            "blog/post/cover.webp"
        );
        assert_eq!(join_key(None, "/cover.webp"), "cover.webp");
    }

    #[test]
    fn plans_single_file_upload_with_prefix() {
        let tempdir = tempfile::tempdir().unwrap();
        let file_path = tempdir.path().join("cover.webp");
        fs::write(&file_path, "image").unwrap();
        let file_path = Utf8PathBuf::from_path_buf(file_path).unwrap();

        let items = plan_uploads(&file_path, Some("blog/post"), None, false).unwrap();

        assert_eq!(
            items,
            vec![UploadItem {
                local_path: file_path,
                key: "blog/post/cover.webp".to_string(),
            }]
        );
    }

    #[test]
    fn plans_single_file_upload_with_custom_name() {
        let tempdir = tempfile::tempdir().unwrap();
        let file_path = tempdir.path().join("cover-original.webp");
        fs::write(&file_path, "image").unwrap();
        let file_path = Utf8PathBuf::from_path_buf(file_path).unwrap();

        let items = plan_uploads(&file_path, Some("blog/post"), Some("cover.webp"), false).unwrap();

        assert_eq!(items[0].key, "blog/post/cover.webp");
    }

    #[test]
    fn refuses_directory_without_recursive_flag() {
        let tempdir = tempfile::tempdir().unwrap();
        let dir_path = Utf8PathBuf::from_path_buf(tempdir.path().to_path_buf()).unwrap();

        let error = plan_uploads(&dir_path, None, None, false)
            .unwrap_err()
            .to_string();

        assert!(error.contains("--recursive"));
    }

    #[test]
    fn plans_recursive_directory_upload_with_relative_keys() {
        let tempdir = tempfile::tempdir().unwrap();
        let nested = tempdir.path().join("images").join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(tempdir.path().join("images").join("cover.webp"), "image").unwrap();
        fs::write(nested.join("demo.mp4"), "video").unwrap();
        let dir_path = Utf8PathBuf::from_path_buf(tempdir.path().join("images")).unwrap();

        let items = plan_uploads(&dir_path, Some("blog/post"), None, true).unwrap();
        let keys = items.into_iter().map(|item| item.key).collect::<Vec<_>>();

        assert_eq!(
            keys,
            vec![
                "blog/post/cover.webp".to_string(),
                "blog/post/nested/demo.mp4".to_string(),
            ]
        );
    }
}
