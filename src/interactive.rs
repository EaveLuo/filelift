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
    style::{Color, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use inquire::{Confirm, Select};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    interactive_completion::{self, CompletionResult, Suggestion, TargetCatalog},
    output, secret,
    target::{self, TargetStore},
};

const IDLE_HINT_DELAY: Duration = Duration::from_millis(1200);
/// Upper bound on completion rows printed at once, so a large directory cannot
/// flood the terminal; the remainder is summarized as `... N more`.
const MAX_LISTED_CANDIDATES: usize = 50;
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
        let store = TargetStore::load().ok();
        let active_targets = store
            .as_ref()
            .map(TargetStore::target_names)
            .unwrap_or_default();
        let draft_targets = store
            .as_ref()
            .map(TargetStore::draft_only_names)
            .unwrap_or_default();
        let Some(line) = read_line(&active_targets, &draft_targets, &mut history)? else {
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

pub fn idle_hint(line: &str, catalog: TargetCatalog<'_>) -> Option<String> {
    interactive_completion::hint(line, catalog)
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
                TargetSelectionScope::TargetsAndDrafts,
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

/// What occupies the space around the prompt while editing one line. Exactly one
/// variant is active at a time, which is precisely what disambiguates the arrow
/// keys: while a [`CompletionPanel`] is open Up/Down move its selection, and
/// otherwise they browse history.
enum Overlay {
    /// Nothing beyond the prompt line.
    None,
    /// A one-line inline hint drawn after the input (adds no extra rows). Shown
    /// after a brief idle pause.
    Hint(String),
    /// A navigable, multi-row completion list drawn below the prompt.
    Completions(CompletionPanel),
}

impl Overlay {
    fn is_none(&self) -> bool {
        matches!(self, Overlay::None)
    }

    fn hint(&self) -> Option<&str> {
        match self {
            Overlay::Hint(text) => Some(text.as_str()),
            _ => None,
        }
    }

    fn panel(&self) -> Option<&CompletionPanel> {
        match self {
            Overlay::Completions(panel) => Some(panel),
            _ => None,
        }
    }
}

/// A completion list shown below the prompt that the user can navigate with the
/// arrow keys (or Tab) and accept with Enter.
struct CompletionPanel {
    items: Vec<Suggestion>,
    /// Highlighted row, or `None` while the list is shown purely for reference
    /// and no row has been chosen yet.
    selected: Option<usize>,
}

impl CompletionPanel {
    fn new(items: Vec<Suggestion>) -> Self {
        Self {
            items,
            selected: None,
        }
    }

    /// Number of rows that can actually be highlighted, which mirrors the number
    /// of rows the renderer draws (the rest are summarized as `... N more`).
    fn selectable_len(&self) -> usize {
        self.items.len().min(MAX_LISTED_CANDIDATES)
    }

    /// Highlights the next row, wrapping around; selects the first row when
    /// nothing is highlighted yet.
    fn select_next(&mut self) {
        let len = self.selectable_len();
        if len == 0 {
            return;
        }
        self.selected = Some(match self.selected {
            None => 0,
            Some(index) => (index + 1) % len,
        });
    }

    /// Highlights the previous row, wrapping around; selects the last row when
    /// nothing is highlighted yet.
    fn select_prev(&mut self) {
        let len = self.selectable_len();
        if len == 0 {
            return;
        }
        self.selected = Some(match self.selected {
            None | Some(0) => len - 1,
            Some(index) => index - 1,
        });
    }

    fn selected_item(&self) -> Option<&Suggestion> {
        self.selected.and_then(|index| self.items.get(index))
    }
}

/// Outcome of handling one key, telling the read loop whether to keep editing,
/// submit the current line, or abort input entirely.
enum Flow {
    Continue,
    Submit,
    Cancel,
}

/// All mutable state for a single line being read interactively.
///
/// [`LineEditor::handle_key`] is the *only* place that mutates this state, and
/// [`Screen::render`] is the *only* place that draws it. A new key binding is
/// therefore just another match arm here that edits state; it can never corrupt
/// the on-screen bookkeeping that lives in [`Screen`].
struct LineEditor {
    input: String,
    cursor: usize,
    mode: InputMode,
    overlay: Overlay,
    history_cursor: Option<usize>,
    history_draft: String,
}

impl LineEditor {
    fn new() -> Self {
        Self {
            input: String::new(),
            cursor: 0,
            mode: InputMode::Insert,
            overlay: Overlay::None,
            history_cursor: None,
            history_draft: String::new(),
        }
    }

    /// Applies one key event to the editor. Sets `edited` to true when the input
    /// text changed, so the caller can reset the idle-hint timer in one place.
    fn handle_key(
        &mut self,
        key: KeyEvent,
        catalog: TargetCatalog<'_>,
        history: &[String],
        edited: &mut bool,
    ) -> Flow {
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
            } => return Flow::Cancel,
            KeyEvent {
                code: KeyCode::Enter,
                ..
            } => {
                // A highlighted completion is accepted first; only a plain Enter
                // (no panel selection) submits the line.
                if self.accept_selection() {
                    *edited = true;
                    return Flow::Continue;
                }
                return Flow::Submit;
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
                    clear_input(&mut self.input, &mut self.cursor);
                } else {
                    backspace_before_cursor(&mut self.input, &mut self.cursor);
                }
                self.reset_after_edit();
                *edited = true;
            }
            KeyEvent {
                code: KeyCode::Delete,
                ..
            } => {
                delete_at_cursor(&mut self.input, &mut self.cursor);
                self.reset_after_edit();
                *edited = true;
            }
            KeyEvent {
                code: KeyCode::Tab, ..
            } => {
                self.on_tab(catalog);
                *edited = true;
            }
            KeyEvent {
                code: KeyCode::Up, ..
            } => {
                if let Overlay::Completions(panel) = &mut self.overlay {
                    panel.select_prev();
                } else {
                    self.history_prev(history);
                }
            }
            KeyEvent {
                code: KeyCode::Down,
                ..
            } => {
                if let Overlay::Completions(panel) = &mut self.overlay {
                    panel.select_next();
                } else {
                    self.history_next(history);
                }
            }
            KeyEvent {
                code: KeyCode::Left,
                modifiers,
                ..
            } => {
                if modifiers.contains(KeyModifiers::CONTROL) {
                    move_cursor_word_left(&self.input, &mut self.cursor);
                } else {
                    move_cursor_left(&self.input, &mut self.cursor);
                }
                self.overlay = Overlay::None;
            }
            KeyEvent {
                code: KeyCode::Right,
                modifiers,
                ..
            } => {
                if modifiers.contains(KeyModifiers::CONTROL) {
                    move_cursor_word_right(&self.input, &mut self.cursor);
                } else {
                    move_cursor_right(&self.input, &mut self.cursor);
                }
                self.overlay = Overlay::None;
            }
            KeyEvent {
                code: KeyCode::Char('b'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => {
                move_cursor_word_left(&self.input, &mut self.cursor);
                self.overlay = Overlay::None;
            }
            KeyEvent {
                code: KeyCode::Char('f'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => {
                move_cursor_word_right(&self.input, &mut self.cursor);
                self.overlay = Overlay::None;
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
                self.cursor = 0;
                self.overlay = Overlay::None;
            }
            KeyEvent {
                code: KeyCode::End, ..
            }
            | KeyEvent {
                code: KeyCode::Char('e'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => {
                self.cursor = self.input.len();
                self.overlay = Overlay::None;
            }
            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                // Esc dismisses an open panel first; otherwise it drops into
                // normal (vi-style) mode.
                if self.overlay.panel().is_some() {
                    self.overlay = Overlay::None;
                } else {
                    self.mode = InputMode::Normal;
                }
            }
            KeyEvent {
                code: KeyCode::Char(ch),
                modifiers,
                ..
            } if !modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
            {
                if self.mode == InputMode::Normal {
                    handle_normal_mode_key(ch, &mut self.mode, &mut self.input, &mut self.cursor);
                } else {
                    insert_at_cursor(&mut self.input, &mut self.cursor, ch);
                }
                self.reset_after_edit();
                *edited = true;
            }
            _ => {}
        }
        Flow::Continue
    }

    /// Clears transient state after the input text changed.
    fn reset_after_edit(&mut self) {
        self.history_cursor = None;
        self.overlay = Overlay::None;
    }

    /// Accepts the highlighted completion, if any, applying it to the input.
    /// Returns true when a selection was consumed.
    fn accept_selection(&mut self) -> bool {
        let completed = {
            let Some(panel) = self.overlay.panel() else {
                return false;
            };
            let Some(item) = panel.selected_item() else {
                return false;
            };
            interactive_completion::apply(&self.input, item)
        };
        self.input = completed;
        self.cursor = self.input.len();
        self.overlay = Overlay::None;
        self.history_cursor = None;
        true
    }

    /// Handles Tab: advances an already-open panel, inserts the single
    /// unambiguous completion, or opens a panel for multiple matches.
    fn on_tab(&mut self, catalog: TargetCatalog<'_>) {
        if let Overlay::Completions(panel) = &mut self.overlay {
            panel.select_next();
            return;
        }
        match interactive_completion::complete(&self.input, catalog) {
            CompletionResult::Insert(completed) => {
                self.input = completed;
                self.cursor = self.input.len();
                self.overlay = Overlay::None;
            }
            CompletionResult::Candidates(items) => {
                self.overlay = Overlay::Completions(CompletionPanel::new(items));
            }
            CompletionResult::None => self.overlay = Overlay::None,
        }
        self.history_cursor = None;
    }

    fn history_prev(&mut self, history: &[String]) {
        if history.is_empty() {
            return;
        }
        match self.history_cursor {
            None => {
                self.history_draft = self.input.clone();
                self.history_cursor = Some(history.len() - 1);
            }
            Some(index) => self.history_cursor = Some(index.saturating_sub(1)),
        }
        let index = self.history_cursor.expect("history cursor set above");
        self.input = history[index].clone();
        self.cursor = self.input.len();
        self.overlay = Overlay::None;
    }

    fn history_next(&mut self, history: &[String]) {
        let Some(index) = self.history_cursor else {
            return;
        };
        if index + 1 < history.len() {
            self.history_cursor = Some(index + 1);
            self.input = history[index + 1].clone();
        } else {
            self.history_cursor = None;
            self.input = self.history_draft.clone();
        }
        self.cursor = self.input.len();
        self.overlay = Overlay::None;
    }
}

/// Owns the terminal handle and the one piece of bookkeeping the whole rendering
/// scheme relies on: how many rows the previous frame painted below the prompt.
///
/// Every repaint goes through [`Screen::render`], which always clears exactly
/// that many rows before drawing the next frame. Centralizing it here is what
/// makes the "blank gap after the panel disappears" class of bug structurally
/// impossible — no key handler ever touches the row count.
struct Screen {
    stdout: io::Stdout,
    rows_below: usize,
}

impl Screen {
    fn new() -> Self {
        Self {
            stdout: io::stdout(),
            rows_below: 0,
        }
    }

    /// Repaints the whole frame from the editor state, leaving the cursor at the
    /// edit position on the prompt line.
    fn render(&mut self, editor: &LineEditor) -> Result<()> {
        let stdout = &mut self.stdout;
        clear_frame(stdout, self.rows_below)?;

        let hint = editor.overlay.hint();
        let cursor_column = draw_prompt_line(stdout, &editor.input, editor.cursor, hint)?;

        let rows = match editor.overlay.panel() {
            Some(panel) => draw_completion_panel(stdout, panel)?,
            None => 0,
        };

        if rows > 0 {
            execute!(stdout, cursor::MoveUp(rows as u16))?;
        }
        execute!(stdout, cursor::MoveToColumn(cursor_column))?;
        stdout.flush().context("failed to flush terminal")?;

        self.rows_below = rows;
        Ok(())
    }

    /// Commits the entered line to scrollback as `filelift> <input>` and advances
    /// to a fresh line so command output starts cleanly below it.
    fn commit(&mut self, input: &str) -> Result<()> {
        clear_frame(&mut self.stdout, self.rows_below)?;
        write!(self.stdout, "filelift> {input}")?;
        write!(self.stdout, "\r\n")?;
        self.stdout.flush().context("failed to flush terminal")?;
        self.rows_below = 0;
        Ok(())
    }

    /// Clears the frame and advances to a fresh line; used when input is aborted.
    fn finish(&mut self) -> Result<()> {
        clear_frame(&mut self.stdout, self.rows_below)?;
        write!(self.stdout, "\r\n")?;
        self.stdout.flush().context("failed to flush terminal")?;
        self.rows_below = 0;
        Ok(())
    }
}

fn read_line(
    active_targets: &[String],
    draft_targets: &[String],
    history: &mut Vec<String>,
) -> Result<Option<String>> {
    let catalog = TargetCatalog {
        active: active_targets,
        drafts: draft_targets,
    };
    let _raw_mode = RawMode::enter()?;
    let mut screen = Screen::new();
    let mut editor = LineEditor::new();
    let mut last_edit = Instant::now();

    screen.render(&editor)?;

    loop {
        if event::poll(Duration::from_millis(80)).context("failed to poll terminal input")? {
            let event = event::read().context("failed to read terminal input")?;
            let Event::Key(key) = event else {
                continue;
            };
            if key.kind == KeyEventKind::Release {
                continue;
            }

            let mut edited = false;
            match editor.handle_key(key, catalog, history.as_slice(), &mut edited) {
                Flow::Continue => {}
                Flow::Submit => {
                    screen.commit(&editor.input)?;
                    record_history_entry(history, editor.input.trim());
                    return Ok(Some(editor.input));
                }
                Flow::Cancel => {
                    screen.finish()?;
                    return Ok(None);
                }
            }

            if edited {
                last_edit = Instant::now();
            }
            screen.render(&editor)?;
        } else if editor.overlay.is_none()
            && last_edit.elapsed() >= IDLE_HINT_DELAY
            && let Some(hint) = idle_hint(&editor.input, catalog)
        {
            editor.overlay = Overlay::Hint(hint);
            screen.render(&editor)?;
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

/// Draws the prompt, the current input, and an optional inline hint on a single
/// terminal line, leaving the cursor at the logical edit position.
///
/// The interactive editor deliberately keeps everything on one line: completion
/// candidates are emitted as committed scrollback by [`show_candidates`] rather
/// than as a multi-row overlay. That keeps clearing trivial (always just the
/// current line) and avoids leaving blank gaps behind once the terminal scrolls.
/// Clears the current frame — the prompt line plus `rows_below` rows beneath it —
/// leaving the cursor at column 0 of the prompt line.
///
/// This is the single primitive every repaint funnels through, which is why the
/// renderer can guarantee no stale rows survive into the next frame.
fn clear_frame(stdout: &mut io::Stdout, rows_below: usize) -> Result<()> {
    execute!(
        stdout,
        cursor::MoveToColumn(0),
        Clear(ClearType::CurrentLine)
    )
    .context("failed to clear interactive prompt")?;
    for _ in 0..rows_below {
        execute!(
            stdout,
            cursor::MoveDown(1),
            cursor::MoveToColumn(0),
            Clear(ClearType::CurrentLine)
        )
        .context("failed to clear interactive panel")?;
    }
    if rows_below > 0 {
        execute!(
            stdout,
            cursor::MoveUp(rows_below as u16),
            cursor::MoveToColumn(0)
        )
        .context("failed to restore interactive cursor")?;
    }
    Ok(())
}

/// Draws the prompt, the current input, and an optional inline hint on the
/// current line. Assumes the line was already cleared by [`clear_frame`] and
/// returns the terminal column where the edit cursor belongs.
fn draw_prompt_line(
    stdout: &mut io::Stdout,
    input: &str,
    cursor_index: usize,
    hint: Option<&str>,
) -> Result<u16> {
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
    Ok(cursor_column)
}

/// Draws the completion panel on the rows below the prompt, highlighting the
/// selected row, and returns how many rows it painted so the renderer can record
/// the frame height. Leaves the cursor at the end of the last painted row.
fn draw_completion_panel(stdout: &mut io::Stdout, panel: &CompletionPanel) -> Result<usize> {
    let shown = panel.items.len().min(MAX_LISTED_CANDIDATES);
    let mut rows = 0;
    for (index, suggestion) in panel.items[..shown].iter().enumerate() {
        write!(stdout, "\r\n")?;
        if panel.selected == Some(index) {
            execute!(
                stdout,
                SetBackgroundColor(Color::Cyan),
                SetForegroundColor(Color::Black)
            )?;
            write!(stdout, "> {}  {}", suggestion.value, suggestion.description)?;
            execute!(stdout, ResetColor)?;
        } else {
            execute!(stdout, SetForegroundColor(Color::Cyan))?;
            write!(stdout, "  {}", suggestion.value)?;
            execute!(stdout, SetForegroundColor(Color::DarkGrey))?;
            write!(stdout, "  {}", suggestion.description)?;
            execute!(stdout, ResetColor)?;
        }
        rows += 1;
    }
    if panel.items.len() > shown {
        write!(stdout, "\r\n")?;
        execute!(stdout, SetForegroundColor(Color::DarkGrey))?;
        write!(stdout, "  ... {} more", panel.items.len() - shown)?;
        execute!(stdout, ResetColor)?;
        rows += 1;
    }
    Ok(rows)
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

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn sample_panel(count: usize) -> CompletionPanel {
        let items = (0..count)
            .map(|index| Suggestion {
                value: format!("item{index}"),
                description: String::new(),
                append_space: true,
                draft: false,
            })
            .collect();
        CompletionPanel::new(items)
    }

    #[test]
    fn completion_panel_navigation_wraps_in_both_directions() {
        let mut panel = sample_panel(3);
        assert_eq!(panel.selected, None);

        panel.select_next();
        assert_eq!(panel.selected, Some(0));
        panel.select_next();
        assert_eq!(panel.selected, Some(1));
        panel.select_next();
        assert_eq!(panel.selected, Some(2));
        panel.select_next();
        assert_eq!(panel.selected, Some(0), "next wraps to the first row");

        panel.select_prev();
        assert_eq!(panel.selected, Some(2), "prev wraps to the last row");
    }

    #[test]
    fn completion_panel_prev_from_unselected_picks_last_row() {
        let mut panel = sample_panel(3);
        panel.select_prev();
        assert_eq!(panel.selected, Some(2));
    }

    #[test]
    fn arrows_drive_the_panel_while_open_and_history_when_closed() {
        let history = vec!["upload a".to_string(), "target list".to_string()];
        let catalog = TargetCatalog {
            active: &[],
            drafts: &[],
        };
        let mut editor = LineEditor::new();
        let mut edited = false;

        // With no panel, Up browses history (most recent first).
        editor.handle_key(press(KeyCode::Up), catalog, &history, &mut edited);
        assert_eq!(editor.input, "target list");
        assert!(editor.overlay.is_none());

        // While a panel is open, Up/Down move the selection and never touch
        // either the input or history navigation.
        editor.overlay = Overlay::Completions(sample_panel(2));
        editor.handle_key(press(KeyCode::Down), catalog, &history, &mut edited);
        assert_eq!(editor.input, "target list");
        assert_eq!(editor.overlay.panel().unwrap().selected, Some(0));
        editor.handle_key(press(KeyCode::Down), catalog, &history, &mut edited);
        assert_eq!(editor.overlay.panel().unwrap().selected, Some(1));
    }

    #[test]
    fn enter_accepts_highlighted_completion_without_submitting() {
        let catalog = TargetCatalog {
            active: &[],
            drafts: &[],
        };
        let mut editor = LineEditor::new();
        editor.input = "tar".to_string();
        editor.cursor = editor.input.len();
        editor.overlay = Overlay::Completions(CompletionPanel::new(vec![Suggestion {
            value: "target".to_string(),
            description: String::new(),
            append_space: true,
            draft: false,
        }]));
        if let Overlay::Completions(panel) = &mut editor.overlay {
            panel.select_next();
        }

        let mut edited = false;
        let flow = editor.handle_key(press(KeyCode::Enter), catalog, &[], &mut edited);

        assert!(matches!(flow, Flow::Continue));
        assert_eq!(editor.input, "target ");
        assert!(editor.overlay.is_none());
    }

    #[test]
    fn enter_submits_when_no_completion_is_highlighted() {
        let catalog = TargetCatalog {
            active: &[],
            drafts: &[],
        };
        let mut editor = LineEditor::new();
        editor.input = "exit".to_string();
        editor.overlay = Overlay::Completions(sample_panel(2));

        let mut edited = false;
        let flow = editor.handle_key(press(KeyCode::Enter), catalog, &[], &mut edited);

        assert!(
            matches!(flow, Flow::Submit),
            "an open panel with no highlighted row still submits on Enter"
        );
    }

    #[test]
    fn esc_dismisses_the_panel_before_changing_mode() {
        let catalog = TargetCatalog {
            active: &[],
            drafts: &[],
        };
        let mut editor = LineEditor::new();
        editor.overlay = Overlay::Completions(sample_panel(2));

        let mut edited = false;
        editor.handle_key(press(KeyCode::Esc), catalog, &[], &mut edited);
        assert!(editor.overlay.is_none());
        assert_eq!(
            editor.mode,
            InputMode::Insert,
            "first Esc only closes panel"
        );

        editor.handle_key(press(KeyCode::Esc), catalog, &[], &mut edited);
        assert_eq!(
            editor.mode,
            InputMode::Normal,
            "second Esc enters normal mode"
        );
    }

    #[test]
    fn editing_closes_an_open_panel() {
        let catalog = TargetCatalog {
            active: &[],
            drafts: &[],
        };
        let mut editor = LineEditor::new();
        editor.overlay = Overlay::Completions(sample_panel(2));

        let mut edited = false;
        editor.handle_key(press(KeyCode::Char('x')), catalog, &[], &mut edited);

        assert_eq!(editor.input, "x");
        assert!(editor.overlay.is_none());
        assert!(edited);
    }

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
        let active = vec!["assets-cdn".to_string(), "r2-blog".to_string()];
        let catalog = TargetCatalog {
            active: &active,
            drafts: &[],
        };

        assert_eq!(
            idle_hint("target update ", catalog).unwrap(),
            "hint: assets-cdn | r2-blog"
        );
    }

    #[test]
    fn does_not_hint_when_command_is_complete() {
        let active = vec!["r2-blog".to_string()];
        let catalog = TargetCatalog {
            active: &active,
            drafts: &[],
        };

        assert!(idle_hint("target update r2-blog", catalog).is_none());
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
