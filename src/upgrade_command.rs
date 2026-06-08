//! `filelift upgrade` (alias `update`) — update the installed CLI in place.
//!
//! Rather than reimplementing platform detection, asset naming, download, and
//! PATH handling, this delegates to the official install scripts so there is a
//! single source of truth for how filelift is installed and updated.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use crate::{cli::UpgradeCommand, diagnostic_log, i18n, output};

const INSTALL_SH_URL: &str =
    "https://raw.githubusercontent.com/EaveLuo/filelift/main/scripts/install.sh";
const INSTALL_PS1_URL: &str =
    "https://raw.githubusercontent.com/EaveLuo/filelift/main/scripts/install.ps1";
const VERSION_ENV: &str = "FILELIFT_VERSION";

/// A resolved plan for invoking the platform installer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradePlan {
    pub program: String,
    pub args: Vec<String>,
    pub version_env: Option<String>,
}

pub fn run(command: UpgradeCommand) -> Result<()> {
    anstream::eprintln!("{}", output::info(&i18n::t("upgrade-starting")));

    // Upgrade through the same channel the running binary came from, so we never
    // install a second copy that the existing one would shadow on PATH.
    if running_from_cargo_bin() {
        return run_cargo_upgrade(std::env::consts::OS, command.version);
    }

    let plan = build_plan(std::env::consts::OS, command.version)?;

    let mut process = Command::new(&plan.program);
    process.args(&plan.args);
    if let Some(version) = &plan.version_env {
        process.env(VERSION_ENV, version);
    }

    let status = process
        .status()
        .with_context(|| i18n::t_args("upgrade-spawn-failed", &[("program", &plan.program)]))?;

    anyhow::ensure!(status.success(), i18n::t("upgrade-failed"));

    diagnostic_log::record_command_result("upgrade", None, "success");
    anstream::eprintln!("{}", output::success(&i18n::t("upgrade-succeeded")));
    Ok(())
}

/// Reports whether the running executable lives in Cargo's bin directory, which
/// indicates it was installed with `cargo install`.
fn running_from_cargo_bin() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    cargo_bin_dir()
        .map(|dir| is_within(&exe, &dir))
        .unwrap_or(false)
}

/// Resolves `$CARGO_HOME/bin`, falling back to `~/.cargo/bin`.
fn cargo_bin_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("CARGO_HOME") {
        return Some(PathBuf::from(home).join("bin"));
    }
    dirs::home_dir().map(|home| home.join(".cargo").join("bin"))
}

fn is_within(path: &Path, dir: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    path.starts_with(&dir)
}

/// Upgrades a cargo-managed install via `cargo install`.
///
/// On Windows the running `.exe` is locked, so cargo cannot replace the very
/// binary that launched the upgrade; we print the exact command instead of
/// failing midway. On other platforms the running binary can be replaced in
/// place, so we run cargo directly for a true one-step upgrade.
fn run_cargo_upgrade(target_os: &str, version: Option<String>) -> Result<()> {
    let version = version.map(|version| version.trim().trim_start_matches('v').to_string());

    if target_os == "windows" {
        let mut command = "cargo install filelift --force".to_string();
        if let Some(version) = &version {
            command.push_str(&format!(" --version {version}"));
        }
        anstream::eprintln!(
            "{}",
            output::warning(&i18n::t_args(
                "upgrade-cargo-manual",
                &[("command", &command)]
            ))
        );
        return Ok(());
    }

    anstream::eprintln!("{}", output::info(&i18n::t("upgrade-via-cargo")));

    let mut process = Command::new("cargo");
    process.arg("install").arg("filelift").arg("--force");
    if let Some(version) = &version {
        process.arg("--version").arg(version);
    }

    let status = process
        .status()
        .with_context(|| i18n::t_args("upgrade-spawn-failed", &[("program", "cargo")]))?;

    anyhow::ensure!(status.success(), i18n::t("upgrade-failed"));

    diagnostic_log::record_command_result("upgrade", None, "success");
    anstream::eprintln!("{}", output::success(&i18n::t("upgrade-succeeded")));
    Ok(())
}

/// Builds the installer invocation for the current platform. Kept pure so the
/// command construction is unit testable without running any installer.
pub fn build_plan(target_os: &str, version: Option<String>) -> Result<UpgradePlan> {
    let version_env = version.map(|version| version.trim().to_string());

    match target_os {
        "windows" => Ok(UpgradePlan {
            program: "powershell".to_string(),
            args: vec![
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-Command".to_string(),
                format!("irm {INSTALL_PS1_URL} | iex"),
            ],
            version_env,
        }),
        "macos" | "linux" => Ok(UpgradePlan {
            program: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                format!("curl -fsSL {INSTALL_SH_URL} | sh"),
            ],
            version_env,
        }),
        other => {
            anyhow::bail!(i18n::t_args("upgrade-unsupported-os", &[("os", other)]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_plan_uses_powershell_and_install_script() {
        let plan = build_plan("windows", None).unwrap();
        assert_eq!(plan.program, "powershell");
        assert!(plan.args.iter().any(|arg| arg.contains("install.ps1")));
        assert!(plan.args.iter().any(|arg| arg.contains("iex")));
        assert!(plan.version_env.is_none());
    }

    #[test]
    fn unix_plan_uses_shell_and_install_script() {
        let plan = build_plan("linux", None).unwrap();
        assert_eq!(plan.program, "sh");
        assert!(plan.args.iter().any(|arg| arg.contains("install.sh")));
        assert_eq!(build_plan("macos", None).unwrap().program, "sh");
    }

    #[test]
    fn version_is_passed_through_as_env() {
        let plan = build_plan("linux", Some(" v0.3.0 ".to_string())).unwrap();
        assert_eq!(plan.version_env.as_deref(), Some("v0.3.0"));
    }

    #[test]
    fn unsupported_os_is_rejected() {
        assert!(build_plan("freebsd", None).is_err());
    }

    #[test]
    fn cargo_upgrade_on_windows_only_prints_guidance() {
        // On Windows the running exe is locked, so this must not attempt to run
        // cargo; it returns Ok after printing the manual command.
        assert!(run_cargo_upgrade("windows", Some("v1.2.3".to_string())).is_ok());
        assert!(run_cargo_upgrade("windows", None).is_ok());
    }
}
