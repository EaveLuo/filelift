use std::{
    io::{self, IsTerminal, Write},
    process::Command,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    style::{Color, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use inquire::{Confirm, Select};

use crate::{
    interactive_completion::{self, CompletionResult, Suggestion},
    output,
    target::TargetStore,
};

const IDLE_HINT_DELAY: Duration = Duration::from_millis(1200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetSelectionRequest {
    Use,
    Update,
    Remove,
}

#[derive(Debug, Clone, Copy)]
pub enum TargetSelectionScope {
    TargetsOnly,
    TargetsAndDrafts,
}

pub async fn run() -> Result<()> {
    if !io::stdin().is_terminal() {
        bail!(
            "interactive mode requires a terminal; pass a command such as `filelift target list`"
        );
    }

    anstream::println!(
        "{}",
        output::info("filelift interactive mode. Type `exit` to leave.")
    );

    let mut history = Vec::new();

    loop {
        let targets = TargetStore::load()
            .map(|store| store.target_and_draft_names())
            .unwrap_or_default();
        let Some(line) = read_line(&targets, &mut history)? else {
            break;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if matches!(line, "exit" | "quit") {
            break;
        }

        let mut args = parse_interactive_line(line)?;
        if let Some(request) = target_selection_request(args_as_strs(&args).as_slice()) {
            match select_target_for_request(request) {
                Ok(name) => args.push(name),
                Err(error) => {
                    anstream::eprintln!("{}", output::warning(&format!("{error:#}")));
                    continue;
                }
            }
        }

        if let Err(error) = run_filelift_command(&args) {
            anstream::eprintln!("{}", output::error(&format!("Error: {error:#}")));
        }
    }

    Ok(())
}

pub fn parse_interactive_line(line: &str) -> Result<Vec<String>> {
    shell_words::split(line).context("failed to parse command line")
}

pub fn idle_hint(line: &str, targets: &[String]) -> Option<String> {
    interactive_completion::hint(line, targets)
}

pub fn target_selection_request(args: &[&str]) -> Option<TargetSelectionRequest> {
    let ["target", subcommand, rest @ ..] = args else {
        return None;
    };

    let request = match *subcommand {
        "use" => TargetSelectionRequest::Use,
        "update" => TargetSelectionRequest::Update,
        "remove" => TargetSelectionRequest::Remove,
        _ => return None,
    };

    if target_name_is_missing(request, rest) {
        Some(request)
    } else {
        None
    }
}

fn target_name_is_missing(request: TargetSelectionRequest, rest: &[&str]) -> bool {
    let mut index = 0;
    while index < rest.len() {
        let token = rest[index];
        if !token.starts_with('-') {
            return false;
        }

        if let Some((name, _value)) = token.split_once('=')
            && option_takes_value(request, name)
        {
            index += 1;
            continue;
        }

        if option_takes_value(request, token) {
            index += 2;
        } else {
            index += 1;
        }
    }

    true
}

fn option_takes_value(request: TargetSelectionRequest, option: &str) -> bool {
    match request {
        TargetSelectionRequest::Update => matches!(
            option,
            "--provider"
                | "--bucket"
                | "--endpoint"
                | "--region"
                | "--public-base-url"
                | "--access-key-id"
                | "--secret-access-key"
        ),
        TargetSelectionRequest::Use | TargetSelectionRequest::Remove => false,
    }
}

pub fn resolve_target_name(
    value: Option<String>,
    request: TargetSelectionRequest,
) -> Result<String> {
    match value {
        Some(value) => Ok(value),
        None if io::stdin().is_terminal() => select_target_for_request(request),
        None => bail!(
            "target name required; pass one explicitly or run `filelift` for interactive mode"
        ),
    }
}

pub fn select_target_for_request(request: TargetSelectionRequest) -> Result<String> {
    match request {
        TargetSelectionRequest::Update => select_target_name(
            "Select a target to update",
            TargetSelectionScope::TargetsAndDrafts,
        ),
        TargetSelectionRequest::Use => {
            select_target_name("Select a target to use", TargetSelectionScope::TargetsOnly)
        }
        TargetSelectionRequest::Remove => {
            let name = select_target_name(
                "Select a target to remove",
                TargetSelectionScope::TargetsOnly,
            )?;
            let confirmed = Confirm::new(&format!("Remove target `{name}`?"))
                .with_default(false)
                .prompt()
                .context("failed to confirm target removal")?;
            if !confirmed {
                bail!("target removal cancelled");
            }
            Ok(name)
        }
    }
}

fn select_target_name(message: &str, scope: TargetSelectionScope) -> Result<String> {
    let store = TargetStore::load()?;
    let names = match scope {
        TargetSelectionScope::TargetsOnly => store.target_names(),
        TargetSelectionScope::TargetsAndDrafts => store.target_and_draft_names(),
    };
    if names.is_empty() {
        bail!("no targets configured; run `filelift target add <name>` first");
    }

    Select::new(message, names)
        .prompt()
        .context("failed to select target")
}

fn read_line(targets: &[String], history: &mut Vec<String>) -> Result<Option<String>> {
    let _raw_mode = RawMode::enter()?;
    let mut stdout = io::stdout();
    let mut input = String::new();
    let mut visible_hint: Option<String> = None;
    let mut last_edit = Instant::now();
    let mut history_cursor: Option<usize> = None;
    let mut history_draft = String::new();

    render_line(&mut stdout, &input, visible_hint.as_deref())?;

    loop {
        if event::poll(Duration::from_millis(80)).context("failed to poll terminal input")? {
            let event = event::read().context("failed to read terminal input")?;
            let Event::Key(key) = event else {
                continue;
            };
            if key.kind == KeyEventKind::Release {
                continue;
            }

            match key {
                KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                }
                | KeyEvent {
                    code: KeyCode::Char('d'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                } => {
                    clear_line(&mut stdout)?;
                    writeln!(stdout)?;
                    return Ok(None);
                }
                KeyEvent {
                    code: KeyCode::Enter,
                    ..
                } => {
                    clear_line(&mut stdout)?;
                    write!(stdout, "filelift> {}", input)?;
                    writeln!(stdout)?;
                    let line = input.trim();
                    if !line.is_empty() && history.last().is_none_or(|last| last != line) {
                        history.push(line.to_string());
                    }
                    return Ok(Some(input));
                }
                KeyEvent {
                    code: KeyCode::Backspace,
                    ..
                } => {
                    input.pop();
                    history_cursor = None;
                    visible_hint = None;
                    last_edit = Instant::now();
                    render_line(&mut stdout, &input, None)?;
                }
                KeyEvent {
                    code: KeyCode::Tab, ..
                } => {
                    match interactive_completion::complete(&input, targets) {
                        CompletionResult::Insert(completed) => input = completed,
                        CompletionResult::Candidates(candidates) => {
                            render_completion_candidates(&mut stdout, &candidates)?
                        }
                        CompletionResult::None => {}
                    }
                    history_cursor = None;
                    visible_hint = None;
                    last_edit = Instant::now();
                    render_line(&mut stdout, &input, None)?;
                }
                KeyEvent {
                    code: KeyCode::Up, ..
                } => {
                    if !history.is_empty() {
                        if history_cursor.is_none() {
                            history_draft = input.clone();
                            history_cursor = Some(history.len() - 1);
                        } else if let Some(index) = history_cursor.as_mut() {
                            *index = index.saturating_sub(1);
                        }
                        input = history[history_cursor.unwrap()].clone();
                        visible_hint = None;
                        render_line(&mut stdout, &input, None)?;
                    }
                }
                KeyEvent {
                    code: KeyCode::Down,
                    ..
                } => {
                    if let Some(index) = history_cursor {
                        if index + 1 < history.len() {
                            history_cursor = Some(index + 1);
                            input = history[index + 1].clone();
                        } else {
                            history_cursor = None;
                            input = history_draft.clone();
                        }
                        visible_hint = None;
                        render_line(&mut stdout, &input, None)?;
                    }
                }
                KeyEvent {
                    code: KeyCode::Char(ch),
                    modifiers,
                    ..
                } if !modifiers.contains(KeyModifiers::CONTROL)
                    && !modifiers.contains(KeyModifiers::ALT) =>
                {
                    input.push(ch);
                    history_cursor = None;
                    visible_hint = None;
                    last_edit = Instant::now();
                    render_line(&mut stdout, &input, None)?;
                }
                _ => {}
            }
        } else if visible_hint.is_none() && last_edit.elapsed() >= IDLE_HINT_DELAY {
            visible_hint = idle_hint(&input, targets);
            if visible_hint.is_some() {
                render_line(&mut stdout, &input, visible_hint.as_deref())?;
            }
        }
    }
}

fn run_filelift_command(args: &[String]) -> Result<()> {
    let executable = std::env::current_exe().context("failed to resolve filelift executable")?;
    let status = Command::new(executable)
        .args(args)
        .status()
        .context("failed to run filelift command")?;
    if !status.success() {
        bail!("command exited with status {status}");
    }
    Ok(())
}

fn render_completion_candidates(stdout: &mut io::Stdout, candidates: &[Suggestion]) -> Result<()> {
    clear_line(stdout)?;
    writeln!(stdout)?;
    for suggestion in candidates.iter().take(8) {
        execute!(stdout, SetForegroundColor(Color::Cyan))?;
        write!(stdout, "  {}", suggestion.value)?;
        execute!(stdout, SetForegroundColor(Color::DarkGrey))?;
        writeln!(stdout, "  {}", suggestion.description)?;
        execute!(stdout, ResetColor)?;
    }
    if candidates.len() > 8 {
        execute!(stdout, SetForegroundColor(Color::DarkGrey))?;
        writeln!(stdout, "  ... {} more", candidates.len() - 8)?;
        execute!(stdout, ResetColor)?;
    }
    stdout.flush().context("failed to render completions")
}

fn args_as_strs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

fn render_line(stdout: &mut io::Stdout, input: &str, hint: Option<&str>) -> Result<()> {
    clear_line(stdout)?;
    let prompt = "filelift> ";
    let width = terminal::size()
        .map(|(width, _)| width as usize)
        .unwrap_or(80);
    let hint_prefix = "  ";
    let hint_width = hint
        .map(|hint| hint_prefix.chars().count() + hint.chars().count())
        .unwrap_or_default();
    let prompt_width = prompt.chars().count();
    let input_budget = width.saturating_sub(prompt_width + hint_width).max(8);
    let display_input = visible_tail(input, input_budget);

    write!(stdout, "{prompt}{display_input}")?;
    let cursor_column =
        (prompt_width + display_input.chars().count()).min(width.saturating_sub(1)) as u16;
    if let Some(hint) = hint {
        let used = prompt_width + display_input.chars().count();
        let hint_budget = width.saturating_sub(used + hint_prefix.chars().count());
        let display_hint = visible_prefix(hint, hint_budget);
        execute!(stdout, SetForegroundColor(Color::DarkGrey))?;
        write!(stdout, "{hint_prefix}{display_hint}")?;
        execute!(stdout, ResetColor)?;
        execute!(stdout, cursor::MoveToColumn(cursor_column))?;
    }
    stdout.flush().context("failed to flush terminal")
}

fn visible_tail(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let tail_width = max_chars.saturating_sub(3);
    let mut tail = value.chars().rev().take(tail_width).collect::<Vec<_>>();
    tail.reverse();
    format!("...{}", tail.into_iter().collect::<String>())
}

fn visible_prefix(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let prefix_width = max_chars.saturating_sub(3);
    let prefix = value.chars().take(prefix_width).collect::<String>();
    format!("{prefix}...")
}

fn clear_line(stdout: &mut io::Stdout) -> Result<()> {
    execute!(
        stdout,
        cursor::MoveToColumn(0),
        Clear(ClearType::CurrentLine)
    )
    .context("failed to redraw terminal")
}

struct RawMode;

impl RawMode {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode().context("failed to enter raw terminal mode")?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_interactive_command_like_a_shell() {
        let args = parse_interactive_line("target update \"r2 blog\" --bucket assets").unwrap();

        assert_eq!(
            args,
            vec!["target", "update", "r2 blog", "--bucket", "assets"]
        );
    }

    #[test]
    fn hints_without_selecting_when_target_name_is_missing() {
        let targets = vec!["assets-cdn".to_string(), "r2-blog".to_string()];

        assert_eq!(
            idle_hint("target update", &targets).unwrap(),
            "hint: assets-cdn | r2-blog"
        );
    }

    #[test]
    fn does_not_hint_when_command_is_complete() {
        let targets = vec!["r2-blog".to_string()];

        assert!(idle_hint("target update r2-blog", &targets).is_none());
    }

    #[test]
    fn detects_target_selection_only_for_missing_target_names() {
        assert_eq!(
            target_selection_request(&["target", "use"]),
            Some(TargetSelectionRequest::Use)
        );
        assert_eq!(
            target_selection_request(&["target", "update"]),
            Some(TargetSelectionRequest::Update)
        );
        assert_eq!(
            target_selection_request(&["target", "remove"]),
            Some(TargetSelectionRequest::Remove)
        );
        assert_eq!(
            target_selection_request(&["target", "use", "r2-blog"]),
            None
        );
        assert_eq!(target_selection_request(&["target", "list"]), None);
    }

    #[test]
    fn detects_missing_target_name_when_options_are_present() {
        assert_eq!(
            target_selection_request(&["target", "update", "--bucket", "assets"]),
            Some(TargetSelectionRequest::Update)
        );
        assert_eq!(
            target_selection_request(&["target", "update", "--skip-check"]),
            Some(TargetSelectionRequest::Update)
        );
        assert_eq!(
            target_selection_request(&["target", "update", "r2-blog", "--bucket", "assets"]),
            None
        );
    }

    #[test]
    fn truncates_long_input_without_exceeding_budget() {
        assert_eq!(visible_tail("abcdef", 4), "...f");
        assert_eq!(visible_prefix("abcdef", 4), "a...");
        assert_eq!(visible_prefix("abcdef", 2), "..");
    }
}
