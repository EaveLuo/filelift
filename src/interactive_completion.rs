use crate::i18n;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub value: String,
    /// Localized helper text shown after the candidate in the interactive list.
    pub description: String,
    pub append_space: bool,
    /// Whether this suggestion is a draft (incomplete) target. Drafts are shown
    /// with an explicit marker so users do not mistake them for usable targets.
    pub draft: bool,
}

impl Suggestion {
    /// Builds a suggestion whose description is loaded from the given i18n key.
    fn localized(value: impl Into<String>, description_key: &str) -> Self {
        Self {
            value: value.into(),
            description: i18n::t(description_key),
            append_space: true,
            draft: false,
        }
    }

    fn command(value: &'static str, description_key: &str) -> Self {
        Self::localized(value, description_key)
    }

    fn option(value: &'static str, description_key: &str) -> Self {
        Self::localized(value, description_key)
    }

    fn value(value: String, description_key: &str) -> Self {
        Self::localized(value, description_key)
    }

    fn draft_target(value: String) -> Self {
        Self {
            value,
            description: i18n::t("completion-draft-target"),
            append_space: true,
            draft: true,
        }
    }

    /// Label shown in the inline idle hint, which renders only this string (not
    /// the description), so drafts must carry their marker here.
    fn hint_label(&self) -> String {
        if self.draft {
            format!("{} {}", self.value, i18n::t("completion-draft-marker"))
        } else {
            self.value.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionResult {
    None,
    Insert(String),
    Candidates(Vec<Suggestion>),
}

/// Target names available to interactive completion, split by lifecycle so that
/// suggestions can match what each subcommand can actually operate on.
///
/// `active` targets are fully created and usable; `drafts` are targets whose
/// connectivity check failed and were saved for later resumption. `target use`
/// and `upload --target` must never surface drafts, while `add`/`update`
/// (resume) and `remove` (delete) include them with an explicit marker.
#[derive(Debug, Clone, Copy)]
pub struct TargetCatalog<'a> {
    pub active: &'a [String],
    pub drafts: &'a [String],
}

/// Which lifecycle of target names a `target` subcommand may complete to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetNameScope {
    /// Fully-created targets only (`use`).
    Active,
    /// Resumable drafts only (`add`, since active names would already exist).
    Drafts,
    /// Both active targets and drafts (`update` resumes, `remove` deletes).
    ActiveAndDrafts,
}

impl TargetNameScope {
    fn includes_active(self) -> bool {
        matches!(self, Self::Active | Self::ActiveAndDrafts)
    }

    fn includes_drafts(self) -> bool {
        matches!(self, Self::Drafts | Self::ActiveAndDrafts)
    }
}

pub fn hint(line: &str, catalog: TargetCatalog<'_>) -> Option<String> {
    let suggestions = suggestions_for_line(line, catalog);
    if suggestions.is_empty() {
        return None;
    }

    // Don't show a single hint that merely repeats the word already fully typed
    // (e.g. `target use` hinting `use`); it adds noise without new information.
    let current_word = if line.ends_with(char::is_whitespace) {
        ""
    } else {
        line.split_whitespace().last().unwrap_or("")
    };
    if suggestions.len() == 1 && suggestions[0].value == current_word {
        return None;
    }

    Some(format!(
        "hint: {}",
        suggestions
            .iter()
            .take(5)
            .map(Suggestion::hint_label)
            .collect::<Vec<_>>()
            .join(" | ")
    ))
}

pub fn complete(line: &str, catalog: TargetCatalog<'_>) -> CompletionResult {
    let suggestions = suggestions_for_line(line, catalog);
    match suggestions.as_slice() {
        [] => CompletionResult::None,
        [suggestion] => CompletionResult::Insert(apply_suggestion(line, suggestion)),
        _ => CompletionResult::Candidates(suggestions),
    }
}

pub fn suggestions_for_line(line: &str, catalog: TargetCatalog<'_>) -> Vec<Suggestion> {
    let context = CompletionContext::new(line);

    if let Some(suggestions) = target_name_suggestions(&context, catalog) {
        return suggestions;
    }

    match context.words.as_slice() {
        [] => prefixed(root_commands(), ""),
        [command] if !context.ends_with_space => {
            let root_matches = prefixed(root_commands(), command);
            if root_matches.len() == 1 && root_matches[0].value == *command {
                scoped_suggestions(command, "", catalog)
            } else {
                root_matches
            }
        }
        [command] => scoped_suggestions(command, "", catalog),
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
        ["upload", ..] => upload_scoped_suggestions(&context, catalog),
        _ => Vec::new(),
    }
}

fn scoped_suggestions(command: &str, prefix: &str, catalog: TargetCatalog<'_>) -> Vec<Suggestion> {
    match command {
        "target" => prefixed(target_subcommands(), prefix),
        "log" => prefixed(log_subcommands(), prefix),
        "language" => prefixed(language_subcommands(), prefix),
        "upload" => upload_scoped_suggestions(&CompletionContext::new("upload "), catalog),
        _ => Vec::new(),
    }
}

fn target_name_suggestions(
    context: &CompletionContext<'_>,
    catalog: TargetCatalog<'_>,
) -> Option<Vec<Suggestion>> {
    let (scope, prefix) = context.target_name_completion()?;

    let mut suggestions = Vec::new();
    if scope.includes_active() {
        suggestions.extend(
            catalog
                .active
                .iter()
                .filter(|target| target.starts_with(prefix))
                .map(|target| Suggestion::value(target.clone(), "completion-saved-target")),
        );
    }
    if scope.includes_drafts() {
        suggestions.extend(
            catalog
                .drafts
                .iter()
                .filter(|target| target.starts_with(prefix))
                .map(|target| Suggestion::draft_target(target.clone())),
        );
    }

    // Fall through to other suggestions (e.g. options) when nothing matches the
    // typed name, instead of swallowing the line with an empty candidate list.
    if suggestions.is_empty() {
        return None;
    }

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
            vec![Suggestion::option("--output", "completion-opt-output")],
            prefix,
        ),
        "clear" => Vec::new(),
        _ => Vec::new(),
    }
}

fn upload_scoped_suggestions(
    context: &CompletionContext<'_>,
    catalog: TargetCatalog<'_>,
) -> Vec<Suggestion> {
    if context.previous_word_is("--target") {
        return target_value_suggestions(catalog.active, context.current_prefix());
    }

    if let Some(prefix) = context.current_prefix().strip_prefix("--target=") {
        return target_value_suggestions(catalog.active, prefix)
            .into_iter()
            .map(|mut suggestion| {
                suggestion.value = format!("--target={}", suggestion.value);
                suggestion
            })
            .collect();
    }

    if let Some(prefix) = context.upload_path_prefix() {
        return path_suggestions(prefix);
    }

    unused_options(upload_options(), context)
}

fn path_suggestions(prefix: &str) -> Vec<Suggestion> {
    let (dir, file_prefix) = split_path_prefix(prefix);
    let read_dir_path = if dir.is_empty() { "." } else { dir };
    let Ok(entries) = std::fs::read_dir(read_dir_path) else {
        return Vec::new();
    };

    let listed = entries.flatten().filter_map(|entry| {
        let name = entry.file_name().into_string().ok()?;
        let is_dir = entry
            .file_type()
            .map(|file_type| file_type.is_dir())
            .unwrap_or(false);
        Some((name, is_dir))
    });

    let mut suggestions = build_path_suggestions(dir, file_prefix, listed);
    suggestions.sort_by(|left, right| left.value.cmp(&right.value));
    suggestions
}

/// Splits a path prefix into its directory portion (kept verbatim, including the
/// trailing separator) and the partial file name being typed.
fn split_path_prefix(prefix: &str) -> (&str, &str) {
    match prefix.rfind(['/', '\\']) {
        Some(index) => prefix.split_at(index + 1),
        None => ("", prefix),
    }
}

fn build_path_suggestions(
    dir: &str,
    file_prefix: &str,
    entries: impl Iterator<Item = (String, bool)>,
) -> Vec<Suggestion> {
    let mut suggestions = Vec::new();
    for (name, is_dir) in entries {
        if !name.starts_with(file_prefix) {
            continue;
        }
        // Hide dotfiles unless the user explicitly started typing a leading dot.
        if name.starts_with('.') && !file_prefix.starts_with('.') {
            continue;
        }

        let mut value = format!("{dir}{name}");
        if is_dir {
            value.push('/');
        }
        suggestions.push(Suggestion {
            value,
            description: i18n::t(if is_dir {
                "completion-path-directory"
            } else {
                "completion-path-file"
            }),
            // Keep the cursor on directories so the user can keep descending.
            append_space: !is_dir,
            draft: false,
        });
    }
    suggestions
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
        .map(|target| Suggestion::value(target.clone(), "completion-saved-target"))
        .collect()
}

fn prefixed(values: Vec<Suggestion>, prefix: &str) -> Vec<Suggestion> {
    values
        .into_iter()
        .filter(|suggestion| suggestion.value.starts_with(prefix))
        .collect()
}

/// Applies a chosen suggestion to the current line, replacing the in-progress
/// word with the suggestion's value. Used when the user accepts a highlighted
/// candidate from the interactive completion panel.
pub fn apply(line: &str, suggestion: &Suggestion) -> String {
    apply_suggestion(line, suggestion)
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
        Suggestion::command("target", "completion-cmd-target"),
        Suggestion::command("upload", "completion-cmd-upload"),
        Suggestion::command("log", "completion-cmd-log"),
        Suggestion::command("language", "completion-cmd-language"),
        Suggestion::command("upgrade", "completion-cmd-upgrade"),
        Suggestion::command("exit", "completion-cmd-exit"),
        Suggestion::command("quit", "completion-cmd-exit"),
    ]
}

fn target_subcommands() -> Vec<Suggestion> {
    vec![
        Suggestion::command("add", "completion-target-add"),
        Suggestion::command("update", "completion-target-update"),
        Suggestion::command("list", "completion-target-list"),
        Suggestion::command("use", "completion-target-use"),
        Suggestion::command("remove", "completion-target-remove"),
    ]
}

fn log_subcommands() -> Vec<Suggestion> {
    vec![
        Suggestion::command("export", "completion-log-export"),
        Suggestion::command("clear", "completion-log-clear"),
    ]
}

fn language_subcommands() -> Vec<Suggestion> {
    vec![
        Suggestion::command("show", "completion-language-show"),
        Suggestion::command("use", "completion-language-use"),
    ]
}

fn language_values() -> Vec<Suggestion> {
    vec![
        Suggestion::command("en", "completion-language-en"),
        Suggestion::command("zh-CN", "completion-language-zh"),
    ]
}

fn target_add_options() -> Vec<Suggestion> {
    vec![
        Suggestion::option("--provider", "completion-opt-provider"),
        Suggestion::option("--bucket", "completion-opt-bucket"),
        Suggestion::option("--endpoint", "completion-opt-endpoint"),
        Suggestion::option("--region", "completion-opt-region"),
        Suggestion::option("--public-base-url", "completion-opt-public-base-url"),
        Suggestion::option("--folder", "completion-opt-folder"),
        Suggestion::option("--access-key-id", "completion-opt-access-key-id"),
        Suggestion::option("--secret-access-key", "completion-opt-secret-access-key"),
        Suggestion::option("--set-default", "completion-opt-set-default"),
        Suggestion::option("--skip-check", "completion-opt-skip-check"),
    ]
}

fn target_update_options() -> Vec<Suggestion> {
    target_add_options()
}

fn upload_options() -> Vec<Suggestion> {
    vec![
        Suggestion::option("--target", "completion-opt-target"),
        Suggestion::option("--folder", "completion-opt-folder"),
        Suggestion::option("--name", "completion-opt-name"),
        Suggestion::option(
            "--ignore-target-folder",
            "completion-opt-ignore-target-folder",
        ),
        Suggestion::option("--markdown", "completion-opt-markdown"),
        Suggestion::option("--dry-run", "completion-opt-dry-run"),
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

    fn target_name_completion(&self) -> Option<(TargetNameScope, &'a str)> {
        let ["target", subcommand, rest @ ..] = self.words.as_slice() else {
            return None;
        };

        let (scope, prefix) = match *subcommand {
            "use" => (
                TargetNameScope::Active,
                self.simple_target_name_prefix(rest)?,
            ),
            "remove" => (
                TargetNameScope::ActiveAndDrafts,
                self.simple_target_name_prefix(rest)?,
            ),
            "add" => (
                TargetNameScope::Drafts,
                self.positional_target_name_prefix(rest)?,
            ),
            "update" => (
                TargetNameScope::ActiveAndDrafts,
                self.positional_target_name_prefix(rest)?,
            ),
            _ => return None,
        };
        Some((scope, prefix))
    }

    /// Returns the partial path being typed when the cursor is on `upload`'s
    /// positional path argument (and not on an option or option value).
    fn upload_path_prefix(&self) -> Option<&'a str> {
        let ["upload", rest @ ..] = self.words.as_slice() else {
            return None;
        };

        let (completed, current) = if self.ends_with_space {
            (rest, "")
        } else {
            match rest.split_last() {
                Some((last, head)) => (head, *last),
                None => (rest, ""),
            }
        };

        if current.starts_with('-') {
            return None;
        }

        let mut index = 0;
        while index < completed.len() {
            let token = completed[index];
            if !token.starts_with('-') {
                // The positional path was already provided earlier in the line.
                return None;
            }

            if token.contains('=') {
                index += 1;
            } else if upload_option_takes_value(token) {
                index += 2;
            } else {
                index += 1;
            }
        }

        // The last completed option consumes `current` as its value, so the
        // cursor is not on the path argument.
        if index > completed.len() {
            return None;
        }

        Some(current)
    }

    fn simple_target_name_prefix(&self, rest: &[&'a str]) -> Option<&'a str> {
        match rest {
            // No name token yet: only offer names once the subcommand word is
            // finished with a space; otherwise the cursor is still on the
            // subcommand and completing here would overwrite it.
            [] => self.ends_with_space.then_some(""),
            [name] if !self.ends_with_space => Some(name),
            _ => None,
        }
    }

    /// Prefix of the positional target name for subcommands where it can appear
    /// among options (`add`, `update`).
    fn positional_target_name_prefix(&self, rest: &[&'a str]) -> Option<&'a str> {
        // No tokens after the subcommand yet: only a trailing space means the
        // cursor has moved off the subcommand word onto the name slot.
        if rest.is_empty() {
            return self.ends_with_space.then_some("");
        }

        let (completed, current) = if self.ends_with_space {
            (rest, "")
        } else {
            let (last, head) = rest.split_last().expect("rest is non-empty");
            (head, *last)
        };

        // The cursor is on an option being typed, not the positional name.
        if current.starts_with('-') {
            return None;
        }

        let mut index = 0;
        while index < completed.len() {
            let token = completed[index];
            if !token.starts_with('-') {
                // A positional name was already provided earlier in the line.
                return None;
            }

            if token.contains('=') {
                index += 1;
            } else if target_option_takes_value(token) {
                index += 2;
            } else {
                index += 1;
            }
        }

        // The last completed option consumes `current` as its value, so the
        // cursor is on that value rather than the name.
        if index > completed.len() {
            return None;
        }

        Some(current)
    }
}

fn target_option_takes_value(option: &str) -> bool {
    matches!(
        option,
        "--provider"
            | "--bucket"
            | "--endpoint"
            | "--region"
            | "--public-base-url"
            | "--folder"
            | "--access-key-id"
            | "--secret-access-key"
    )
}

fn upload_option_takes_value(option: &str) -> bool {
    matches!(option, "--target" | "--folder" | "--prefix" | "--name")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets() -> Vec<String> {
        vec!["cf-wiki-bucket-apac".to_string(), "r2-blog".to_string()]
    }

    fn catalog(active: &[String]) -> TargetCatalog<'_> {
        TargetCatalog {
            active,
            drafts: &[],
        }
    }

    #[test]
    fn hints_target_subcommands_after_complete_target_command() {
        assert_eq!(
            hint("target", catalog(&targets())).unwrap(),
            "hint: add | update | list | use | remove"
        );
    }

    #[test]
    fn completes_unique_root_command_prefix() {
        assert_eq!(
            complete("tar", catalog(&targets())),
            CompletionResult::Insert("target ".to_string())
        );
    }

    #[test]
    fn offers_matching_target_subcommands() {
        assert_eq!(
            hint("target u", catalog(&targets())).unwrap(),
            "hint: update | use"
        );
    }

    #[test]
    fn completes_target_name_for_update() {
        assert_eq!(
            complete("target update cf", catalog(&targets())),
            CompletionResult::Insert("target update cf-wiki-bucket-apac ".to_string())
        );
    }

    #[test]
    fn completes_upload_target_option_value() {
        assert_eq!(
            complete("upload ./a.png --target cf", catalog(&targets())),
            CompletionResult::Insert("upload ./a.png --target cf-wiki-bucket-apac ".to_string())
        );
    }

    #[test]
    fn omits_options_that_are_already_present() {
        let suggestions =
            suggestions_for_line(r#"target add --provider="xxxx" --"#, catalog(&targets()))
                .into_iter()
                .map(|suggestion| suggestion.value)
                .collect::<Vec<_>>();

        assert!(!suggestions.contains(&"--provider".to_string()));
        assert!(suggestions.contains(&"--bucket".to_string()));
        assert!(suggestions.contains(&"--endpoint".to_string()));
    }

    #[test]
    fn omits_upload_options_that_are_already_present() {
        let suggestions = suggestions_for_line("upload ./a.png --markdown --", catalog(&targets()))
            .into_iter()
            .map(|suggestion| suggestion.value)
            .collect::<Vec<_>>();

        assert!(!suggestions.contains(&"--markdown".to_string()));
        assert!(suggestions.contains(&"--target".to_string()));
    }

    #[test]
    fn use_hint_excludes_draft_targets() {
        let active = vec!["cf-wiki-bucket-apac".to_string()];
        let drafts = vec!["eavetest1".to_string()];
        let catalog = TargetCatalog {
            active: &active,
            drafts: &drafts,
        };

        assert_eq!(
            hint("target use ", catalog).unwrap(),
            "hint: cf-wiki-bucket-apac"
        );
    }

    #[test]
    fn remove_hint_marks_draft_targets() {
        let active = vec!["cf-wiki-bucket-apac".to_string()];
        let drafts = vec!["eavetest1".to_string()];
        let catalog = TargetCatalog {
            active: &active,
            drafts: &drafts,
        };

        assert_eq!(
            hint("target remove ", catalog).unwrap(),
            format!("hint: cf-wiki-bucket-apac | eavetest1 {}", draft_marker())
        );
    }

    fn draft_marker() -> String {
        i18n::t("completion-draft-marker")
    }

    #[test]
    fn update_hint_marks_draft_targets() {
        let active = vec!["cf-wiki-bucket-apac".to_string()];
        let drafts = vec!["eavetest1".to_string()];
        let catalog = TargetCatalog {
            active: &active,
            drafts: &drafts,
        };

        assert_eq!(
            hint("target update ", catalog).unwrap(),
            format!("hint: cf-wiki-bucket-apac | eavetest1 {}", draft_marker())
        );
    }

    #[test]
    fn add_hint_lists_only_marked_draft_targets() {
        let active = vec!["cf-wiki-bucket-apac".to_string()];
        let drafts = vec!["eavetest1".to_string()];
        let catalog = TargetCatalog {
            active: &active,
            drafts: &drafts,
        };

        assert_eq!(
            hint("target add ", catalog).unwrap(),
            format!("hint: eavetest1 {}", draft_marker())
        );
    }

    #[test]
    fn tab_on_complete_subcommand_appends_space_instead_of_replacing() {
        // Regression: `target use` (no trailing space) + Tab must not overwrite
        // the `use` subcommand with a target name.
        assert_eq!(
            complete("target use", catalog(&targets())),
            CompletionResult::Insert("target use ".to_string())
        );
        assert_eq!(
            complete("target remove", catalog(&targets())),
            CompletionResult::Insert("target remove ".to_string())
        );
        assert_eq!(
            complete("target update", catalog(&targets())),
            CompletionResult::Insert("target update ".to_string())
        );
    }

    #[test]
    fn tab_after_subcommand_space_completes_target_name() {
        let active = vec!["cf-wiki-bucket-apac".to_string()];

        assert_eq!(
            complete("target use ", catalog(&active)),
            CompletionResult::Insert("target use cf-wiki-bucket-apac ".to_string())
        );
    }

    #[test]
    fn add_completes_resumable_draft_name() {
        let active = vec!["cf-wiki-bucket-apac".to_string()];
        let drafts = vec!["eavetest1".to_string()];
        let catalog = TargetCatalog {
            active: &active,
            drafts: &drafts,
        };

        assert_eq!(
            complete("target add eave", catalog),
            CompletionResult::Insert("target add eavetest1 ".to_string())
        );
    }

    #[test]
    fn splits_path_prefix_into_dir_and_file() {
        assert_eq!(split_path_prefix("./src/ma"), ("./src/", "ma"));
        assert_eq!(split_path_prefix("ma"), ("", "ma"));
        assert_eq!(split_path_prefix("dir/"), ("dir/", ""));
    }

    #[test]
    fn builds_path_suggestions_with_directory_markers() {
        let entries = vec![
            ("main.rs".to_string(), false),
            ("nested".to_string(), true),
            (".hidden".to_string(), false),
        ];

        let suggestions = build_path_suggestions("./src/", "", entries.into_iter());
        let values = suggestions
            .iter()
            .map(|suggestion| suggestion.value.clone())
            .collect::<Vec<_>>();

        assert!(values.contains(&"./src/main.rs".to_string()));
        assert!(values.contains(&"./src/nested/".to_string()));
        assert!(!values.iter().any(|value| value.contains(".hidden")));
        assert!(
            suggestions
                .iter()
                .find(|suggestion| suggestion.value == "./src/nested/")
                .is_some_and(|suggestion| !suggestion.append_space)
        );
    }

    #[test]
    fn detects_upload_path_positional() {
        assert_eq!(
            CompletionContext::new("upload ./co").upload_path_prefix(),
            Some("./co")
        );
        assert_eq!(
            CompletionContext::new("upload --target r2 ").upload_path_prefix(),
            Some("")
        );
        assert_eq!(
            CompletionContext::new("upload cover.png ").upload_path_prefix(),
            None
        );
        assert_eq!(
            CompletionContext::new("upload cover.png --tar").upload_path_prefix(),
            None
        );
        assert_eq!(
            CompletionContext::new("upload --folder posts").upload_path_prefix(),
            None
        );
    }
}
