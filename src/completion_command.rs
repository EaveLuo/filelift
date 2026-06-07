use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_complete::{Shell, generate};

use crate::{
    cli::{Cli, CompletionsCommand},
    i18n,
    target::TargetStore,
};

pub fn run(command: CompletionsCommand) -> Result<()> {
    let mut cli_command = i18n::localize_command(Cli::command());
    let bin_name = cli_command.get_name().to_string();
    let mut completion = Vec::new();
    generate(command.shell, &mut cli_command, bin_name, &mut completion);

    if command.shell == Shell::PowerShell {
        let mut script = String::from_utf8(completion)
            .context("generated PowerShell completions were not valid UTF-8")?;
        add_powershell_dynamic_target_completion(&mut script);
        print!("{script}");
    } else {
        std::io::copy(&mut completion.as_slice(), &mut std::io::stdout())
            .context("failed to write shell completions")?;
    }

    Ok(())
}

pub fn complete_targets() -> Result<()> {
    for name in target_names()? {
        println!("{name}");
    }

    Ok(())
}

fn target_names() -> Result<Vec<String>> {
    let store = TargetStore::load()?;
    let mut names = store.targets.keys().cloned().collect::<Vec<_>>();
    names.extend(
        store
            .draft_targets
            .keys()
            .filter(|name| !store.targets.contains_key(*name))
            .cloned(),
    );
    names.sort();
    Ok(names)
}

fn add_powershell_dynamic_target_completion(script: &mut String) {
    let target_candidates = r#"            filelift __complete targets |
                Where-Object { $_ -like "$wordToComplete*" } |
                ForEach-Object {
                    [CompletionResult]::new($_, $_, [CompletionResultType]::ParameterValue, $_)
                }
"#;
    for command in ["update", "use", "remove"] {
        let needle = format!("        'filelift;target;{command}' {{\n");
        let replacement = format!("{needle}{target_candidates}");
        *script = script.replace(&needle, &replacement);
    }

    let upload_needle = "        'filelift;upload' {\n";
    let upload_replacement = format!(
        "{upload_needle}            if ($commandElements.Count -ge 3 -and $commandElements[($commandElements.Count - 2)].Extent.Text -eq '--target') {{\n{target_candidates}                break\n            }}\n"
    );
    *script = script.replace(upload_needle, &upload_replacement);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_hook_calls_target_completion_command() {
        let mut script =
            "        'filelift;target;update' {\n        'filelift;upload' {\n".to_string();
        add_powershell_dynamic_target_completion(&mut script);

        assert!(script.contains("filelift __complete targets"));
        assert!(script.contains("target"));
        assert!(script.contains("--target"));
    }
}
