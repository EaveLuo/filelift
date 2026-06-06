# filelift Design

## Product Shape

`filelift` is a general-purpose asset upload CLI. It is intentionally smaller
than sync tools such as rclone: it focuses on taking local publish assets,
placing them in object storage, and returning URLs that can be pasted into
articles, docs, release notes, or websites.

## Core Capabilities

1. Upload local files to S3-compatible object storage.
2. Upload directories recursively for batch publishing.
3. Support multiple named configs across platforms and buckets.
4. Keep secrets encrypted by delegating to the OS keyring.
5. Generate deterministic public URLs from config metadata and object keys.

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

The config file stores storage metadata only. Credentials are stored under the
`filelift` service in the operating system keyring. This gives native encrypted
storage on Windows Credential Manager, macOS Keychain, and Linux Secret Service
where available.

If keyring support fails on a platform, the CLI should return an actionable
error rather than silently writing secrets to disk.

## Command Model

```text
filelift config add <name>
filelift config list
filelift config use <name>
filelift config remove <name>
filelift upload <path>
```

Upload options:

- `--config <name>` overrides the default config.
- `--prefix <path>` chooses the remote key prefix.
- `--recursive` allows directory upload.
- `--name <name>` renames a single uploaded file.
- `--markdown` prints Markdown image links when useful.
- `--dry-run` prints planned keys and URLs without uploading.

## Milestones

1. CLI skeleton and storage config.
2. Keyring-backed credentials.
3. Dry-run path planning and URL generation.
4. S3-compatible upload.
5. Recursive upload reporting.
6. Release packaging for crates.io.
