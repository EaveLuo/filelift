use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, IsTerminal, Write},
    process::Command,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, OsRng, rand_core::RngCore},
};
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
    output, secret,
    target::{self, TargetStore},
};

const IDLE_HINT_DELAY: Duration = Duration::from_millis(1200);
const HISTORY_LIMIT: usize = 200;
const HISTORY_KEY_ENV: &str = "FILELIFT_HISTORY_KEY_HEX";
const HISTORY_KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

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

    let mut history_store = HistoryStore::load().unwrap_or_default();
    let history_key = HistoryStore::current_key().unwrap_or_else(|_| "global".to_string());
    let mut history = history_store.entries_for(&history_key);

    loop {
        let targets = TargetStore::load()
            .map(|store| store.target_and_draft_names())
            .unwrap_or_default();
        let Some(line) = read_line(&targets, &mut history)? else {
            break;
        };
        history_store.replace_entries(&history_key, history.clone());
        if let Err(error) = history_store.save() {
            anstream::eprintln!(
                "{}",
                output::warning(&format!("failed to save interactive history: {error:#}"))
            );
        }
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

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
struct HistoryStore {
    #[serde(default)]
    directories: BTreeMap<String, Vec<String>>,
}

impl HistoryStore {
    fn load() -> Result<Self> {
        let encrypted_path = encrypted_history_path()?;
        if encrypted_path.exists() {
            let content = fs::read_to_string(&encrypted_path).with_context(|| {
                format!(
                    "failed to read encrypted interactive history at {}",
                    encrypted_path.display()
                )
            })?;
            return decrypt_history_store(&content).with_context(|| {
                format!(
                    "failed to decrypt interactive history at {}",
                    encrypted_path.display()
                )
            });
        }

        let legacy_path = legacy_history_path()?;
        if legacy_path.exists() {
            let content = fs::read_to_string(&legacy_path).with_context(|| {
                format!(
                    "failed to read legacy interactive history at {}",
                    legacy_path.display()
                )
            })?;
            return toml::from_str(&content).with_context(|| {
                format!(
                    "failed to parse legacy interactive history at {}",
                    legacy_path.display()
                )
            });
        }

        Ok(Self::default())
    }

    fn save(&self) -> Result<()> {
        let path = encrypted_history_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create interactive history directory at {}",
                    parent.display()
                )
            })?;
        }

        let plaintext = toml::to_string_pretty(self).context("failed to serialize history")?;
        let content = encrypt_history_store(&plaintext)?;
        fs::write(&path, content).with_context(|| {
            format!("failed to write interactive history at {}", path.display())
        })?;

        let legacy_path = legacy_history_path()?;
        if legacy_path.exists() {
            fs::remove_file(&legacy_path).with_context(|| {
                format!(
                    "failed to remove legacy plaintext history at {}",
                    legacy_path.display()
                )
            })?;
        }

        Ok(())
    }

    fn current_key() -> Result<String> {
        env::current_dir()
            .context("failed to resolve current directory")
            .map(|path| path.to_string_lossy().into_owned())
    }

    fn entries_for(&self, key: &str) -> Vec<String> {
        self.directories.get(key).cloned().unwrap_or_default()
    }

    fn replace_entries(&mut self, key: &str, entries: Vec<String>) {
        if entries.is_empty() {
            self.directories.remove(key);
        } else {
            self.directories.insert(key.to_string(), entries);
        }
    }
}

fn encrypted_history_path() -> Result<std::path::PathBuf> {
    Ok(target::filelift_home_dir()?.join("history.toml.enc"))
}

fn legacy_history_path() -> Result<std::path::PathBuf> {
    Ok(target::filelift_home_dir()?.join("history.toml"))
}

fn record_history_entry(history: &mut Vec<String>, line: &str) {
    if line.trim().is_empty() {
        return;
    }
    if contains_sensitive_history_input(line) {
        return;
    }
    if history.last().is_some_and(|last| last == line) {
        return;
    }

    history.push(line.to_string());
    if history.len() > HISTORY_LIMIT {
        history.drain(..history.len() - HISTORY_LIMIT);
    }
}

fn contains_sensitive_history_input(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    [
        "--access-key-id",
        "--secret-access-key",
        "access_key_id",
        "secret_access_key",
        "authorization",
        "password",
        "token",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn encrypt_history_store(plaintext: &str) -> Result<String> {
    let key = history_key()?;
    encrypt_history_store_with_key(plaintext, &key)
}

fn encrypt_history_store_with_key(plaintext: &str, key: &[u8; HISTORY_KEY_LEN]) -> Result<String> {
    let cipher = ChaCha20Poly1305::new_from_slice(key).context("invalid history key")?;
    let mut nonce = [0_u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
        .map_err(|_| anyhow::anyhow!("failed to encrypt interactive history"))?;
    let encrypted = EncryptedHistory {
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(ciphertext),
    };

    toml::to_string_pretty(&encrypted).context("failed to serialize encrypted history")
}

fn decrypt_history_store(content: &str) -> Result<HistoryStore> {
    let key = history_key()?;
    decrypt_history_store_with_key(content, &key)
}

fn decrypt_history_store_with_key(
    content: &str,
    key: &[u8; HISTORY_KEY_LEN],
) -> Result<HistoryStore> {
    let encrypted: EncryptedHistory =
        toml::from_str(content).context("failed to parse encrypted history")?;
    let nonce = STANDARD
        .decode(encrypted.nonce)
        .context("failed to decode history nonce")?;
    let ciphertext = STANDARD
        .decode(encrypted.ciphertext)
        .context("failed to decode history ciphertext")?;
    let cipher = ChaCha20Poly1305::new_from_slice(key).context("invalid history key")?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| anyhow::anyhow!("failed to decrypt interactive history"))?;
    let plaintext =
        String::from_utf8(plaintext).context("decrypted history contains non-UTF-8 data")?;

    toml::from_str(&plaintext).context("failed to parse decrypted history")
}

fn history_key() -> Result<[u8; HISTORY_KEY_LEN]> {
    if let Ok(value) = env::var(HISTORY_KEY_ENV) {
        return decode_hex_history_key(&value);
    }

    match secret::interactive_history_key() {
        Ok(value) => decode_hex_history_key(&value),
        Err(_) => {
            let mut key = [0_u8; HISTORY_KEY_LEN];
            OsRng.fill_bytes(&mut key);
            let encoded = encode_hex_history_key(&key);
            secret::set_interactive_history_key(&encoded)?;
            Ok(key)
        }
    }
}

fn decode_hex_history_key(value: &str) -> Result<[u8; HISTORY_KEY_LEN]> {
    let value = value.trim();
    if value.len() != HISTORY_KEY_LEN * 2 {
        bail!("history key must be 64 hex characters");
    }

    let mut key = [0_u8; HISTORY_KEY_LEN];
    for (index, byte) in key.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(&value[start..start + 2], 16)
            .context("history key contains non-hex characters")?;
    }
    Ok(key)
}

fn encode_hex_history_key(bytes: &[u8; HISTORY_KEY_LEN]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct EncryptedHistory {
    nonce: String,
    ciphertext: String,
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
                    record_history_entry(history, line);
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
    fn records_distinct_history_entries() {
        let mut history = Vec::new();

        record_history_entry(&mut history, "target list");
        record_history_entry(&mut history, "target list");
        record_history_entry(&mut history, "target use assets");

        assert_eq!(history, vec!["target list", "target use assets"]);
    }

    #[test]
    fn skips_sensitive_history_entries() {
        let mut history = Vec::new();

        record_history_entry(
            &mut history,
            "target add r2 --secret-access-key very-secret",
        );
        record_history_entry(&mut history, "target update r2 --access-key-id key-id");
        record_history_entry(&mut history, "target list");

        assert_eq!(history, vec!["target list"]);
    }

    #[test]
    fn does_not_record_empty_history_entries() {
        let mut history = Vec::new();

        record_history_entry(&mut history, "");
        record_history_entry(&mut history, "   ");

        assert!(history.is_empty());
    }

    #[test]
    fn trims_history_to_limit() {
        let mut history = Vec::new();

        for index in 0..(HISTORY_LIMIT + 5) {
            record_history_entry(&mut history, &format!("target list {index}"));
        }

        assert_eq!(history.len(), HISTORY_LIMIT);
        assert_eq!(history.first().unwrap(), "target list 5");
    }

    #[test]
    fn encrypts_history_without_plaintext_commands() {
        let mut store = HistoryStore::default();
        store.replace_entries(
            "C:/repo",
            vec!["target list".to_string(), "upload cover.png".to_string()],
        );
        let plaintext = toml::to_string_pretty(&store).unwrap();
        let key = [7_u8; HISTORY_KEY_LEN];

        let encrypted = encrypt_history_store_with_key(&plaintext, &key).unwrap();

        assert!(!encrypted.contains("target list"));
        assert!(!encrypted.contains("upload cover.png"));
        let decrypted = decrypt_history_store_with_key(&encrypted, &key).unwrap();
        assert_eq!(
            decrypted.entries_for("C:/repo"),
            vec!["target list".to_string(), "upload cover.png".to_string()]
        );
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
