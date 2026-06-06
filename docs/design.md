# filelift Design

## Product Shape

`filelift` is a general-purpose asset upload CLI. It is intentionally smaller
than sync tools such as rclone: it focuses on taking local publish assets,
placing them in object storage, and returning URLs that can be pasted into
articles, docs, release notes, or websites.

## Core Capabilities

1. Upload local files to S3-compatible object storage.
2. Upload directories recursively for batch publishing.
3. Support multiple named targets across platforms and buckets.
4. Keep secrets encrypted by delegating to the OS keyring.
5. Generate deterministic public URLs from target metadata and object keys.
6. Keep encrypted diagnostic logs that users can export when reporting issues.

## Storage Strategy

The first provider is `s3`, implemented with the official AWS Rust SDK and a
custom endpoint. This covers Cloudflare R2, AWS S3, MinIO, Backblaze B2 S3, and
similar services.

Upload APIs usually return object metadata such as ETag or checksums, not the
public URL. `filelift` treats URL generation as a separate output concern:

```text
public_base_url + "/" + object_key
```

This keeps behavior predictable across providers.

## Secret Strategy

The target store file keeps upload target metadata only. Credentials are stored
under the `filelift` service in the operating system keyring. This gives native encrypted
storage on Windows Credential Manager, macOS Keychain, and Linux Secret Service
where available.

If keyring support fails on a platform, the CLI should return an actionable
error rather than silently writing secrets to disk.

## Diagnostic Log Strategy

The CLI records structured diagnostic events through `tracing`. Business code
uses standard `tracing::info!`, `tracing::warn!`, and `tracing::error!` calls;
the filelift diagnostic layer handles encrypted local storage.

Logs are appended to `~/.filelift/logs/events.log.enc`. Each event is encrypted
independently with a random nonce so the CLI can append events without reading
and rewriting the full log. The log encryption key is stored in the operating
system keyring under the `filelift` service.

Users export readable troubleshooting logs explicitly:

```text
filelift log export --output filelift-debug-log.jsonl
filelift log clear
```

Exported logs are JSONL and should be reviewed before sharing. Secrets such as
access keys, secret keys, authorization headers, passwords, and tokens are
redacted.

## Command Model

```text
filelift target add <name>
filelift target list
filelift target use <name>
filelift target remove <name>
filelift upload <path>
filelift log export
filelift log clear
```

Upload options:

- `--target <name>` overrides the default target.
- `--prefix <path>` chooses the remote key prefix.
- `--recursive` allows directory upload.
- `--name <name>` renames a single uploaded file.
- `--markdown` prints Markdown image links when useful.
- `--dry-run` prints planned keys and URLs without uploading.

## Milestones

1. CLI skeleton and upload targets.
2. Keyring-backed credentials.
3. Dry-run path planning and URL generation.
4. S3-compatible upload.
5. Recursive upload reporting.
6. Release packaging for crates.io.
