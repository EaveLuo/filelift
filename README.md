# filelift

[![CI](https://github.com/EaveLuo/filelift/actions/workflows/ci.yml/badge.svg)](https://github.com/EaveLuo/filelift/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/EaveLuo/filelift?sort=semver)](https://github.com/EaveLuo/filelift/releases)
[![Crates.io](https://img.shields.io/crates/v/filelift.svg)](https://crates.io/crates/filelift)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A small, fast CLI for lifting local files to S3-compatible object storage and
printing shareable URLs.

English | [简体中文](README.zh-CN.md)

---

`filelift` is built for writing, publishing, and static-site workflows where
images, videos, and other assets should live outside the source repository. It
is also **AI-first**: every command runs non-interactively and resolves secrets
without an OS keyring, so it works the same on your laptop, in CI, inside
containers, and within agent sandboxes.

## Features

- **One command, any S3** — upload a single file or a whole directory (recursive
  by default) to Cloudflare R2, AWS S3, MinIO, Backblaze B2, or any
  S3-compatible service.
- **Multiple named targets** — manage per-project buckets, endpoints, regions,
  and public base URLs in one place.
- **Sandbox-friendly secrets** — resolve credentials from CLI flags/stdin,
  environment variables, or a machine-encrypted local file. No OS keyring, no
  interactive desktop session required.
- **Shareable output** — print public URLs, Markdown snippets, or machine-
  readable JSON after every upload.
- **Self-updating** — `filelift upgrade` updates the binary in place, and a
  throttled background check tells you when a new release is out.
- **Encrypted diagnostics** — opt-in encrypted local logs you can export as
  JSONL when you need to troubleshoot.

## Install

macOS and Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/EaveLuo/filelift/main/scripts/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/EaveLuo/filelift/main/scripts/install.ps1 | iex
```

The installer downloads the latest GitHub Release binary, installs it into a
user-owned directory, and adds that directory to your user `PATH`:

- macOS / Linux default: `~/.local/bin`
- Windows default: `%LOCALAPPDATA%\Programs\filelift\bin`

Install a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/EaveLuo/filelift/main/scripts/install.sh | FILELIFT_VERSION=v0.3.0 sh
```

```powershell
$env:FILELIFT_VERSION = "v0.3.0"; irm https://raw.githubusercontent.com/EaveLuo/filelift/main/scripts/install.ps1 | iex
```

From crates.io (requires a Rust toolchain):

```bash
cargo install filelift
```

## Quick start

```bash
# 1. Register a target (you will be prompted for the access keys).
filelift target add r2-blog \
  --bucket eave-assets \
  --endpoint https://example.r2.cloudflarestorage.com \
  --region auto \
  --public-base-url https://assets.example.com \
  --set-default

# 2. Upload a file and get a Markdown snippet.
filelift upload ./cover.webp --folder blog/2026/my-post --markdown

# 3. Upload an entire directory (recursive by default).
filelift upload ./assets --folder blog/2026/my-post
```

## Commands

| Command | Description |
| --- | --- |
| `filelift upload <path>` | Upload a file or directory and print URLs. |
| `filelift target add <name>` | Register a new upload target. |
| `filelift target update [name]` | Update an existing target. |
| `filelift target list` | List configured targets. |
| `filelift target use [name]` | Set the default target. |
| `filelift target remove [name]` | Remove a target and its stored credentials. |
| `filelift credentials export [name]` | Print stored credentials as env vars. |
| `filelift log export` / `log clear` | Export or clear encrypted diagnostic logs. |
| `filelift language show` / `language use <lang>` | Inspect or set the CLI language. |
| `filelift upgrade` (alias `update`) | Update filelift to the latest release. |

Useful `upload` flags:

- `--target <name>` — override the default target for this upload.
- `--folder <prefix>` (alias `--prefix`) — key prefix within the bucket.
- `--name <name>` — override the uploaded object name.
- `--markdown` — print a Markdown image/link snippet.
- `--output json` — emit machine-readable JSON (great for scripts and agents).
- `--dry-run` — plan the upload without sending anything.

Run `filelift <command> --help` for the full set of options.

## Credentials and security model

Target metadata (bucket, endpoint, region, public base URL) is non-secret and is
stored at `~/.filelift/targets.toml`. Access keys are kept **separately** in an
encrypted file at `~/.filelift/secrets.enc` (`0600`).

When a credential is needed, filelift resolves it in priority order:

1. **Explicit CLI argument / stdin** — `--access-key-id`,
   `--secret-access-key`, or `--secret-access-key-stdin`.
2. **Environment variables** — per-target
   `FILELIFT_<TARGET>_ACCESS_KEY_ID` / `FILELIFT_<TARGET>_SECRET_ACCESS_KEY`,
   then global `FILELIFT_ACCESS_KEY_ID` / `FILELIFT_SECRET_ACCESS_KEY`.
3. **The encrypted file store**.

Pipe a secret in without it ever touching your shell history or argv:

```bash
echo "$SECRET" | filelift target add r2-blog --bucket ... --access-key-id AKIA... --secret-access-key-stdin
```

Export stored credentials for CI or an agent sandbox:

```bash
filelift credentials export r2-blog --format shell   # or: --format dotenv
eval "$(filelift credentials export r2-blog --format shell)"
```

The store is encrypted with a key derived from a stable machine identifier
(overridable via `FILELIFT_MASTER_KEY_HEX`). This protects against accidental
exfiltration — commits, backups, file copies — but **not** against a malicious
process running as the same user. For stronger isolation, inject credentials via
environment variables from your own secret manager.

### Non-interactive use

Pass `--non-interactive` to `target add` / `target update` to fail fast instead
of prompting when a required value is missing — ideal for CI and automation.

## Environment variables

| Variable | Purpose |
| --- | --- |
| `FILELIFT_ACCESS_KEY_ID` / `FILELIFT_SECRET_ACCESS_KEY` | Global credentials. |
| `FILELIFT_<TARGET>_ACCESS_KEY_ID` / `FILELIFT_<TARGET>_SECRET_ACCESS_KEY` | Per-target credentials. |
| `FILELIFT_MASTER_KEY_HEX` | Override the secret-store encryption key. |
| `FILELIFT_VERSION` | Pin the version for the install script. |
| `FILELIFT_INSTALL_DIR` | Override the install directory. |
| `FILELIFT_NO_UPDATE_CHECK` | Set to `1` to disable update checks. |

## Interactive mode

Run `filelift` without a subcommand to open the interactive shell:

```text
filelift> target update
```

When a command is missing a target name, filelift shows a dim hint after a short
pause without interrupting typing. Press `Tab` to choose from configured targets,
or press `Enter` with the target name still missing to open the selector. Type
`exit` or `quit` to leave.

## Updating

Update from the CLI itself (this re-runs the official installer for your
platform):

```bash
filelift upgrade               # or: filelift update
filelift upgrade --version v0.3.0
```

filelift checks GitHub for a newer release at most once a day (only from an
interactive terminal) and prints a one-line notice when an update is available.
Set `FILELIFT_NO_UPDATE_CHECK=1` to disable this check.

### Install channels and PATH

filelift can be installed two ways, and `filelift upgrade` follows the channel
the running binary came from:

- **Prebuilt (install script)** → installs to the user dir above. `upgrade`
  re-runs the install script.
- **`cargo install`** → installs to `~/.cargo/bin`. `upgrade` runs
  `cargo install filelift --force` (on Windows it prints the command, because a
  running `.exe` cannot replace itself).

If you have both, the one earlier on `PATH` wins (commonly `~/.cargo/bin`). The
install script warns you when a different copy will shadow what it just
installed. Run `where filelift` (Windows) or `which -a filelift` (Unix) to see
which copy is active.

## Diagnostic logs

filelift can write encrypted diagnostic logs to
`~/.filelift/logs/events.log.enc`. The log encryption key lives in the encrypted
secret store. Export them to readable JSONL when troubleshooting:

```bash
filelift log export --output filelift-debug-log.jsonl
filelift log clear
```

Review exported logs before sharing them; secrets are redacted and are never
intentionally written to the log.

## Architecture

- `cli` — command definitions and argument parsing.
- `target` / `target_command` — target store and target command handlers.
- `secret` — encrypted local secret store and credential resolution.
- `secret_file` — hardened (`0600`) secret file read/write helpers.
- `machine_key` — derivation of the secret-store encryption key.
- `credentials_command` — `credentials export` handler.
- `upload` — upload planning and execution.
- `storage` / `storage::s3` — storage provider interface and S3 implementation.
- `diagnostic_log` / `log_command` — encrypted diagnostic logging and export.
- `update_check` / `upgrade_command` — release checks and self-update.
- `output` — URL and Markdown formatting.

## Contributing

Issues and pull requests are welcome. Before opening a PR, please run:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

## License

Licensed under the [MIT License](LICENSE).
