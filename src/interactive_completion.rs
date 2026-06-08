#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub value: String,
    pub description: &'static str,
    pub append_space: bool,
}

impl Suggestion {
    fn command(value: &'static str, description: &'static str) -> Self {
        Self {
            value: value.to_string(),
            description,
            append_space: true,
        }
    }

    fn option(value: &'static str, description: &'static str) -> Self {
        Self {
            value: value.to_string(),
            description,
            append_space: true,
        }
    }

    fn value(value: String, description: &'static str) -> Self {
        Self {
            value,
            description,
            append_space: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionResult {
    None,
    Insert(String),
    Candidates(Vec<Suggestion>),
}

pub fn hint(line: &str, targets: &[String]) -> Option<String> {
    let suggestions = suggestions_for_line(line, targets);
    if suggestions.is_empty() {
        return None;
    }

    Some(format!(
        "hint: {}",
        suggestions
            .iter()
            .take(5)
            .map(|suggestion| suggestion.value.as_str())
            .collect::<Vec<_>>()
            .join(" | ")
    ))
}

pub fn complete(line: &str, targets: &[String]) -> CompletionResult {
    let suggestions = suggestions_for_line(line, targets);
    match suggestions.as_slice() {
        [] => CompletionResult::None,
        [suggestion] => CompletionResult::Insert(apply_suggestion(line, suggestion)),
        _ => CompletionResult::Candidates(suggestions),
    }
}

pub fn suggestions_for_line(line: &str, targets: &[String]) -> Vec<Suggestion> {
    let context = CompletionContext::new(line);

    if let Some(suggestions) = target_name_suggestions(&context, targets) {
        return suggestions;
    }

    match context.words.as_slice() {
        [] => prefixed(root_commands(), ""),
        [command] if !context.ends_with_space => {
            let root_matches = prefixed(root_commands(), command);
            if root_matches.len() == 1 && root_matches[0].value == *command {
                scoped_suggestions(command, "", targets)
            } else {
                root_matches
            }
        }
        [command] => scoped_suggestions(command, "", targets),
        ["target", subcommand] if !context.ends_with_space => {
            prefixed(target_subcommands(), subcommand)
        }
        ["target", subcommand, ..] => target_scoped_suggestions(subcommand, &context),
        ["log", subcommand] if !context.ends_with_space => prefixed(log_subcommands(), subcommand),
        ["log", subcommand, ..] => log_scoped_suggestions(subcommand, context.current_prefix()),
        ["language", subcommand] if !context.ends_with_space => {
            prefixed(language_subcommands(), subcommand)
        }
        ["language", "use", language] if !context.ends_with_space => {
            prefixed(language_values(), language)
        }
        ["language", "use", ..] => prefixed(language_values(), context.current_prefix()),
        ["language", subcommand, ..] => {
            if *subcommand == "use" {
                prefixed(language_values(), context.current_prefix())
            } else {
                Vec::new()
            }
        }
        ["upload", ..] => upload_scoped_suggestions(&context, targets),
        _ => Vec::new(),
    }
}

fn scoped_suggestions(command: &str, prefix: &str, targets: &[String]) -> Vec<Suggestion> {
    match command {
        "target" => prefixed(target_subcommands(), prefix),
        "log" => prefixed(log_subcommands(), prefix),
        "language" => prefixed(language_subcommands(), prefix),
        "upload" => upload_scoped_suggestions(&CompletionContext::new("upload "), targets),
        _ => Vec::new(),
    }
}

fn target_name_suggestions(
    context: &CompletionContext<'_>,
    targets: &[String],
) -> Option<Vec<Suggestion>> {
    if targets.is_empty() {
        return None;
    }

    let prefix = context.target_name_prefix()?;

    let suggestions = targets
        .iter()
        .filter(|target| target.starts_with(prefix))
        .map(|target| Suggestion::value(target.clone(), "saved target"))
        .collect::<Vec<_>>();

    if !context.ends_with_space && suggestions.len() == 1 && suggestions[0].value.as_str() == prefix
    {
        return Some(Vec::new());
    }

    Some(suggestions)
}

fn target_scoped_suggestions(subcommand: &str, context: &CompletionContext<'_>) -> Vec<Suggestion> {
    match subcommand {
        "add" => unused_options(target_add_options(), context),
        "update" => unused_options(target_update_options(), context),
        "use" | "remove" | "list" => Vec::new(),
        _ => Vec::new(),
    }
}

fn log_scoped_suggestions(subcommand: &str, prefix: &str) -> Vec<Suggestion> {
    match subcommand {
        "export" => prefixed(
            vec![Suggestion::option("--output", "write logs to this file")],
            prefix,
        ),
        "clear" => Vec::new(),
        _ => Vec::new(),
    }
}

fn upload_scoped_suggestions(
    context: &CompletionContext<'_>,
    targets: &[String],
) -> Vec<Suggestion> {
    if context.previous_word_is("--target") {
        return target_value_suggestions(targets, context.current_prefix());
    }

    if let Some(prefix) = context.current_prefix().strip_prefix("--target=") {
        return target_value_suggestions(targets, prefix)
            .into_iter()
            .map(|mut suggestion| {
                suggestion.value = format!("--target={}", suggestion.value);
                suggestion
            })
            .collect();
    }

    unused_options(upload_options(), context)
}

fn unused_options(values: Vec<Suggestion>, context: &CompletionContext<'_>) -> Vec<Suggestion> {
    let used = context.used_options();
    prefixed(values, context.current_prefix())
        .into_iter()
        .filter(|suggestion| !used.iter().any(|option| option == &suggestion.value))
        .collect()
}

fn target_value_suggestions(targets: &[String], prefix: &str) -> Vec<Suggestion> {
    targets
        .iter()
        .filter(|target| target.starts_with(prefix))
        .map(|target| Suggestion::value(target.clone(), "saved target"))
        .collect()
}

fn prefixed(values: Vec<Suggestion>, prefix: &str) -> Vec<Suggestion> {
    values
        .into_iter()
        .filter(|suggestion| suggestion.value.starts_with(prefix))
        .collect()
}

fn apply_suggestion(line: &str, suggestion: &Suggestion) -> String {
    let prefix_start = current_prefix_start(line);
    let mut completed = line[..prefix_start].to_string();
    completed.push_str(&suggestion.value);
    if suggestion.append_space && !completed.ends_with(' ') {
        completed.push(' ');
    }
    completed
}

fn current_prefix_start(line: &str) -> usize {
    if line.ends_with(char::is_whitespace) {
        return line.len();
    }

    line.char_indices()
        .rev()
        .find_map(|(index, ch)| ch.is_whitespace().then_some(index + ch.len_utf8()))
        .unwrap_or(0)
}

fn root_commands() -> Vec<Suggestion> {
    vec![
        Suggestion::command("target", "manage upload targets"),
        Suggestion::command("upload", "upload files"),
        Suggestion::command("log", "manage diagnostic logs"),
        Suggestion::command("language", "manage CLI language"),
        Suggestion::command("exit", "leave interactive mode"),
        Suggestion::command("quit", "leave interactive mode"),
    ]
}

fn target_subcommands() -> Vec<Suggestion> {
    vec![
        Suggestion::command("add", "add a target"),
        Suggestion::command("update", "update a target"),
        Suggestion::command("list", "list targets"),
        Suggestion::command("use", "select the default target"),
        Suggestion::command("remove", "remove a target"),
    ]
}

fn log_subcommands() -> Vec<Suggestion> {
    vec![
        Suggestion::command("export", "export diagnostic logs"),
        Suggestion::command("clear", "clear diagnostic logs"),
    ]
}

fn language_subcommands() -> Vec<Suggestion> {
    vec![
        Suggestion::command("show", "show current language"),
        Suggestion::command("use", "set language"),
    ]
}

fn language_values() -> Vec<Suggestion> {
    vec![
        Suggestion::command("en", "English"),
        Suggestion::command("zh-CN", "Simplified Chinese"),
    ]
}

fn target_add_options() -> Vec<Suggestion> {
    vec![
        Suggestion::option("--provider", "storage provider"),
        Suggestion::option("--bucket", "bucket name"),
        Suggestion::option("--endpoint", "S3-compatible endpoint"),
        Suggestion::option("--region", "storage region"),
        Suggestion::option("--public-base-url", "public file URL base"),
        Suggestion::option("--folder", "object key folder"),
        Suggestion::option("--access-key-id", "access key id"),
        Suggestion::option("--secret-access-key", "secret access key"),
        Suggestion::option("--set-default", "make this target default"),
        Suggestion::option("--skip-check", "skip connectivity check"),
    ]
}

fn target_update_options() -> Vec<Suggestion> {
    target_add_options()
}

fn upload_options() -> Vec<Suggestion> {
    vec![
        Suggestion::option("--target", "target name"),
        Suggestion::option("--folder", "object key folder"),
        Suggestion::option("--name", "object key name"),
        Suggestion::option("--recursive", "upload a directory recursively"),
        Suggestion::option("--markdown", "print markdown image links"),
        Suggestion::option("--dry-run", "plan without uploading"),
    ]
}

#[derive(Debug)]
struct CompletionContext<'a> {
    words: Vec<&'a str>,
    ends_with_space: bool,
}

impl<'a> CompletionContext<'a> {
    fn new(line: &'a str) -> Self {
        Self {
            words: line.split_whitespace().collect(),
            ends_with_space: line.ends_with(char::is_whitespace),
        }
    }

    fn current_prefix(&self) -> &'a str {
        if self.ends_with_space {
            ""
        } else {
            self.words.last().copied().unwrap_or("")
        }
    }

    fn previous_word_is(&self, value: &str) -> bool {
        self.words
            .len()
            .checked_sub(2)
            .and_then(|index| self.words.get(index))
            .is_some_and(|word| *word == value)
    }

    fn used_options(&self) -> Vec<String> {
        let mut options = Vec::new();
        for word in &self.words {
            if !word.starts_with("--") {
                continue;
            }
            let option = word.split_once('=').map_or(*word, |(option, _)| option);
            if !options.iter().any(|existing| existing == option) {
                options.push(option.to_string());
            }
        }
        options
    }

    fn target_name_prefix(&self) -> Option<&str> {
        let ["target", subcommand, rest @ ..] = self.words.as_slice() else {
            return None;
        };

        match *subcommand {
            "use" | "remove" => self.simple_target_name_prefix(rest),
            "update" => self.update_target_name_prefix(rest),
            _ => None,
        }
    }

    fn simple_target_name_prefix(&self, rest: &[&'a str]) -> Option<&'a str> {
        match rest {
            [] => Some(""),
            [name] if !self.ends_with_space => Some(name),
            _ => None,
        }
    }

    fn update_target_name_prefix(&self, rest: &[&'a str]) -> Option<&'a str> {
        if rest.is_empty() {
            return Some("");
        }

        let mut index = 0;
        while index < rest.len() {
            let token = rest[index];
            if !token.starts_with('-') {
                return (index == rest.len() - 1 && !self.ends_with_space).then_some(token);
            }

            if token.split_once('=').is_some() {
                index += 1;
                continue;
            }

            if update_option_takes_value(token) {
                if index + 1 >= rest.len() {
                    return None;
                }
                index += 2;
            } else {
                index += 1;
            }
        }

        Some("")
    }
}

fn update_option_takes_value(option: &str) -> bool {
    matches!(
        option,
        "--provider"
            | "--bucket"
            | "--endpoint"
            | "--region"
            | "--public-base-url"
            | "--access-key-id"
            | "--secret-access-key"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets() -> Vec<String> {
        vec!["cf-wiki-bucket-apac".to_string(), "r2-blog".to_string()]
    }

    #[test]
    fn hints_target_subcommands_after_complete_target_command() {
        assert_eq!(
            hint("target", &targets()).unwrap(),
            "hint: add | update | list | use | remove"
        );
    }

    #[test]
    fn completes_unique_root_command_prefix() {
        assert_eq!(
            complete("tar", &targets()),
            CompletionResult::Insert("target ".to_string())
        );
    }

    #[test]
    fn offers_matching_target_subcommands() {
        assert_eq!(hint("target u", &targets()).unwrap(), "hint: update | use");
    }

    #[test]
    fn completes_target_name_for_update() {
        assert_eq!(
            complete("target update cf", &targets()),
            CompletionResult::Insert("target update cf-wiki-bucket-apac ".to_string())
        );
    }

    #[test]
    fn completes_upload_target_option_value() {
        assert_eq!(
            complete("upload ./a.png --target cf", &targets()),
            CompletionResult::Insert("upload ./a.png --target cf-wiki-bucket-apac ".to_string())
        );
    }

    #[test]
    fn omits_options_that_are_already_present() {
        let suggestions = suggestions_for_line(r#"target add --provider="xxxx" --"#, &targets())
            .into_iter()
            .map(|suggestion| suggestion.value)
            .collect::<Vec<_>>();

        assert!(!suggestions.contains(&"--provider".to_string()));
        assert!(suggestions.contains(&"--bucket".to_string()));
        assert!(suggestions.contains(&"--endpoint".to_string()));
    }

    #[test]
    fn omits_upload_options_that_are_already_present() {
        let suggestions = suggestions_for_line("upload ./a.png --markdown --", &targets())
            .into_iter()
            .map(|suggestion| suggestion.value)
            .collect::<Vec<_>>();

        assert!(!suggestions.contains(&"--markdown".to_string()));
        assert!(suggestions.contains(&"--target".to_string()));
    }
}
