use std::{
    collections::HashSet,
    fs,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use indicatif::ProgressBar;
use tokio::task::JoinSet;

use crate::{
    cli::{OutputFormat, UploadCommand},
    i18n, output, secret, storage,
    target::TargetStore,
};

/// Maximum number of files uploaded in parallel for a directory upload.
const UPLOAD_CONCURRENCY: usize = 8;

pub async fn run(command: UploadCommand) -> Result<()> {
    let dry_run = command.dry_run;
    let markdown = command.markdown;
    let store = TargetStore::load()?;
    let target_name = store.active_target_name(command.target.as_deref())?;
    let target = store
        .targets
        .get(&target_name)
        .with_context(|| format!("target `{target_name}` does not exist"))?;

    let folder = resolve_upload_folder(
        target.folder.as_deref(),
        command.folder.as_deref(),
        command.ignore_target_folder,
    )?;
    let planned = plan_uploads(&command.paths, folder.as_deref(), command.name.as_deref())?;
    let items = dedupe_and_disambiguate(planned);
    if items.is_empty() {
        bail!("no files matched upload input");
    }

    for renamed in items.iter().filter_map(UploadItem::rename_notice) {
        anstream::eprintln!(
            "{}",
            output::warning(&i18n::t_args(
                "upload-renamed-on-conflict",
                &[("local", renamed.local), ("key", renamed.key)],
            ))
        );
    }
    let upload_count = items.len() as u64;
    tracing::info!(
        command = "upload",
        target = %target_name,
        dry_run,
        markdown,
        upload_count,
        result = "planned",
        "upload planned"
    );

    let credentials = if dry_run {
        None
    } else {
        Some(secret::credentials(&target_name).with_context(|| {
            i18n::t_args("upload-missing-credentials", &[("target", &target_name)])
        })?)
    };

    let client = if let Some(credentials) = credentials {
        Some(Arc::new(
            storage::s3::Client::new(target.clone(), credentials).await?,
        ))
    } else {
        None
    };

    let progress = if dry_run {
        None
    } else {
        Some(output::upload_progress(upload_count))
    };

    if let Some(client) = &client
        && let Err(error) = upload_items(
            client.clone(),
            &items,
            progress.as_ref(),
            UPLOAD_CONCURRENCY,
        )
        .await
    {
        if let Some(progress) = &progress {
            progress.finish_and_clear();
        }
        return Err(error);
    }

    if let Some(progress) = &progress {
        progress.finish_and_clear();
    }

    if dry_run {
        anstream::eprintln!(
            "{}",
            output::info(&format!(
                "Dry run: {upload_count} file(s) planned for target `{target_name}` (nothing uploaded)."
            ))
        );
        for item in &items {
            anstream::eprintln!("  {}  ->  {}", item.local_path, item.key);
        }
    }

    match command.output {
        OutputFormat::Text => {
            for item in &items {
                let url = output::public_url(target, &item.key)?;
                if markdown {
                    let label = item
                        .local_path
                        .file_stem()
                        .unwrap_or_else(|| item.local_path.as_str());
                    println!("{}", output::markdown_image(label, &url));
                } else {
                    println!("{url}");
                }
            }
        }
        OutputFormat::Json => {
            let entries = items
                .iter()
                .map(|item| {
                    let url = output::public_url(target, &item.key)?;
                    Ok(serde_json::json!({
                        "local_path": item.local_path.as_str(),
                        "key": item.key,
                        "url": url,
                    }))
                })
                .collect::<Result<Vec<_>>>()?;
            let document = serde_json::json!({
                "target": target_name,
                "dry_run": dry_run,
                "count": items.len(),
                "uploads": entries,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&document)
                    .context("failed to serialize upload result as JSON")?
            );
        }
    }

    if !dry_run {
        anstream::eprintln!(
            "{}",
            output::success(&format!("Uploaded {upload_count} file(s)."))
        );
    }

    tracing::info!(
        command = "upload",
        target = %target_name,
        dry_run,
        markdown,
        upload_count,
        result = "success",
        "upload finished"
    );

    Ok(())
}

async fn upload_items(
    client: Arc<storage::s3::Client>,
    items: &[UploadItem],
    progress: Option<&ProgressBar>,
    concurrency: usize,
) -> Result<()> {
    let mut set: JoinSet<Result<()>> = JoinSet::new();
    let mut pending = items.iter().cloned();

    for _ in 0..concurrency.max(1) {
        let Some(item) = pending.next() else {
            break;
        };
        spawn_upload(&mut set, client.clone(), item, progress.cloned());
    }

    while let Some(joined) = set.join_next().await {
        joined.context("upload task failed to complete")??;
        if let Some(item) = pending.next() {
            spawn_upload(&mut set, client.clone(), item, progress.cloned());
        }
    }

    Ok(())
}

fn spawn_upload(
    set: &mut JoinSet<Result<()>>,
    client: Arc<storage::s3::Client>,
    item: UploadItem,
    progress: Option<ProgressBar>,
) {
    set.spawn(async move {
        if let Some(progress) = &progress {
            progress.set_message(format!("Uploading {}", item.key));
        }
        client.upload_file(&item.local_path, &item.key).await?;
        if let Some(progress) = &progress {
            progress.inc(1);
        }
        Ok(())
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UploadItem {
    local_path: Utf8PathBuf,
    key: String,
    /// True when this item's key was suffixed to avoid colliding with another
    /// local file mapping to the same remote key, so the caller can warn that the
    /// resulting URL differs from the original file name.
    renamed: bool,
}

/// Borrowed view of a renamed item, used to render the transparency notice.
struct RenameNotice<'a> {
    local: &'a str,
    key: &'a str,
}

impl UploadItem {
    fn rename_notice(&self) -> Option<RenameNotice<'_>> {
        self.renamed.then_some(RenameNotice {
            local: self.local_path.as_str(),
            key: &self.key,
        })
    }
}

/// Plans uploads for one or more inputs (files and/or directories), aggregating
/// them into a single, deterministically ordered list.
fn plan_uploads(
    paths: &[Utf8PathBuf],
    prefix: Option<&str>,
    name: Option<&str>,
) -> Result<Vec<UploadItem>> {
    if name.is_some() && !(paths.len() == 1 && paths[0].is_file()) {
        bail!(i18n::t("upload-name-single-file-only"));
    }

    let missing = paths
        .iter()
        .filter(|path| !path.exists())
        .map(Utf8PathBuf::to_string)
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(i18n::t_args(
            "upload-path-not-found",
            &[("paths", &missing.join(", "))],
        ));
    }

    let mut items = Vec::new();
    for path in paths {
        plan_one(path, prefix, name, &mut items)?;
    }
    sort_items(&mut items);
    Ok(items)
}

/// Plans a single input. Files map to one item keyed by their (optional renamed)
/// file name; directories are walked recursively, each rooted at itself so its
/// internal structure is preserved relative to that input.
fn plan_one(
    path: &Utf8Path,
    prefix: Option<&str>,
    name: Option<&str>,
    items: &mut Vec<UploadItem>,
) -> Result<()> {
    if path.is_file() {
        let filename = name
            .or_else(|| path.file_name())
            .context("could not infer file name")?;
        items.push(UploadItem {
            local_path: path.to_path_buf(),
            key: join_key(prefix, filename),
            renamed: false,
        });
        return Ok(());
    }

    if path.is_dir() {
        collect_directory(path, path, prefix, items)?;
        return Ok(());
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
                renamed: false,
            });
        }
    }
    Ok(())
}

fn sort_items(items: &mut [UploadItem]) {
    items.sort_by(|left, right| {
        left.key
            .cmp(&right.key)
            .then_with(|| left.local_path.cmp(&right.local_path))
    });
}

/// Drops exact duplicates (same local file and key) and gives every remaining
/// key collision a unique suffixed key (`name-1.ext`, `name-2.ext`, ...), so two
/// distinct local files never silently overwrite each other on the remote.
fn dedupe_and_disambiguate(items: Vec<UploadItem>) -> Vec<UploadItem> {
    let mut items = items;
    sort_items(&mut items);
    items.dedup();

    let mut used: HashSet<String> = HashSet::new();
    let mut result = Vec::with_capacity(items.len());
    for mut item in items {
        if used.insert(item.key.clone()) {
            result.push(item);
            continue;
        }

        let unique_key = next_available_key(&item.key, &used);
        used.insert(unique_key.clone());
        item.key = unique_key;
        item.renamed = true;
        result.push(item);
    }
    result
}

fn next_available_key(key: &str, used: &HashSet<String>) -> String {
    let (stem, extension) = split_key_extension(key);
    let mut suffix = 1;
    loop {
        let candidate = match extension {
            Some(extension) => format!("{stem}-{suffix}.{extension}"),
            None => format!("{stem}-{suffix}"),
        };
        if !used.contains(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

/// Splits a key into its stem and extension, considering only the final path
/// segment so directory separators are never mistaken for extension dots. A
/// leading dot (dotfile) is treated as part of the stem, not an extension.
fn split_key_extension(key: &str) -> (&str, Option<&str>) {
    let segment_start = key.rfind('/').map(|index| index + 1).unwrap_or(0);
    let segment = &key[segment_start..];
    match segment.rfind('.') {
        Some(dot) if dot > 0 => {
            let split = segment_start + dot;
            (&key[..split], Some(&key[split + 1..]))
        }
        _ => (key, None),
    }
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

fn resolve_upload_folder(
    target_folder: Option<&str>,
    upload_folder: Option<&str>,
    ignore_target_folder: bool,
) -> Result<Option<String>> {
    let now = DateParts::today_utc();
    let mut parts = Vec::new();
    if !ignore_target_folder && let Some(folder) = normalize_folder(target_folder, now)? {
        parts.push(folder);
    }
    if let Some(folder) = normalize_folder(upload_folder, now)? {
        parts.push(folder);
    }

    if parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(parts.join("/")))
    }
}

fn normalize_folder(folder: Option<&str>, today: DateParts) -> Result<Option<String>> {
    let Some(folder) = folder.map(str::trim).filter(|folder| !folder.is_empty()) else {
        return Ok(None);
    };
    let rendered = render_folder_template(folder, today);
    let rendered = rendered.replace('\\', "/");
    let normalized = rendered
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    if normalized.contains(&"..") {
        bail!("folder cannot contain `..` path segments");
    }

    Ok(Some(normalized.join("/")))
}

#[derive(Debug, Clone, Copy)]
struct DateParts {
    year: i32,
    month: u32,
    day: u32,
}

impl DateParts {
    fn today_utc() -> Self {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self::from_unix_days((seconds / 86_400) as i64)
    }

    fn from_unix_days(days: i64) -> Self {
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let mut year = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = mp + if mp < 10 { 3 } else { -9 };
        year += if month <= 2 { 1 } else { 0 };

        Self {
            year: year as i32,
            month: month as u32,
            day: day as u32,
        }
    }
}

fn render_folder_template(template: &str, today: DateParts) -> String {
    template
        .replace("{yyyy}", &format!("{:04}", today.year))
        .replace("{MM}", &format!("{:02}", today.month))
        .replace("{dd}", &format!("{:02}", today.day))
        .replace(
            "{date}",
            &format!("{:04}-{:02}-{:02}", today.year, today.month, today.day),
        )
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

        let items = plan_uploads(std::slice::from_ref(&file_path), Some("blog/post"), None).unwrap();

        assert_eq!(
            items,
            vec![UploadItem {
                local_path: file_path,
                key: "blog/post/cover.webp".to_string(),
                renamed: false,
            }]
        );
    }

    #[test]
    fn combines_target_folder_and_upload_folder() {
        let tempdir = tempfile::tempdir().unwrap();
        let file_path = tempdir.path().join("cover.webp");
        fs::write(&file_path, "image").unwrap();
        let file_path = Utf8PathBuf::from_path_buf(file_path).unwrap();

        let folder = resolve_upload_folder(Some("blog"), Some("posts/2026/06/08"), false).unwrap();
        let items =
            plan_uploads(std::slice::from_ref(&file_path), folder.as_deref(), None).unwrap();

        assert_eq!(items[0].key, "blog/posts/2026/06/08/cover.webp");
    }

    #[test]
    fn ignore_target_folder_skips_target_base_folder() {
        let folder = resolve_upload_folder(Some("blog"), Some("posts/2026"), true).unwrap();

        assert_eq!(folder.as_deref(), Some("posts/2026"));
    }

    #[test]
    fn rejects_parent_directory_segments_in_folder() {
        let error = resolve_upload_folder(Some("blog"), Some("../private"), false)
            .unwrap_err()
            .to_string();

        assert!(error.contains("folder"));
        assert!(error.contains(".."));
    }

    #[test]
    fn renders_basic_date_folder_template() {
        let folder = render_folder_template(
            "posts/{yyyy}/{MM}/{dd}/{date}",
            DateParts {
                year: 2026,
                month: 6,
                day: 8,
            },
        );

        assert_eq!(folder, "posts/2026/06/08/2026-06-08");
    }

    #[test]
    fn plans_single_file_upload_with_custom_name() {
        let tempdir = tempfile::tempdir().unwrap();
        let file_path = tempdir.path().join("cover-original.webp");
        fs::write(&file_path, "image").unwrap();
        let file_path = Utf8PathBuf::from_path_buf(file_path).unwrap();

        let items = plan_uploads(
            std::slice::from_ref(&file_path),
            Some("blog/post"),
            Some("cover.webp"),
        )
        .unwrap();

        assert_eq!(items[0].key, "blog/post/cover.webp");
    }

    #[test]
    fn rejects_custom_name_for_directory_upload() {
        let tempdir = tempfile::tempdir().unwrap();
        let dir_path = Utf8PathBuf::from_path_buf(tempdir.path().to_path_buf()).unwrap();

        let error = plan_uploads(std::slice::from_ref(&dir_path), None, Some("cover.webp"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("--name"));
    }

    #[test]
    fn plans_directory_upload_recursively_by_default() {
        let tempdir = tempfile::tempdir().unwrap();
        let nested = tempdir.path().join("images").join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(tempdir.path().join("images").join("cover.webp"), "image").unwrap();
        fs::write(nested.join("demo.mp4"), "video").unwrap();
        let dir_path = Utf8PathBuf::from_path_buf(tempdir.path().join("images")).unwrap();

        let items = plan_uploads(std::slice::from_ref(&dir_path), Some("blog/post"), None).unwrap();
        let keys = items.into_iter().map(|item| item.key).collect::<Vec<_>>();

        assert_eq!(
            keys,
            vec![
                "blog/post/cover.webp".to_string(),
                "blog/post/nested/demo.mp4".to_string(),
            ]
        );
    }

    fn item(local: &str, key: &str) -> UploadItem {
        UploadItem {
            local_path: Utf8PathBuf::from(local),
            key: key.to_string(),
            renamed: false,
        }
    }

    #[test]
    fn plans_multiple_file_inputs_into_one_list() {
        let tempdir = tempfile::tempdir().unwrap();
        let first = tempdir.path().join("cover.webp");
        let second = tempdir.path().join("banner.png");
        fs::write(&first, "a").unwrap();
        fs::write(&second, "b").unwrap();
        let paths = vec![
            Utf8PathBuf::from_path_buf(first).unwrap(),
            Utf8PathBuf::from_path_buf(second).unwrap(),
        ];

        let keys = plan_uploads(&paths, Some("blog"), None)
            .unwrap()
            .into_iter()
            .map(|item| item.key)
            .collect::<Vec<_>>();

        assert_eq!(
            keys,
            vec!["blog/banner.png".to_string(), "blog/cover.webp".to_string()]
        );
    }

    #[test]
    fn plans_mixed_file_and_directory_inputs() {
        let tempdir = tempfile::tempdir().unwrap();
        let logo = tempdir.path().join("logo.svg");
        fs::write(&logo, "svg").unwrap();
        let icons = tempdir.path().join("icons");
        fs::create_dir_all(&icons).unwrap();
        fs::write(icons.join("a.png"), "a").unwrap();
        let paths = vec![
            Utf8PathBuf::from_path_buf(logo).unwrap(),
            Utf8PathBuf::from_path_buf(icons).unwrap(),
        ];

        let keys = plan_uploads(&paths, None, None)
            .unwrap()
            .into_iter()
            .map(|item| item.key)
            .collect::<Vec<_>>();

        assert_eq!(keys, vec!["a.png".to_string(), "logo.svg".to_string()]);
    }

    #[test]
    fn reports_all_missing_paths_at_once() {
        let tempdir = tempfile::tempdir().unwrap();
        let present = tempdir.path().join("here.png");
        fs::write(&present, "x").unwrap();
        let paths = vec![
            Utf8PathBuf::from_path_buf(present).unwrap(),
            Utf8PathBuf::from(format!("{}/missing-a.png", tempdir.path().display())),
            Utf8PathBuf::from(format!("{}/missing-b.png", tempdir.path().display())),
        ];

        let error = plan_uploads(&paths, None, None).unwrap_err().to_string();

        assert!(error.contains("missing-a.png"));
        assert!(error.contains("missing-b.png"));
    }

    #[test]
    fn rejects_custom_name_with_multiple_inputs() {
        let tempdir = tempfile::tempdir().unwrap();
        let first = tempdir.path().join("a.png");
        let second = tempdir.path().join("b.png");
        fs::write(&first, "a").unwrap();
        fs::write(&second, "b").unwrap();
        let paths = vec![
            Utf8PathBuf::from_path_buf(first).unwrap(),
            Utf8PathBuf::from_path_buf(second).unwrap(),
        ];

        let error = plan_uploads(&paths, None, Some("renamed.png"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("--name"));
    }

    #[test]
    fn dedupe_drops_exact_duplicates() {
        let items = vec![item("a/cover.png", "cover.png"), item("a/cover.png", "cover.png")];

        let result = dedupe_and_disambiguate(items);

        assert_eq!(result.len(), 1);
        assert!(!result[0].renamed);
    }

    #[test]
    fn disambiguate_suffixes_colliding_keys() {
        let items = vec![
            item("a/logo.png", "logo.png"),
            item("b/logo.png", "logo.png"),
            item("c/logo.png", "logo.png"),
        ];

        let result = dedupe_and_disambiguate(items);
        let keys = result.iter().map(|item| item.key.clone()).collect::<Vec<_>>();

        assert_eq!(
            keys,
            vec![
                "logo.png".to_string(),
                "logo-1.png".to_string(),
                "logo-2.png".to_string(),
            ]
        );
        assert!(!result[0].renamed);
        assert!(result[1].renamed);
        assert!(result[2].renamed);
    }

    #[test]
    fn disambiguate_skips_keys_already_taken() {
        let items = vec![
            item("a/logo.png", "logo.png"),
            item("b/logo-1.png", "logo-1.png"),
            item("c/logo.png", "logo.png"),
        ];

        let keys = dedupe_and_disambiguate(items)
            .into_iter()
            .map(|item| item.key)
            .collect::<Vec<_>>();

        assert!(keys.contains(&"logo.png".to_string()));
        assert!(keys.contains(&"logo-1.png".to_string()));
        assert!(keys.contains(&"logo-2.png".to_string()));
    }

    #[test]
    fn splits_key_extension_for_suffixing() {
        assert_eq!(split_key_extension("blog/logo.png"), ("blog/logo", Some("png")));
        assert_eq!(split_key_extension("archive.tar.gz"), ("archive.tar", Some("gz")));
        assert_eq!(split_key_extension("README"), ("README", None));
        assert_eq!(split_key_extension("dir/.hidden"), ("dir/.hidden", None));
    }

    #[test]
    fn suffixes_extensionless_key_at_the_end() {
        let items = vec![item("a/README", "README"), item("b/README", "README")];

        let keys = dedupe_and_disambiguate(items)
            .into_iter()
            .map(|item| item.key)
            .collect::<Vec<_>>();

        assert_eq!(keys, vec!["README".to_string(), "README-1".to_string()]);
    }
}
