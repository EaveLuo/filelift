//! `filelift upgrade` (alias `update`) — update the installed CLI in place.
//!
//! Rather than reimplementing platform detection, asset naming, download, and
//! PATH handling, this delegates to the official install scripts so there is a
//! single source of truth for how filelift is installed and updated.

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
    let plan = build_plan(std::env::consts::OS, command.version)?;

    anstream::eprintln!("{}", output::info(&i18n::t("upgrade-starting")));

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
}
