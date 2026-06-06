# filelift

`filelift` is a small CLI for uploading local assets to S3-compatible object
storage and printing shareable URLs.

It is designed for writing, publishing, and static-site workflows where images,
videos, and other assets should live outside the source repository.

## Goals

- Upload one file or a directory of files.
- Support Cloudflare R2, AWS S3, MinIO, and other S3-compatible services.
- Manage multiple storage profiles.
- Store secrets through the operating system keyring instead of plain text.
- Print public URLs and Markdown snippets after uploads.

## Planned CLI

```bash
filelift profile add r2-blog \
  --bucket eave-assets \
  --endpoint https://example.r2.cloudflarestorage.com \
  --region auto \
  --public-base-url https://assets.example.com

filelift profile use r2-blog
filelift upload ./cover.webp --prefix blog/2026/my-post --markdown
filelift upload ./assets --recursive --prefix blog/2026/my-post
```

## Configuration Model

Profiles contain non-secret storage metadata:

```toml
default_profile = "r2-blog"

[profiles.r2-blog]
provider = "s3"
bucket = "eave-assets"
endpoint = "https://example.r2.cloudflarestorage.com"
region = "auto"
public_base_url = "https://assets.example.com"
```

Access keys are stored separately in the system keyring:

- service: `filelift`
- account: `<profile-name>:access_key_id`
- account: `<profile-name>:secret_access_key`

## Architecture

- `cli`: command definitions and argument parsing.
- `config`: profile config load/save.
- `secret`: keyring integration.
- `profile`: profile command handlers.
- `storage`: storage provider interface.
- `storage::s3`: S3-compatible upload implementation.
- `output`: URL and Markdown formatting.

## Status

This repository is in early scaffolding. The first implementation milestone is
profile management plus single and recursive uploads to S3-compatible storage.
