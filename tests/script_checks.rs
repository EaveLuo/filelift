use std::fs;

#[test]
fn unix_installer_uses_user_bin_and_release_assets() {
    let script = fs::read_to_string("scripts/install.sh").expect("install.sh should exist");

    assert!(script.contains("REPO=\"EaveLuo/filelift\""));
    assert!(script.contains("${FILELIFT_INSTALL_DIR:-$HOME/.local/bin}"));
    assert!(script.contains("x86_64-apple-darwin"));
    assert!(script.contains("aarch64-apple-darwin"));
    assert!(script.contains("x86_64-unknown-linux-gnu"));
    assert!(script.contains("$HOME/.profile"));
    assert!(script.contains("$HOME/.zshrc"));
    assert!(script.contains("Installing or updating filelift"));
    assert!(script.contains("cp -f"));
    assert!(script.contains("\"$INSTALL_DIR/$BINARY_NAME\" --version"));
    assert!(script.contains("another filelift is earlier on your PATH"));
    assert!(script.contains("cargo install filelift --force"));
}

#[test]
fn windows_installer_uses_user_path_and_release_assets() {
    let script = fs::read_to_string("scripts/install.ps1").expect("install.ps1 should exist");

    assert!(script.contains("$Repo = \"EaveLuo/filelift\""));
    assert!(script.contains("$env:LOCALAPPDATA"));
    assert!(script.contains("Programs\\filelift\\bin"));
    assert!(script.contains("$env:FILELIFT_VERSION"));
    assert!(script.contains("Installing or updating filelift"));
    assert!(script.contains("x86_64-pc-windows-msvc"));
    assert!(script.contains("SetEnvironmentVariable(\"Path\""));
    assert!(script.contains("User)"));
    assert!(script.contains("& $InstalledPath --version"));
    assert!(script.contains("Another filelift is earlier on your PATH"));
    assert!(script.contains("cargo install filelift --force"));
}

#[test]
fn release_workflow_uploads_installer_assets() {
    let workflow =
        fs::read_to_string(".github/workflows/release.yml").expect("release workflow should exist");

    assert!(workflow.contains("filelift-x86_64-pc-windows-msvc.zip"));
    assert!(workflow.contains("filelift-x86_64-unknown-linux-gnu.tar.gz"));
    assert!(workflow.contains("filelift-x86_64-apple-darwin.tar.gz"));
    assert!(workflow.contains("filelift-aarch64-apple-darwin.tar.gz"));
    assert!(workflow.contains("softprops/action-gh-release"));
}

#[test]
fn release_workflow_publishes_crate() {
    let workflow =
        fs::read_to_string(".github/workflows/release.yml").expect("release workflow should exist");

    assert!(workflow.contains("cargo-publish"));
    assert!(workflow.contains("CARGO_REGISTRY_TOKEN"));
    assert!(workflow.contains("cargo publish --locked"));
    assert!(workflow.contains("Verify release tag matches Cargo version"));
    assert!(workflow.contains("expected=\"v${version}\""));
}
