use assert_cmd::Command;
use predicates::prelude::*;

fn with_home_dir(command: &mut Command, home_dir: &std::path::Path) {
    command.env("FILELIFT_HOME", home_dir);
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
fn root_help_lists_completion_command() {
    let mut command = Command::cargo_bin("filelift").unwrap();

    command
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("completions"))
        .stdout(predicate::str::contains("Generate shell completions"));
}

#[test]
fn hidden_target_completion_outputs_saved_targets_and_drafts() {
    let config_dir = tempfile::tempdir().unwrap();
    write_target_store_with_target_and_draft(config_dir.path());

    let mut command = Command::cargo_bin("filelift").unwrap();
    with_home_dir(&mut command, config_dir.path());

    command
        .args(["__complete", "targets"])
        .assert()
        .success()
        .stdout(predicate::str::contains("r2-blog\n"))
        .stdout(predicate::str::contains("draft-cdn\n"));
}

#[test]
fn powershell_completions_include_dynamic_target_lookup() {
    let mut command = Command::cargo_bin("filelift").unwrap();

    command
        .args(["completions", "powershell"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Register-ArgumentCompleter"))
        .stdout(predicate::str::contains("filelift __complete targets"));
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
        .stdout(predicate::str::contains("--prefix <PREFIX>"))
        .stdout(predicate::str::contains("--recursive"))
        .stdout(predicate::str::contains("--markdown"))
        .stdout(predicate::str::contains("--dry-run"));
}

#[test]
fn log_help_lists_export_and_clear_commands() {
    let mut command = Command::cargo_bin("filelift").unwrap();

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
             img.eaveluo.com\n",
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
