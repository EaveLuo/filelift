use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn root_help_lists_target_and_upload_commands() {
    let mut command = Command::cargo_bin("filelift").unwrap();

    command
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("target  Manage upload targets"))
        .stdout(predicate::str::contains(
            "upload  Upload a file or directory",
        ));
}

#[test]
fn target_help_lists_target_management_commands() {
    let mut command = Command::cargo_bin("filelift").unwrap();

    command
        .args(["target", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "add     Add or update an upload target",
        ))
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
fn dry_run_upload_without_target_returns_actionable_error() {
    let tempdir = tempfile::tempdir().unwrap();
    let file_path = tempdir.path().join("cover.webp");
    std::fs::write(&file_path, "image").unwrap();

    let mut command = Command::cargo_bin("filelift").unwrap();

    command
        .args(["upload", file_path.to_str().unwrap(), "--dry-run"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("filelift target use"))
        .stderr(predicate::str::contains("--target"));
}
