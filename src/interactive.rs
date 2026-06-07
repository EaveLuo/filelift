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
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    interactive_completion::{self, CompletionResult, Suggestion},
    output,
    target::TargetStore,
};

const IDLE_HINT_DELAY: Duration = Duration::from_millis(1200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Insert,
    Normal,
}

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
    let mut cursor_index = 0;
    let mut visible_hint: Option<String> = None;
    let mut candidates = Vec::new();
    let mut rendered_rows = 0;
    let mut last_edit = Instant::now();
    let mut history_cursor: Option<usize> = None;
    let mut history_draft = String::new();
    let mut mode = InputMode::Insert;

    rendered_rows = render_screen(
        &mut stdout,
        rendered_rows,
        &input,
        cursor_index,
        visible_hint.as_deref(),
        &candidates,
    )?;

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
                    clear_rendered_screen(&mut stdout, rendered_rows)?;
                    writeln!(stdout)?;
                    return Ok(None);
                }
                KeyEvent {
                    code: KeyCode::Enter,
                    ..
                } => {
                    clear_rendered_screen(&mut stdout, rendered_rows)?;
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
                }
                | KeyEvent {
                    code: KeyCode::Char('h'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                }
                | KeyEvent {
                    code: KeyCode::Char('w'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                } => {
                    if clears_input_key(key) {
                        clear_input(&mut input, &mut cursor_index);
                    } else {
                        backspace_before_cursor(&mut input, &mut cursor_index);
                    }
                    history_cursor = None;
                    visible_hint = None;
                    candidates.clear();
                    last_edit = Instant::now();
                    rendered_rows = render_screen(
                        &mut stdout,
                        rendered_rows,
                        &input,
                        cursor_index,
                        None,
                        &candidates,
                    )?;
                }
                KeyEvent {
                    code: KeyCode::Delete,
                    ..
                } => {
                    delete_at_cursor(&mut input, &mut cursor_index);
                    history_cursor = None;
                    visible_hint = None;
                    candidates.clear();
                    last_edit = Instant::now();
                    rendered_rows = render_screen(
                        &mut stdout,
                        rendered_rows,
                        &input,
                        cursor_index,
                        None,
                        &candidates,
                    )?;
                }
                KeyEvent {
                    code: KeyCode::Tab, ..
                } => {
                    match interactive_completion::complete(&input, targets) {
                        CompletionResult::Insert(completed) => {
                            input = completed;
                            cursor_index = input.len();
                            candidates.clear();
                        }
                        CompletionResult::Candidates(next_candidates) => {
                            candidates = next_candidates
                        }
                        CompletionResult::None => {}
                    }
                    history_cursor = None;
                    visible_hint = None;
                    last_edit = Instant::now();
                    rendered_rows = render_screen(
                        &mut stdout,
                        rendered_rows,
                        &input,
                        cursor_index,
                        None,
                        &candidates,
                    )?;
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
                        cursor_index = input.len();
                        visible_hint = None;
                        candidates.clear();
                        rendered_rows = render_screen(
                            &mut stdout,
                            rendered_rows,
                            &input,
                            cursor_index,
                            None,
                            &candidates,
                        )?;
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
                        cursor_index = input.len();
                        visible_hint = None;
                        candidates.clear();
                        rendered_rows = render_screen(
                            &mut stdout,
                            rendered_rows,
                            &input,
                            cursor_index,
                            None,
                            &candidates,
                        )?;
                    }
                }
                KeyEvent {
                    code: KeyCode::Left,
                    modifiers,
                    ..
                } => {
                    if modifiers.contains(KeyModifiers::CONTROL) {
                        move_cursor_word_left(&input, &mut cursor_index);
                    } else {
                        move_cursor_left(&input, &mut cursor_index);
                    }
                    candidates.clear();
                    visible_hint = None;
                    rendered_rows = render_screen(
                        &mut stdout,
                        rendered_rows,
                        &input,
                        cursor_index,
                        None,
                        &candidates,
                    )?;
                }
                KeyEvent {
                    code: KeyCode::Right,
                    modifiers,
                    ..
                } => {
                    if modifiers.contains(KeyModifiers::CONTROL) {
                        move_cursor_word_right(&input, &mut cursor_index);
                    } else {
                        move_cursor_right(&input, &mut cursor_index);
                    }
                    candidates.clear();
                    visible_hint = None;
                    rendered_rows = render_screen(
                        &mut stdout,
                        rendered_rows,
                        &input,
                        cursor_index,
                        None,
                        &candidates,
                    )?;
                }
                KeyEvent {
                    code: KeyCode::Char('b'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                } => {
                    move_cursor_word_left(&input, &mut cursor_index);
                    candidates.clear();
                    visible_hint = None;
                    rendered_rows = render_screen(
                        &mut stdout,
                        rendered_rows,
                        &input,
                        cursor_index,
                        None,
                        &candidates,
                    )?;
                }
                KeyEvent {
                    code: KeyCode::Char('f'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                } => {
                    move_cursor_word_right(&input, &mut cursor_index);
                    candidates.clear();
                    visible_hint = None;
                    rendered_rows = render_screen(
                        &mut stdout,
                        rendered_rows,
                        &input,
                        cursor_index,
                        None,
                        &candidates,
                    )?;
                }
                KeyEvent {
                    code: KeyCode::Home,
                    ..
                }
                | KeyEvent {
                    code: KeyCode::Char('a'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                } => {
                    cursor_index = 0;
                    candidates.clear();
                    visible_hint = None;
                    rendered_rows = render_screen(
                        &mut stdout,
                        rendered_rows,
                        &input,
                        cursor_index,
                        None,
                        &candidates,
                    )?;
                }
                KeyEvent {
                    code: KeyCode::End, ..
                }
                | KeyEvent {
                    code: KeyCode::Char('e'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                } => {
                    cursor_index = input.len();
                    candidates.clear();
                    visible_hint = None;
                    rendered_rows = render_screen(
                        &mut stdout,
                        rendered_rows,
                        &input,
                        cursor_index,
                        None,
                        &candidates,
                    )?;
                }
                KeyEvent {
                    code: KeyCode::Esc, ..
                } => {
                    mode = InputMode::Normal;
                    candidates.clear();
                    visible_hint = None;
                    rendered_rows = render_screen(
                        &mut stdout,
                        rendered_rows,
                        &input,
                        cursor_index,
                        None,
                        &candidates,
                    )?;
                }
                KeyEvent {
                    code: KeyCode::Char(ch),
                    modifiers,
                    ..
                } if !modifiers.contains(KeyModifiers::CONTROL)
                    && !modifiers.contains(KeyModifiers::ALT) =>
                {
                    if mode == InputMode::Normal {
                        handle_normal_mode_key(ch, &mut mode, &mut input, &mut cursor_index);
                    } else {
                        insert_at_cursor(&mut input, &mut cursor_index, ch);
                    }
                    history_cursor = None;
                    visible_hint = None;
                    candidates.clear();
                    last_edit = Instant::now();
                    rendered_rows = render_screen(
                        &mut stdout,
                        rendered_rows,
                        &input,
                        cursor_index,
                        None,
                        &candidates,
                    )?;
                }
                _ => {}
            }
        } else if visible_hint.is_none() && last_edit.elapsed() >= IDLE_HINT_DELAY {
            visible_hint = idle_hint(&input, targets);
            if visible_hint.is_some() {
                rendered_rows = render_screen(
                    &mut stdout,
                    rendered_rows,
                    &input,
                    cursor_index,
                    visible_hint.as_deref(),
                    &candidates,
                )?;
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

fn args_as_strs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

fn render_screen(
    stdout: &mut io::Stdout,
    previous_rows: usize,
    input: &str,
    cursor_index: usize,
    hint: Option<&str>,
    candidates: &[Suggestion],
) -> Result<usize> {
    clear_rendered_screen(stdout, previous_rows)?;
    let prompt = "filelift> ";
    let width = terminal::size()
        .map(|(width, _)| width as usize)
        .unwrap_or(80);
    let hint_prefix = "  ";
    let hint_width = hint
        .map(|hint| display_width(hint_prefix) + display_width(hint))
        .unwrap_or_default();
    let prompt_width = display_width(prompt);
    let input_budget = width.saturating_sub(prompt_width + hint_width).max(8);
    let display_input = visible_tail(input, input_budget);
    let visible_cursor_offset = visible_cursor_offset(input, cursor_index, input_budget);

    write!(stdout, "{prompt}{display_input}")?;
    let cursor_column = (prompt_width + visible_cursor_offset).min(width.saturating_sub(1)) as u16;
    if let Some(hint) = hint {
        let used = prompt_width + display_width(&display_input);
        let hint_budget = width.saturating_sub(used + display_width(hint_prefix));
        let display_hint = visible_prefix(hint, hint_budget);
        execute!(stdout, SetForegroundColor(Color::DarkGrey))?;
        write!(stdout, "{hint_prefix}{display_hint}")?;
        execute!(stdout, ResetColor)?;
    }

    for suggestion in candidates {
        writeln!(stdout)?;
        execute!(stdout, SetForegroundColor(Color::Cyan))?;
        write!(stdout, "  {}", suggestion.value)?;
        execute!(stdout, SetForegroundColor(Color::DarkGrey))?;
        write!(stdout, "  {}", suggestion.description)?;
        execute!(stdout, ResetColor)?;
    }

    if !candidates.is_empty() {
        execute!(stdout, cursor::MoveUp(candidates.len() as u16))?;
    }
    execute!(stdout, cursor::MoveToColumn(cursor_column))?;
    stdout.flush().context("failed to flush terminal")?;
    Ok(candidates.len())
}

fn clear_rendered_screen(stdout: &mut io::Stdout, rows_below_prompt: usize) -> Result<()> {
    execute!(
        stdout,
        cursor::MoveToColumn(0),
        Clear(ClearType::CurrentLine)
    )
    .context("failed to clear interactive prompt")?;

    for _ in 0..rows_below_prompt {
        execute!(
            stdout,
            cursor::MoveDown(1),
            cursor::MoveToColumn(0),
            Clear(ClearType::CurrentLine)
        )
        .context("failed to clear interactive completions")?;
    }
    if rows_below_prompt > 0 {
        execute!(
            stdout,
            cursor::MoveUp(rows_below_prompt as u16),
            cursor::MoveToColumn(0)
        )
        .context("failed to restore interactive cursor")?;
    }
    Ok(())
}

fn visible_tail(value: &str, max_chars: usize) -> String {
    if display_width(value) <= max_chars {
        return value.to_string();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let tail_budget = max_chars.saturating_sub(3);
    let mut width = 0;
    let mut start = value.len();
    for (index, ch) in value.char_indices().rev() {
        let ch_width = char_width(ch);
        if width + ch_width > tail_budget {
            break;
        }
        width += ch_width;
        start = index;
    }
    format!("...{}", &value[start..])
}

fn visible_prefix(value: &str, max_chars: usize) -> String {
    if display_width(value) <= max_chars {
        return value.to_string();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let prefix_budget = max_chars.saturating_sub(3);
    let mut width = 0;
    let mut end = 0;
    for (index, ch) in value.char_indices() {
        let ch_width = char_width(ch);
        if width + ch_width > prefix_budget {
            break;
        }
        width += ch_width;
        end = index + ch.len_utf8();
    }
    format!("{}...", &value[..end])
}

fn visible_cursor_offset(input: &str, cursor_index: usize, max_chars: usize) -> usize {
    if display_width(input) <= max_chars {
        return display_width(&input[..cursor_index]);
    }
    if max_chars <= 3 {
        return 0;
    }

    let tail_width = max_chars.saturating_sub(3);
    let tail_start = visible_tail_start(input, tail_width);
    if cursor_index <= tail_start {
        3
    } else {
        3 + display_width(&input[tail_start..cursor_index])
    }
}

fn visible_tail_start(value: &str, max_width: usize) -> usize {
    let mut width = 0;
    let mut start = value.len();
    for (index, ch) in value.char_indices().rev() {
        let ch_width = char_width(ch);
        if width + ch_width > max_width {
            break;
        }
        width += ch_width;
        start = index;
    }
    start
}

fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

fn char_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}

fn insert_at_cursor(input: &mut String, cursor_index: &mut usize, ch: char) {
    input.insert(*cursor_index, ch);
    *cursor_index += ch.len_utf8();
}

fn backspace_before_cursor(input: &mut String, cursor_index: &mut usize) {
    if *cursor_index == 0 {
        return;
    }
    move_cursor_left(input, cursor_index);
    delete_at_cursor(input, cursor_index);
}

fn clear_input(input: &mut String, cursor_index: &mut usize) {
    input.clear();
    *cursor_index = 0;
}

fn clears_input_key(key: KeyEvent) -> bool {
    match key {
        KeyEvent {
            code: KeyCode::Backspace,
            modifiers,
            ..
        } => modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT),
        KeyEvent {
            code: KeyCode::Char('h') | KeyCode::Char('w'),
            modifiers: KeyModifiers::CONTROL,
            ..
        } => true,
        _ => false,
    }
}

fn delete_at_cursor(input: &mut String, cursor_index: &mut usize) {
    if *cursor_index >= input.len() {
        return;
    }
    let next = next_char_boundary(input, *cursor_index);
    input.replace_range(*cursor_index..next, "");
}

fn move_cursor_left(input: &str, cursor_index: &mut usize) {
    if *cursor_index == 0 {
        return;
    }
    *cursor_index = previous_char_boundary(input, *cursor_index);
}

fn move_cursor_right(input: &str, cursor_index: &mut usize) {
    if *cursor_index >= input.len() {
        return;
    }
    *cursor_index = next_char_boundary(input, *cursor_index);
}

fn move_cursor_word_left(input: &str, cursor_index: &mut usize) {
    if *cursor_index == 0 {
        return;
    }

    while *cursor_index > 0 {
        let previous = previous_char_boundary(input, *cursor_index);
        if !input[previous..*cursor_index]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            break;
        }
        *cursor_index = previous;
    }

    while *cursor_index > 0 {
        let previous = previous_char_boundary(input, *cursor_index);
        if input[previous..*cursor_index]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            break;
        }
        *cursor_index = previous;
    }
}

fn move_cursor_word_right(input: &str, cursor_index: &mut usize) {
    if *cursor_index >= input.len() {
        return;
    }

    while *cursor_index < input.len() {
        let next = next_char_boundary(input, *cursor_index);
        if input[*cursor_index..next]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            break;
        }
        *cursor_index = next;
    }

    while *cursor_index < input.len() {
        let next = next_char_boundary(input, *cursor_index);
        if !input[*cursor_index..next]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            break;
        }
        *cursor_index = next;
    }
}

fn previous_char_boundary(input: &str, cursor_index: usize) -> usize {
    input[..cursor_index]
        .char_indices()
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn next_char_boundary(input: &str, cursor_index: usize) -> usize {
    input[cursor_index..]
        .char_indices()
        .nth(1)
        .map(|(index, _)| cursor_index + index)
        .unwrap_or(input.len())
}

fn handle_normal_mode_key(
    ch: char,
    mode: &mut InputMode,
    input: &mut String,
    cursor_index: &mut usize,
) {
    match ch {
        'h' => move_cursor_left(input, cursor_index),
        'l' => move_cursor_right(input, cursor_index),
        '0' => *cursor_index = 0,
        '$' => *cursor_index = input.len(),
        'x' => delete_at_cursor(input, cursor_index),
        'i' => *mode = InputMode::Insert,
        'a' => {
            move_cursor_right(input, cursor_index);
            *mode = InputMode::Insert;
        }
        'I' => {
            *cursor_index = 0;
            *mode = InputMode::Insert;
        }
        'A' => {
            *cursor_index = input.len();
            *mode = InputMode::Insert;
        }
        _ => {}
    }
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

    #[test]
    fn truncation_respects_display_columns() {
        let input = "\u{8bed}\u{8a00} use";

        assert_eq!(display_width(input), 8);
        assert_eq!(visible_tail(input, 7), "... use");
        assert_eq!(visible_prefix(input, 7), "\u{8bed}\u{8a00}...");
    }

    #[test]
    fn visible_cursor_offset_respects_wide_glyph_columns() {
        let input = "\u{8bed}\u{8a00} use";
        let after_wide_glyphs = "\u{8bed}\u{8a00}".len();

        assert_eq!(visible_cursor_offset(input, after_wide_glyphs, 20), 4);
    }

    #[test]
    fn edits_at_cursor_without_appending() {
        let mut input = "target update".to_string();
        let mut cursor_index = "target ".len();

        insert_at_cursor(&mut input, &mut cursor_index, 'X');
        assert_eq!(input, "target Xupdate");

        backspace_before_cursor(&mut input, &mut cursor_index);
        assert_eq!(input, "target update");
        assert_eq!(cursor_index, "target ".len());
    }

    #[test]
    fn clears_input_and_cursor() {
        let mut input = "target add --bucket assets".to_string();
        let mut cursor_index = "target add".len();

        clear_input(&mut input, &mut cursor_index);

        assert!(input.is_empty());
        assert_eq!(cursor_index, 0);
    }

    #[test]
    fn recognizes_modified_backspace_clear_aliases() {
        assert!(clears_input_key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::CONTROL
        )));
        assert!(clears_input_key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::ALT
        )));
        assert!(clears_input_key(KeyEvent::new(
            KeyCode::Char('h'),
            KeyModifiers::CONTROL
        )));
        assert!(clears_input_key(KeyEvent::new(
            KeyCode::Char('w'),
            KeyModifiers::CONTROL
        )));
        assert!(!clears_input_key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE
        )));
    }

    #[test]
    fn cursor_movement_respects_utf8_boundaries() {
        let input = "\u{8bed}\u{8a00} use".to_string();
        let mut cursor_index = input.len();

        move_cursor_left(&input, &mut cursor_index);
        move_cursor_left(&input, &mut cursor_index);
        assert!(input.is_char_boundary(cursor_index));

        move_cursor_right(&input, &mut cursor_index);
        assert!(input.is_char_boundary(cursor_index));
    }

    #[test]
    fn word_movement_skips_over_words() {
        let input = "target update cf-wiki".to_string();
        let mut cursor_index = input.len();

        move_cursor_word_left(&input, &mut cursor_index);
        assert_eq!(&input[cursor_index..], "cf-wiki");

        move_cursor_word_left(&input, &mut cursor_index);
        assert_eq!(&input[cursor_index..], "update cf-wiki");

        move_cursor_word_right(&input, &mut cursor_index);
        assert_eq!(&input[cursor_index..], "cf-wiki");
    }
}
