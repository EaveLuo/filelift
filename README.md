# filelift

`filelift` is a small CLI for uploading local assets to S3-compatible object
storage and printing shareable URLs.

It is designed for writing, publishing, and static-site workflows where images,
videos, and other assets should live outside the source repository.

## Goals

- Upload one file or a directory of files.
- Support Cloudflare R2, AWS S3, MinIO, and other S3-compatible services.
- Manage multiple upload targets.
- Store secrets through the operating system keyring instead of plain text.
- Print public URLs and Markdown snippets after uploads.

## Install or Update

macOS and Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/EaveLuo/filelift/main/scripts/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/EaveLuo/filelift/main/scripts/install.ps1 | iex
```

The installer downloads the latest GitHub Release binary, installs it into a
user-owned directory, and adds that directory to the user PATH. Run the same
command again later to update filelift in place.

- macOS/Linux default: `~/.local/bin`
- Windows default: `%LOCALAPPDATA%\Programs\filelift\bin`

To install a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/EaveLuo/filelift/main/scripts/install.sh | FILELIFT_VERSION=v0.2.3 sh
```

```powershell
$env:FILELIFT_VERSION = "v0.2.3"; irm https://raw.githubusercontent.com/EaveLuo/filelift/main/scripts/install.ps1 | iex
```

## Planned CLI

```bash
filelift target add r2-blog \
  --bucket eave-assets \
  --endpoint https://example.r2.cloudflarestorage.com \
  --region auto \
  --public-base-url https://assets.example.com

filelift target use r2-blog
filelift upload ./cover.webp --prefix blog/2026/my-post --markdown
filelift upload ./assets --recursive --prefix blog/2026/my-post
filelift log export --output filelift-debug-log.jsonl
```

## Interactive Mode

Run `filelift` without a subcommand to open the interactive shell:

```text
filelift> target update
```

When a command is missing a target name, filelift shows a dim hint after a short
pause without interrupting typing. Press `Tab` to choose from configured targets,
or press `Enter` with the target name still missing to open the selector.

Full commands still run directly and are suitable for scripts:

```powershell
filelift target use r2-blog
filelift target update r2-blog --bucket eave-assets --skip-check
```

## Target Store

Targets contain non-secret storage metadata:

The target store is saved at `~/.filelift/targets.toml`.

```toml
default_target = "r2-blog"

[targets.r2-blog]
provider = "s3"
bucket = "eave-assets"
endpoint = "https://example.r2.cloudflarestorage.com"
region = "auto"
public_base_url = "https://assets.example.com"
```

Access keys are stored separately in the system keyring:

- service: `filelift`
- account: `<target-name>:access_key_id`
- account: `<target-name>:secret_access_key`

## Diagnostic Logs

`filelift` writes encrypted diagnostic logs to `~/.filelift/logs/events.log.enc`.
The log encryption key is stored in the operating system keyring. Logs can be
exported to a readable JSONL file for troubleshooting:

```bash
filelift log export --output filelift-debug-log.jsonl
filelift log clear
```

Review exported logs before sharing them. Secrets are redacted and are never
written to the encrypted log intentionally.

## Architecture

- `cli`: command definitions and argument parsing.
- `target`: target store load/save.
- `secret`: keyring integration.
- `diagnostic_log`: encrypted diagnostic logging and export.
- `log_command`: diagnostic log command handlers.
- `target_command`: target command handlers.
- `storage`: storage provider interface.
- `storage::s3`: S3-compatible upload implementation.
- `output`: URL and Markdown formatting.

## Status

This repository is in early scaffolding. The first implementation milestone is
target management plus single and recursive uploads to S3-compatible storage.
