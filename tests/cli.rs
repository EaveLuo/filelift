use assert_cmd::Command;
use predicates::prelude::*;

fn with_home_dir(command: &mut Command, home_dir: &std::path::Path) {
    command.env("FILELIFT_HOME", home_dir);
}

/// Pins the CLI to English so help assertions do not depend on the developer's
/// global `~/.filelift/language.toml`. `FILELIFT_LANG` takes priority over the
/// saved language config.
fn with_english(command: &mut Command) {
    command.env("FILELIFT_LANG", "en");
}

/// A fixed 64-hex master key so the encrypted secret store is deterministic in
/// tests and independent of the host machine identifier.
const TEST_MASTER_KEY: &str = "0f0e0d0c0b0a09080706050403020100ffeeddccbbaa99887766554433221100";

fn with_master_key(command: &mut Command) {
    command.env("FILELIFT_MASTER_KEY_HEX", TEST_MASTER_KEY);
}

fn add_target_with_credentials(
    home_dir: &std::path::Path,
    name: &str,
    access_key_id: &str,
    secret_access_key: &str,
) {
    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, home_dir);
    with_english(&mut command);
    with_master_key(&mut command);
    command
        .args([
            "target",
            "add",
            name,
            "--bucket",
            "eave-assets",
            "--endpoint",
            "https://example.r2.cloudflarestorage.com",
            "--public-base-url",
            "https://assets.example.com",
            "--access-key-id",
            access_key_id,
            "--secret-access-key",
            secret_access_key,
            "--skip-check",
        ])
        .assert()
        .success();
}

fn write_draft_target(home_dir: &std::path::Path, name: &str) {
    let filelift_dir = home_dir.join(".filelift");
    std::fs::create_dir_all(&filelift_dir).unwrap();
    std::fs::write(
        filelift_dir.join("targets.toml"),
        format!(
            r#"
[draft_targets."{name}"]
provider = "s3"
bucket = "eave-assets"
endpoint = "https://example.r2.cloudflarestorage.com"
region = "auto"
public_base_url = "https://assets.example.com"
"#
        ),
    )
    .unwrap();
}

fn write_target_store_with_target_and_draft(home_dir: &std::path::Path) {
    let filelift_dir = home_dir.join(".filelift");
    std::fs::create_dir_all(&filelift_dir).unwrap();
    std::fs::write(
        filelift_dir.join("targets.toml"),
        r#"
default_target = "r2-blog"

[targets.r2-blog]
provider = "s3"
bucket = "eave-assets"
endpoint = "https://example.r2.cloudflarestorage.com"
region = "auto"
public_base_url = "https://assets.example.com"

[draft_targets.draft-cdn]
provider = "s3"
bucket = "draft-assets"
endpoint = "https://example.r2.cloudflarestorage.com"
region = "auto"
public_base_url = "https://draft.example.com"
"#,
    )
    .unwrap();
}

#[test]
fn root_help_lists_target_and_upload_commands() {
    let mut command = Command::cargo_bin("filelift").unwrap();
    with_english(&mut command);

    command
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("target"))
        .stdout(predicate::str::contains("Manage upload targets"))
        .stdout(predicate::str::contains("upload"))
        .stdout(predicate::str::contains("Upload a file or directory"))
        .stdout(predicate::str::contains("language"))
        .stdout(predicate::str::contains("Manage CLI language"));
}

#[test]
fn no_args_in_non_interactive_context_returns_actionable_error() {
    let mut command = Command::cargo_bin("filelift").unwrap();

    command
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "interactive mode requires a terminal",
        ))
        .stderr(predicate::str::contains("filelift target list"));
}

#[test]
fn missing_target_name_in_non_interactive_context_returns_actionable_error() {
    let config_dir = tempfile::tempdir().unwrap();
    write_target_store_with_target_and_draft(config_dir.path());

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args(["target", "use"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("target name required"))
        .stderr(predicate::str::contains("filelift"));
}

#[test]
fn root_help_uses_saved_chinese_language() {
    let config_dir = tempfile::tempdir().unwrap();

    let mut language_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut language_command, config_dir.path());
    language_command
        .args(["language", "use", "zh"])
        .assert()
        .success();

    let mut help_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut help_command, config_dir.path());
    help_command
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("管理上传 target"))
        .stdout(predicate::str::contains("上传文件或目录"))
        .stdout(predicate::str::contains("管理 CLI 语言"));
}

#[test]
fn target_help_lists_target_management_commands() {
    let mut command = Command::cargo_bin("filelift").unwrap();
    with_english(&mut command);

    command
        .args(["target", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("add     Add an upload target"))
        .stdout(predicate::str::contains("update  Update an upload target"))
        .stdout(predicate::str::contains(
            "list    List configured upload targets",
        ))
        .stdout(predicate::str::contains(
            "use     Set the default upload target",
        ))
        .stdout(predicate::str::contains("remove  Remove an upload target"));
}

#[test]
fn upload_help_exposes_target_and_batch_options() {
    let mut command = Command::cargo_bin("filelift").unwrap();

    command
        .args(["upload", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--target <TARGET>"))
        .stdout(predicate::str::contains("--folder <FOLDER>"))
        .stdout(predicate::str::contains("--ignore-target-folder"))
        .stdout(predicate::str::contains("--markdown"))
        .stdout(predicate::str::contains("--dry-run"))
        .stdout(predicate::str::contains("--recursive").not());
}

#[test]
fn log_help_lists_export_and_clear_commands() {
    let mut command = Command::cargo_bin("filelift").unwrap();
    with_english(&mut command);

    command
        .args(["log", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "export  Export decrypted diagnostic logs",
        ))
        .stdout(predicate::str::contains(
            "clear   Clear encrypted diagnostic logs",
        ));
}

#[test]
fn language_use_switches_prompts_to_chinese() {
    let config_dir = tempfile::tempdir().unwrap();

    let mut language_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut language_command, config_dir.path());
    language_command
        .args(["language", "use", "zh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("zh"));

    let mut add_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut add_command, config_dir.path());
    add_command
        .args([
            "target",
            "add",
            "r2-blog",
            "--access-key-id",
            "test-access-key",
            "--secret-access-key",
            "test-secret-key",
            "--skip-check",
        ])
        .write_stdin(
            "eave-assets\n\
             https://example.r2.cloudflarestorage.com\n\
             auto\n\
             https://assets.example.com\n",
        )
        .assert()
        .success()
        .stdout(predicate::str::contains("存储桶:"))
        .stdout(predicate::str::contains("公开访问基础 URL:"));
}

#[test]
fn dry_run_upload_without_target_returns_actionable_error() {
    let tempdir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let file_path = tempdir.path().join("cover.webp");
    std::fs::write(&file_path, "image").unwrap();

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args(["upload", file_path.to_str().unwrap(), "--dry-run"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("filelift target use"))
        .stderr(predicate::str::contains("--target"));
}

#[test]
fn upload_without_credentials_points_to_target_update() {
    let tempdir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let file_path = tempdir.path().join("cover.webp");
    std::fs::write(&file_path, "image").unwrap();

    let filelift_dir = config_dir.path().join(".filelift");
    std::fs::create_dir_all(&filelift_dir).unwrap();
    std::fs::write(
        filelift_dir.join("targets.toml"),
        r#"
default_target = "missing-creds-target"

[targets.missing-creds-target]
provider = "s3"
bucket = "eave-assets"
endpoint = "https://example.r2.cloudflarestorage.com"
region = "auto"
public_base_url = "https://assets.example.com"
"#,
    )
    .unwrap();

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args(["upload", file_path.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "filelift target update missing-creds-target",
        ));
}

#[test]
fn target_add_prompts_for_missing_metadata() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args(["target", "add", "r2-blog"])
        .args([
            "--access-key-id",
            "test-access-key",
            "--secret-access-key",
            "test-secret-key",
            "--skip-check",
        ])
        .write_stdin(
            "eave-assets\n\
             https://example.r2.cloudflarestorage.com\n\
             auto\n\
             https://assets.example.com\n",
        )
        .assert()
        .success()
        .stdout(predicate::str::contains("Bucket:"))
        .stdout(predicate::str::contains("Endpoint:"))
        .stdout(predicate::str::contains("Public base URL:"))
        .stdout(predicate::str::contains("Added target `r2-blog`."));

    let target_store = config_dir.path().join(".filelift").join("targets.toml");
    let content = std::fs::read_to_string(target_store).unwrap();
    assert!(content.contains("default_target = \"r2-blog\""));
    assert!(content.contains("[targets.r2-blog]"));
    assert!(content.contains("bucket = \"eave-assets\""));
    assert!(content.contains("endpoint = \"https://example.r2.cloudflarestorage.com\""));
    assert!(content.contains("region = \"auto\""));
    assert!(content.contains("public_base_url = \"https://assets.example.com\""));
}

#[test]
fn target_add_fails_on_eof_when_required_prompt_is_missing() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args(["target", "add", "r2-blog"])
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicate::str::contains("prompt input ended before"));
}

#[test]
fn target_add_accepts_all_metadata_as_options() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args([
            "target",
            "add",
            "r2-blog",
            "--bucket",
            "eave-assets",
            "--endpoint",
            "https://example.r2.cloudflarestorage.com",
            "--public-base-url",
            "https://assets.example.com",
            "--folder",
            "blog/images",
            "--access-key-id",
            "test-access-key",
            "--secret-access-key",
            "test-secret-key",
            "--skip-check",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added target `r2-blog`."));

    let target_store = config_dir.path().join(".filelift").join("targets.toml");
    let content = std::fs::read_to_string(target_store).unwrap();
    assert!(content.contains("bucket = \"eave-assets\""));
    assert!(content.contains("endpoint = \"https://example.r2.cloudflarestorage.com\""));
    assert!(content.contains("region = \"auto\""));
    assert!(content.contains("public_base_url = \"https://assets.example.com\""));
    assert!(content.contains("folder = \"blog/images\""));
}

#[test]
fn dry_run_upload_combines_target_folder_and_upload_folder() {
    let tempdir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let file_path = tempdir.path().join("cover.webp");
    std::fs::write(&file_path, "image").unwrap();

    let filelift_dir = config_dir.path().join(".filelift");
    std::fs::create_dir_all(&filelift_dir).unwrap();
    std::fs::write(
        filelift_dir.join("targets.toml"),
        r#"
default_target = "r2-blog"

[targets.r2-blog]
provider = "s3"
bucket = "eave-assets"
endpoint = "https://example.r2.cloudflarestorage.com"
region = "auto"
public_base_url = "https://assets.example.com"
folder = "blog"
"#,
    )
    .unwrap();

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args([
            "upload",
            file_path.to_str().unwrap(),
            "--folder",
            "posts/2026/06/08",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "https://assets.example.com/blog/posts/2026/06/08/cover.webp",
        ));
}

#[test]
fn dry_run_previews_local_path_to_key_mapping_on_stderr() {
    let tempdir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let file_path = tempdir.path().join("cover.webp");
    std::fs::write(&file_path, "image").unwrap();

    let filelift_dir = config_dir.path().join(".filelift");
    std::fs::create_dir_all(&filelift_dir).unwrap();
    std::fs::write(
        filelift_dir.join("targets.toml"),
        r#"
default_target = "r2-blog"

[targets.r2-blog]
provider = "s3"
bucket = "eave-assets"
endpoint = "https://example.r2.cloudflarestorage.com"
region = "auto"
public_base_url = "https://assets.example.com"
folder = "blog"
"#,
    )
    .unwrap();

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args(["upload", file_path.to_str().unwrap(), "--dry-run"])
        .assert()
        .success()
        // stdout stays a clean, pipeable URL list.
        .stdout(predicate::str::contains(
            "https://assets.example.com/blog/cover.webp",
        ))
        // The human-facing preview (path -> key) goes to stderr.
        .stderr(predicate::str::contains("Dry run:"))
        .stderr(predicate::str::contains("->"))
        .stderr(predicate::str::contains("blog/cover.webp"));
}

#[test]
fn dry_run_upload_directory_defaults_to_recursive() {
    let tempdir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let images_dir = tempdir.path().join("images");
    let nested_dir = images_dir.join("nested");
    std::fs::create_dir_all(&nested_dir).unwrap();
    std::fs::write(images_dir.join("cover.webp"), "image").unwrap();
    std::fs::write(nested_dir.join("demo.mp4"), "video").unwrap();

    let filelift_dir = config_dir.path().join(".filelift");
    std::fs::create_dir_all(&filelift_dir).unwrap();
    std::fs::write(
        filelift_dir.join("targets.toml"),
        r#"
default_target = "r2-blog"

[targets.r2-blog]
provider = "s3"
bucket = "eave-assets"
endpoint = "https://example.r2.cloudflarestorage.com"
region = "auto"
public_base_url = "https://assets.example.com"
folder = "blog"
"#,
    )
    .unwrap();

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args(["upload", images_dir.to_str().unwrap(), "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "https://assets.example.com/blog/cover.webp",
        ))
        .stdout(predicate::str::contains(
            "https://assets.example.com/blog/nested/demo.mp4",
        ));
}

#[test]
fn dry_run_upload_ignore_target_folder_skips_base_folder() {
    let tempdir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let file_path = tempdir.path().join("cover.webp");
    std::fs::write(&file_path, "image").unwrap();

    let filelift_dir = config_dir.path().join(".filelift");
    std::fs::create_dir_all(&filelift_dir).unwrap();
    std::fs::write(
        filelift_dir.join("targets.toml"),
        r#"
default_target = "r2-blog"

[targets.r2-blog]
provider = "s3"
bucket = "eave-assets"
endpoint = "https://example.r2.cloudflarestorage.com"
region = "auto"
public_base_url = "https://assets.example.com"
folder = "blog"
"#,
    )
    .unwrap();

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args([
            "upload",
            file_path.to_str().unwrap(),
            "--folder",
            "standalone",
            "--ignore-target-folder",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "https://assets.example.com/standalone/cover.webp",
        ))
        .stdout(predicate::str::contains("/blog/").not());
}

#[test]
fn target_add_normalizes_public_base_url_without_scheme() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args([
            "target",
            "add",
            "r2-blog",
            "--bucket",
            "eave-assets",
            "--endpoint",
            "https://example.r2.cloudflarestorage.com",
            "--public-base-url",
            "img.eaveluo.com",
            "--access-key-id",
            "test-access-key",
            "--secret-access-key",
            "test-secret-key",
            "--skip-check",
        ])
        .assert()
        .success();

    let target_store = config_dir.path().join(".filelift").join("targets.toml");
    let content = std::fs::read_to_string(target_store).unwrap();
    assert!(content.contains("public_base_url = \"https://img.eaveluo.com\""));
}

#[test]
fn target_add_prompts_for_missing_access_key_id() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args([
            "target",
            "add",
            "r2-blog",
            "--bucket",
            "eave-assets",
            "--endpoint",
            "https://example.r2.cloudflarestorage.com",
            "--public-base-url",
            "https://assets.example.com",
            "--secret-access-key",
            "test-secret-key",
            "--skip-check",
        ])
        .write_stdin("test-access-key\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Access key ID:"));
}

#[test]
fn target_add_refuses_to_replace_existing_target() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut add_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut add_command, config_dir.path());
    add_command
        .args([
            "target",
            "add",
            "r2-blog",
            "--bucket",
            "eave-assets",
            "--endpoint",
            "https://example.r2.cloudflarestorage.com",
            "--public-base-url",
            "https://assets.example.com",
            "--access-key-id",
            "test-access-key",
            "--secret-access-key",
            "test-secret-key",
            "--skip-check",
        ])
        .assert()
        .success();

    let mut second_add = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut second_add, config_dir.path());
    second_add
        .args([
            "target",
            "add",
            "r2-blog",
            "--bucket",
            "other-assets",
            "--endpoint",
            "https://example.r2.cloudflarestorage.com",
            "--public-base-url",
            "https://assets.example.com",
            "--access-key-id",
            "test-access-key",
            "--secret-access-key",
            "test-secret-key",
            "--skip-check",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn target_update_changes_selected_metadata() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut add_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut add_command, config_dir.path());
    add_command
        .args([
            "target",
            "add",
            "r2-blog",
            "--bucket",
            "eave-assets",
            "--endpoint",
            "https://example.r2.cloudflarestorage.com",
            "--public-base-url",
            "https://assets.example.com",
            "--access-key-id",
            "test-access-key",
            "--secret-access-key",
            "test-secret-key",
            "--skip-check",
        ])
        .assert()
        .success();

    let mut update_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut update_command, config_dir.path());
    update_command
        .args([
            "target",
            "update",
            "r2-blog",
            "--bucket",
            "updated-assets",
            "--public-base-url",
            "img.eaveluo.com",
            "--skip-check",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated target `r2-blog`."));

    let target_store = config_dir.path().join(".filelift").join("targets.toml");
    let content = std::fs::read_to_string(target_store).unwrap();
    assert!(content.contains("bucket = \"updated-assets\""));
    assert!(content.contains("endpoint = \"https://example.r2.cloudflarestorage.com\""));
    assert!(content.contains("public_base_url = \"https://img.eaveluo.com\""));
}

#[test]
fn target_update_prompts_for_metadata_when_no_fields_are_given() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut add_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut add_command, config_dir.path());
    add_command
        .args([
            "target",
            "add",
            "r2-blog",
            "--bucket",
            "eave-assets",
            "--endpoint",
            "https://example.r2.cloudflarestorage.com",
            "--public-base-url",
            "https://assets.example.com",
            "--access-key-id",
            "test-access-key",
            "--secret-access-key",
            "test-secret-key",
            "--skip-check",
        ])
        .assert()
        .success();

    let mut update_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut update_command, config_dir.path());
    update_command
        .args(["target", "update", "r2-blog", "--skip-check"])
        .write_stdin(
            "\n\
             updated-assets\n\
             \n\
             \n\
             img.eaveluo.com\n\
             \n",
        )
        .assert()
        .success()
        .stdout(predicate::str::contains("Provider [s3]:"))
        .stdout(predicate::str::contains("Bucket [eave-assets]:"))
        .stdout(predicate::str::contains(
            "Endpoint [https://example.r2.cloudflarestorage.com]:",
        ))
        .stdout(predicate::str::contains("Region [auto]:"))
        .stdout(predicate::str::contains(
            "Public base URL [https://assets.example.com]:",
        ))
        .stdout(predicate::str::contains("Base folder"))
        .stdout(predicate::str::contains("Updated target `r2-blog`."));

    let target_store = config_dir.path().join(".filelift").join("targets.toml");
    let content = std::fs::read_to_string(target_store).unwrap();
    assert!(content.contains("provider = \"s3\""));
    assert!(content.contains("bucket = \"updated-assets\""));
    assert!(content.contains("endpoint = \"https://example.r2.cloudflarestorage.com\""));
    assert!(content.contains("region = \"auto\""));
    assert!(content.contains("public_base_url = \"https://img.eaveluo.com\""));
}

#[test]
fn target_update_refuses_missing_target() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args([
            "target",
            "update",
            "missing",
            "--bucket",
            "updated-assets",
            "--skip-check",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn target_update_can_resume_failed_connectivity_draft() {
    let config_dir = tempfile::tempdir().unwrap();
    write_draft_target(config_dir.path(), "r2-blog");

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args([
            "target",
            "update",
            "r2-blog",
            "--region",
            "APAC",
            "--public-base-url",
            "img.eaveluo.com",
            "--access-key-id",
            "test-access-key",
            "--secret-access-key",
            "test-secret-key",
            "--skip-check",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Resuming draft target `r2-blog`."))
        .stdout(predicate::str::contains("Added target `r2-blog`."));

    let content =
        std::fs::read_to_string(config_dir.path().join(".filelift").join("targets.toml")).unwrap();
    assert!(content.contains("[targets.r2-blog]"));
    assert!(!content.contains("[draft_targets."));
    assert!(content.contains("region = \"APAC\""));
    assert!(content.contains("public_base_url = \"https://img.eaveluo.com\""));
}

#[test]
fn target_add_does_not_save_formal_target_when_connectivity_check_fails() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args([
            "target",
            "add",
            "r2-blog",
            "--bucket",
            "eave-assets",
            "--endpoint",
            "http://127.0.0.1:1",
            "--public-base-url",
            "https://assets.example.com",
            "--access-key-id",
            "test",
            "--secret-access-key",
            "test",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("target connectivity check failed"));

    let target_store = config_dir.path().join(".filelift").join("targets.toml");
    let content = std::fs::read_to_string(target_store).unwrap();
    assert!(!content.contains("[targets.r2-blog]"));
}

#[test]
fn target_add_saves_failed_connectivity_input_as_draft() {
    let config_dir = tempfile::tempdir().unwrap();
    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args([
            "target",
            "add",
            "r2-blog",
            "--bucket",
            "eave-assets",
            "--endpoint",
            "http://127.0.0.1:1",
            "--region",
            "auto",
            "--public-base-url",
            "https://assets.example.com",
            "--access-key-id",
            "test-access-key",
            "--secret-access-key",
            "test-secret-key",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("target connectivity check failed"));

    let target_store = config_dir.path().join(".filelift").join("targets.toml");
    let content = std::fs::read_to_string(target_store).unwrap();
    assert!(!content.contains("[targets.r2-blog]"));
    assert!(content.contains("[draft_targets.r2-blog]"));
    assert!(content.contains("bucket = \"eave-assets\""));
    assert!(content.contains("endpoint = \"http://127.0.0.1:1\""));
    assert!(content.contains("region = \"auto\""));
    assert!(content.contains("public_base_url = \"https://assets.example.com\""));
}

#[test]
fn target_add_resumes_failed_connectivity_draft_defaults() {
    let config_dir = tempfile::tempdir().unwrap();
    write_draft_target(config_dir.path(), "r2-blog");

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args([
            "target",
            "add",
            "r2-blog",
            "--access-key-id",
            "test-access-key",
            "--secret-access-key",
            "test-secret-key",
            "--skip-check",
        ])
        .write_stdin("\n\nAPAC\nhttps://img.eaveluo.com\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Resuming draft target `r2-blog`."))
        .stdout(predicate::str::contains("Bucket [eave-assets]:"))
        .stdout(predicate::str::contains(
            "Endpoint [https://example.r2.cloudflarestorage.com]:",
        ))
        .stdout(predicate::str::contains("Region [auto]:"))
        .stdout(predicate::str::contains(
            "Public base URL [https://assets.example.com]:",
        ))
        .stdout(predicate::str::contains("Added target `r2-blog`."));

    let content =
        std::fs::read_to_string(config_dir.path().join(".filelift").join("targets.toml")).unwrap();
    assert!(content.contains("[targets.r2-blog]"));
    assert!(!content.contains("[draft_targets.r2-blog]"));
    assert!(content.contains("region = \"APAC\""));
    assert!(content.contains("public_base_url = \"https://img.eaveluo.com\""));
}

#[test]
fn target_remove_deletes_draft_target() {
    let config_dir = tempfile::tempdir().unwrap();
    write_draft_target(config_dir.path(), "eavetest1");

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args(["target", "remove", "eavetest1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed target `eavetest1`."));

    let content =
        std::fs::read_to_string(config_dir.path().join(".filelift").join("targets.toml")).unwrap();
    assert!(!content.contains("eavetest1"));
}

#[test]
fn target_add_writes_encrypted_log_that_can_be_exported() {
    let home_dir = tempfile::tempdir().unwrap();
    let output_path = home_dir.path().join("debug-log.jsonl");

    let mut add_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut add_command, home_dir.path());
    add_command
        .env(
            "FILELIFT_LOG_KEY_HEX",
            "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        )
        .args([
            "target",
            "add",
            "r2-blog",
            "--bucket",
            "eave-assets",
            "--endpoint",
            "https://example.r2.cloudflarestorage.com",
            "--public-base-url",
            "https://assets.example.com",
            "--access-key-id",
            "test-access-key",
            "--secret-access-key",
            "test-secret-key",
            "--skip-check",
        ])
        .assert()
        .success();

    let encrypted_log = home_dir
        .path()
        .join(".filelift")
        .join("logs")
        .join("events.log.enc");
    let encrypted_content = std::fs::read_to_string(&encrypted_log).unwrap();
    assert!(!encrypted_content.contains("target add"));
    assert!(!encrypted_content.contains("r2-blog"));
    assert!(!encrypted_content.contains("eave-assets"));

    let mut export_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut export_command, home_dir.path());
    export_command
        .env(
            "FILELIFT_LOG_KEY_HEX",
            "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        )
        .args(["log", "export", "--output", output_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Exported diagnostic log"));

    let exported = std::fs::read_to_string(output_path).unwrap();
    assert!(exported.contains("\"command\":\"target add\""));
    assert!(exported.contains("\"target\":\"r2-blog\""));
    assert!(exported.contains("\"result\":\"success\""));
}

#[test]
fn log_clear_removes_encrypted_log_file() {
    let home_dir = tempfile::tempdir().unwrap();

    let mut add_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut add_command, home_dir.path());
    add_command
        .env(
            "FILELIFT_LOG_KEY_HEX",
            "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        )
        .args([
            "target",
            "add",
            "r2-blog",
            "--bucket",
            "eave-assets",
            "--endpoint",
            "https://example.r2.cloudflarestorage.com",
            "--public-base-url",
            "https://assets.example.com",
            "--access-key-id",
            "test-access-key",
            "--secret-access-key",
            "test-secret-key",
            "--skip-check",
        ])
        .assert()
        .success();

    let encrypted_log = home_dir
        .path()
        .join(".filelift")
        .join("logs")
        .join("events.log.enc");
    assert!(encrypted_log.exists());

    let mut clear_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut clear_command, home_dir.path());
    clear_command
        .args(["log", "clear"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cleared diagnostic logs."));

    assert!(!encrypted_log.exists());
}

#[test]
fn credentials_are_stored_in_encrypted_file_not_plaintext() {
    let home_dir = tempfile::tempdir().unwrap();
    add_target_with_credentials(
        home_dir.path(),
        "r2-blog",
        "plain-access-id",
        "plain-secret-key",
    );

    let secrets = home_dir.path().join(".filelift").join("secrets.enc");
    assert!(secrets.exists(), "secret store file should be created");

    let bytes = std::fs::read(&secrets).unwrap();
    let haystack = String::from_utf8_lossy(&bytes);
    assert!(!haystack.contains("plain-access-id"));
    assert!(!haystack.contains("plain-secret-key"));

    // No plaintext credentials file should be written.
    assert!(
        !home_dir
            .path()
            .join(".filelift")
            .join("credentials.toml")
            .exists()
    );
}

#[test]
fn credentials_export_emits_per_target_env_vars() {
    let home_dir = tempfile::tempdir().unwrap();
    add_target_with_credentials(
        home_dir.path(),
        "r2-blog",
        "test-access-key",
        "test-secret-key",
    );

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, home_dir.path());
    with_english(&mut command);
    with_master_key(&mut command);
    command
        .args(["credentials", "export", "r2-blog"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "FILELIFT_R2_BLOG_ACCESS_KEY_ID=test-access-key",
        ))
        .stdout(predicate::str::contains(
            "FILELIFT_R2_BLOG_SECRET_ACCESS_KEY=test-secret-key",
        ))
        // The plaintext warning must go to stderr, keeping stdout machine-clean.
        .stdout(predicate::str::contains("export ").not())
        .stderr(predicate::str::contains("plaintext"));
}

#[test]
fn credentials_export_shell_format_prefixes_export() {
    let home_dir = tempfile::tempdir().unwrap();
    add_target_with_credentials(
        home_dir.path(),
        "r2-blog",
        "test-access-key",
        "test-secret-key",
    );

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, home_dir.path());
    with_english(&mut command);
    with_master_key(&mut command);
    command
        .args(["credentials", "export", "r2-blog", "--format", "shell"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "export FILELIFT_R2_BLOG_ACCESS_KEY_ID=test-access-key",
        ))
        .stdout(predicate::str::contains(
            "export FILELIFT_R2_BLOG_SECRET_ACCESS_KEY=test-secret-key",
        ));
}

#[test]
fn credentials_export_with_wrong_master_key_fails_to_decrypt() {
    let home_dir = tempfile::tempdir().unwrap();
    add_target_with_credentials(
        home_dir.path(),
        "r2-blog",
        "test-access-key",
        "test-secret-key",
    );

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, home_dir.path());
    with_english(&mut command);
    command
        .env(
            "FILELIFT_MASTER_KEY_HEX",
            "00000000000000000000000000000000000000000000000000000000deadbeef",
        )
        .args(["credentials", "export", "r2-blog"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("decrypt"));
}

#[test]
fn credentials_export_missing_target_is_actionable() {
    let home_dir = tempfile::tempdir().unwrap();

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, home_dir.path());
    with_english(&mut command);
    with_master_key(&mut command);
    command
        .args(["credentials", "export", "ghost"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No stored credentials"))
        .stderr(predicate::str::contains("FILELIFT_"));
}

#[test]
fn target_add_reads_secret_access_key_from_stdin() {
    let home_dir = tempfile::tempdir().unwrap();

    let mut add_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut add_command, home_dir.path());
    with_english(&mut add_command);
    with_master_key(&mut add_command);
    add_command
        .args([
            "target",
            "add",
            "r2-blog",
            "--bucket",
            "eave-assets",
            "--endpoint",
            "https://example.r2.cloudflarestorage.com",
            "--public-base-url",
            "https://assets.example.com",
            "--access-key-id",
            "stdin-access-key",
            "--secret-access-key-stdin",
            "--skip-check",
        ])
        .write_stdin("stdin-secret-key\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Added target `r2-blog`."));

    let mut export_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut export_command, home_dir.path());
    with_english(&mut export_command);
    with_master_key(&mut export_command);
    export_command
        .args(["credentials", "export", "r2-blog"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "FILELIFT_R2_BLOG_SECRET_ACCESS_KEY=stdin-secret-key",
        ));
}

#[test]
fn target_add_non_interactive_reports_missing_fields() {
    let home_dir = tempfile::tempdir().unwrap();

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, home_dir.path());
    with_english(&mut command);
    with_master_key(&mut command);
    command
        .args([
            "target",
            "add",
            "r2-blog",
            "--non-interactive",
            "--skip-check",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Non-interactive mode requires"))
        .stderr(predicate::str::contains("--bucket"))
        .stderr(predicate::str::contains("--secret-access-key"));
}

#[test]
fn target_remove_clears_stored_credentials() {
    let home_dir = tempfile::tempdir().unwrap();
    add_target_with_credentials(
        home_dir.path(),
        "r2-blog",
        "test-access-key",
        "test-secret-key",
    );

    let mut remove_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut remove_command, home_dir.path());
    with_english(&mut remove_command);
    with_master_key(&mut remove_command);
    remove_command
        .args(["target", "remove", "r2-blog"])
        .assert()
        .success();

    let mut export_command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut export_command, home_dir.path());
    with_english(&mut export_command);
    with_master_key(&mut export_command);
    export_command
        .args(["credentials", "export", "r2-blog"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No stored credentials"));
}

fn seed_version_cache(home_dir: &std::path::Path, latest_version: &str) {
    let filelift_dir = home_dir.join(".filelift");
    std::fs::create_dir_all(&filelift_dir).unwrap();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    std::fs::write(
        filelift_dir.join("version_check.json"),
        format!(r#"{{"last_checked_ms":{now_ms},"latest_version":"{latest_version}"}}"#),
    )
    .unwrap();
}

#[test]
fn upgrade_command_is_listed_with_update_alias() {
    let mut command = Command::cargo_bin("filelift").unwrap();
    with_english(&mut command);
    command
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("upgrade"))
        .stdout(predicate::str::contains(
            "Update filelift to the latest release",
        ));

    // The `update` alias resolves to the same command.
    let mut alias_help = Command::cargo_bin("filelift").unwrap();
    with_english(&mut alias_help);
    alias_help
        .args(["update", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--version"));
}

#[test]
fn update_notice_is_shown_when_a_newer_version_is_cached() {
    let config_dir = tempfile::tempdir().unwrap();
    seed_version_cache(config_dir.path(), "999.0.0");

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());
    with_english(&mut command);
    command
        .args(["target", "list"])
        .assert()
        .success()
        .stderr(predicate::str::contains("999.0.0"))
        .stderr(predicate::str::contains("filelift upgrade"));
}

#[test]
fn update_notice_is_suppressed_by_opt_out_env() {
    let config_dir = tempfile::tempdir().unwrap();
    seed_version_cache(config_dir.path(), "999.0.0");

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());
    with_english(&mut command);
    command
        .env("FILELIFT_NO_UPDATE_CHECK", "1")
        .args(["target", "list"])
        .assert()
        .success()
        .stderr(predicate::str::contains("999.0.0").not());
}

#[test]
fn update_notice_is_absent_when_cached_version_is_not_newer() {
    let config_dir = tempfile::tempdir().unwrap();
    seed_version_cache(config_dir.path(), "0.0.1");

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());
    with_english(&mut command);
    command
        .args(["target", "list"])
        .assert()
        .success()
        .stderr(predicate::str::contains("0.0.1").not());
}

#[test]
fn upload_dry_run_json_output_lists_uploads() {
    let tempdir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let file_path = tempdir.path().join("cover.webp");
    std::fs::write(&file_path, "image").unwrap();

    let filelift_dir = config_dir.path().join(".filelift");
    std::fs::create_dir_all(&filelift_dir).unwrap();
    std::fs::write(
        filelift_dir.join("targets.toml"),
        r#"
default_target = "r2-blog"

[targets.r2-blog]
provider = "s3"
bucket = "eave-assets"
endpoint = "https://example.r2.cloudflarestorage.com"
region = "auto"
public_base_url = "https://assets.example.com"
folder = "blog"
"#,
    )
    .unwrap();

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());
    with_english(&mut command);
    command
        .args([
            "upload",
            file_path.to_str().unwrap(),
            "--dry-run",
            "--output",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"uploads\""))
        .stdout(predicate::str::contains("\"count\": 1"))
        .stdout(predicate::str::contains("\"dry_run\": true"))
        .stdout(predicate::str::contains(
            "https://assets.example.com/blog/cover.webp",
        ));
}
