use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{Event, Level, Subscriber, field::Visit};
use tracing_subscriber::{Layer, layer::Context as LayerContext, prelude::*, registry::LookupSpan};

use crate::{secret, target};

const LOG_KEY_ENV: &str = "FILELIFT_LOG_KEY_HEX";
const LOG_KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// Caches the resolved diagnostic-log key for the process so the encrypted
/// secret store is not decrypted on every log event.
static LOG_KEY_CACHE: std::sync::OnceLock<[u8; LOG_KEY_LEN]> = std::sync::OnceLock::new();

pub fn init() {
    let layer = DiagnosticLogLayer::default();
    let subscriber = tracing_subscriber::registry().with(layer);
    let _ = tracing::subscriber::set_global_default(subscriber);
}

pub fn log_path() -> Result<PathBuf> {
    Ok(target::filelift_home_dir()?
        .join("logs")
        .join("events.log.enc"))
}

pub fn export_to(path: &Path) -> Result<usize> {
    let events = read_events()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create log export directory at {}",
                parent.display()
            )
        })?;
    }

    let mut output = String::new();
    for event in &events {
        output.push_str(&serde_json::to_string(event).context("failed to serialize log event")?);
        output.push('\n');
    }

    fs::write(path, output).with_context(|| {
        format!(
            "failed to write diagnostic log export to {}",
            path.display()
        )
    })?;
    Ok(events.len())
}

pub fn clear() -> Result<()> {
    let path = log_path()?;
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("failed to remove diagnostic log at {}", path.display()))?;
    }
    Ok(())
}

pub fn record_command_result(command: &str, target_name: Option<&str>, result: &str) {
    match target_name {
        Some(target_name) => {
            tracing::info!(command, target = target_name, result, "command finished");
        }
        None => {
            tracing::info!(command, result, "command finished");
        }
    }
}

fn read_events() -> Result<Vec<Value>> {
    let path = log_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&path).with_context(|| {
        format!(
            "failed to read encrypted diagnostic log at {}",
            path.display()
        )
    })?;
    let key = log_key()?;
    let cipher = ChaCha20Poly1305::new_from_slice(&key).context("invalid diagnostic log key")?;
    let mut events = Vec::new();

    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let encrypted: EncryptedEvent =
            serde_json::from_str(line).context("failed to parse encrypted diagnostic log line")?;
        let nonce = STANDARD
            .decode(encrypted.nonce)
            .context("failed to decode diagnostic log nonce")?;
        let ciphertext = STANDARD
            .decode(encrypted.ciphertext)
            .context("failed to decode diagnostic log ciphertext")?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
            .map_err(|_| anyhow::anyhow!("failed to decrypt diagnostic log event"))?;
        let event =
            serde_json::from_slice(&plaintext).context("failed to parse diagnostic log event")?;
        events.push(redact_event(event));
    }

    Ok(events)
}

fn append_event(event: Value) -> Result<()> {
    let path = log_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create diagnostic log directory at {}",
                parent.display()
            )
        })?;
    }

    let key = log_key()?;
    let cipher = ChaCha20Poly1305::new_from_slice(&key).context("invalid diagnostic log key")?;
    let mut nonce = [0_u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);

    let plaintext =
        serde_json::to_vec(&event).context("failed to serialize diagnostic log event")?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|_| anyhow::anyhow!("failed to encrypt diagnostic log event"))?;
    let encrypted = EncryptedEvent {
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(ciphertext),
    };

    let mut line =
        serde_json::to_string(&encrypted).context("failed to serialize encrypted log event")?;
    line.push('\n');

    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open diagnostic log at {}", path.display()))?
        .write_all(line.as_bytes())
        .with_context(|| format!("failed to append diagnostic log at {}", path.display()))
}

fn log_key() -> Result<[u8; LOG_KEY_LEN]> {
    if let Some(key) = LOG_KEY_CACHE.get() {
        return Ok(*key);
    }

    let key = resolve_log_key()?;
    let _ = LOG_KEY_CACHE.set(key);
    Ok(key)
}

/// Resolves the diagnostic-log key from the env override, then the secret store,
/// generating and persisting a fresh key when none exists yet.
fn resolve_log_key() -> Result<[u8; LOG_KEY_LEN]> {
    if let Ok(value) = env::var(LOG_KEY_ENV) {
        return decode_hex_key(&value);
    }

    match secret::diagnostic_log_key() {
        Ok(value) => decode_hex_key(&value),
        Err(_) => {
            let mut key = [0_u8; LOG_KEY_LEN];
            OsRng.fill_bytes(&mut key);
            let encoded = encode_hex(&key);
            secret::set_diagnostic_log_key(&encoded)?;
            Ok(key)
        }
    }
}

fn decode_hex_key(value: &str) -> Result<[u8; LOG_KEY_LEN]> {
    let value = value.trim();
    if value.len() != LOG_KEY_LEN * 2 {
        bail!("diagnostic log key must be 64 hex characters");
    }

    let mut key = [0_u8; LOG_KEY_LEN];
    for (index, byte) in key.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(&value[start..start + 2], 16)
            .context("diagnostic log key contains non-hex characters")?;
    }
    Ok(key)
}

fn encode_hex(bytes: &[u8; LOG_KEY_LEN]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn redact_event(mut event: Value) -> Value {
    if let Some(fields) = event.get_mut("fields").and_then(Value::as_object_mut) {
        for key in [
            "secret_access_key",
            "access_key_id",
            "authorization",
            "password",
            "token",
        ] {
            if fields.contains_key(key) {
                fields.insert(key.to_string(), Value::String("[redacted]".to_string()));
            }
        }
    }
    event
}

#[derive(Debug, Serialize, Deserialize)]
struct EncryptedEvent {
    nonce: String,
    ciphertext: String,
}

#[derive(Debug, Default)]
struct DiagnosticLogLayer {
    lock: Mutex<()>,
}

impl<S> Layer<S> for DiagnosticLogLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, _context: LayerContext<'_, S>) {
        if event.metadata().target().starts_with("aws_") {
            return;
        }

        let _guard = self.lock.lock().ok();
        let mut visitor = JsonVisitor::default();
        event.record(&mut visitor);

        let log_event = json!({
            "timestamp_ms": now_ms(),
            "level": level_name(event.metadata().level()),
            "target": event.metadata().target(),
            "filelift_version": env!("CARGO_PKG_VERSION"),
            "os": env::consts::OS,
            "fields": visitor.fields,
        });

        let _ = append_event(log_event);
    }
}

#[derive(Default)]
struct JsonVisitor {
    fields: BTreeMap<String, Value>,
}

impl Visit for JsonVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.fields.insert(
            field.name().to_string(),
            Value::String(format!("{value:?}")),
        );
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.fields
            .insert(field.name().to_string(), Value::String(value.to_string()));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), Value::Bool(value));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), Value::Number(value.into()));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), Value::Number(value.into()));
    }
}

fn level_name(level: &Level) -> &'static str {
    match *level {
        Level::ERROR => "error",
        Level::WARN => "warn",
        Level::INFO => "info",
        Level::DEBUG => "debug",
        Level::TRACE => "trace",
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

use std::io::Write;
