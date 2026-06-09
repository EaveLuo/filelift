# filelift

[![CI](https://github.com/EaveLuo/filelift/actions/workflows/ci.yml/badge.svg)](https://github.com/EaveLuo/filelift/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/EaveLuo/filelift?sort=semver)](https://github.com/EaveLuo/filelift/releases)
[![Crates.io](https://img.shields.io/crates/v/filelift.svg)](https://crates.io/crates/filelift)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

一个小巧、快速的命令行工具，用于把本地文件上传到 S3 兼容对象存储，并打印可分享的 URL。

[English](README.md) | 简体中文

---

`filelift` 面向写作、发布和静态站点工作流——把图片、视频等静态资源放到源码仓库之外。它同时是 **AI 优先（AI-first）** 的：所有命令都可非交互运行，且解析密钥时不依赖操作系统钥匙串（OS keyring），因此在本机、CI、容器以及智能体（agent）沙箱中表现一致。

## 功能特性

- **一条命令，任意 S3** —— 上传单个文件或整个目录（默认递归）到 Cloudflare R2、AWS S3、MinIO、Backblaze B2 等任意 S3 兼容服务。
- **多个命名目标** —— 在一处集中管理各项目的 bucket、endpoint、region 与公开访问基址。
- **沙箱友好的密钥管理** —— 凭证可来自 CLI 参数 / stdin、环境变量，或机器加密的本地文件。无需 OS keyring，也不需要交互式桌面会话。
- **可分享的输出** —— 上传后可打印公开 URL、Markdown 片段或机器可读的 JSON。
- **自更新** —— `filelift upgrade` 原地更新二进制；后台限频检查会在有新版本时提醒你。
- **加密诊断日志** —— 可选的本地加密日志，排障时可导出为 JSONL。

## 安装

macOS 与 Linux：

```bash
curl -fsSL https://raw.githubusercontent.com/EaveLuo/filelift/main/scripts/install.sh | sh
```

Windows PowerShell：

```powershell
irm https://raw.githubusercontent.com/EaveLuo/filelift/main/scripts/install.ps1 | iex
```

安装脚本会下载最新的 GitHub Release 二进制，安装到当前用户目录，并把该目录加入用户 `PATH`：

- macOS / Linux 默认：`~/.local/bin`
- Windows 默认：`%LOCALAPPDATA%\Programs\filelift\bin`

安装指定版本：

```bash
curl -fsSL https://raw.githubusercontent.com/EaveLuo/filelift/main/scripts/install.sh | FILELIFT_VERSION=v0.3.0 sh
```

```powershell
$env:FILELIFT_VERSION = "v0.3.0"; irm https://raw.githubusercontent.com/EaveLuo/filelift/main/scripts/install.ps1 | iex
```

通过 crates.io 安装（需要 Rust 工具链）：

```bash
cargo install filelift
```

## 快速开始

```bash
# 1. 注册一个目标（会提示你输入访问密钥）。
filelift target add r2-blog \
  --bucket eave-assets \
  --endpoint https://example.r2.cloudflarestorage.com \
  --region auto \
  --public-base-url https://assets.example.com \
  --set-default

# 2. 上传一个文件并得到 Markdown 片段。
filelift upload ./cover.webp --folder blog/2026/my-post --markdown

# 3. 上传整个目录（默认递归）。
filelift upload ./assets --folder blog/2026/my-post
```

## 命令一览

| 命令 | 说明 |
| --- | --- |
| `filelift upload <path>...` | 上传一个或多个文件或目录并打印 URL。 |
| `filelift target add <name>` | 注册新的上传目标。 |
| `filelift target update [name]` | 更新已有目标。 |
| `filelift target list` | 列出已配置的目标。 |
| `filelift target use [name]` | 设置默认目标。 |
| `filelift target remove [name]` | 删除目标及其存储的凭证。 |
| `filelift credentials export [name]` | 以环境变量形式打印已存凭证。 |
| `filelift log export` / `log clear` | 导出或清空加密诊断日志。 |
| `filelift language show` / `language use <lang>` | 查看或设置 CLI 语言。 |
| `filelift upgrade`（别名 `update`） | 更新 filelift 到最新版本。 |

常用 `upload` 参数：

- `--target <name>` —— 本次上传覆盖默认目标。
- `--folder <prefix>`（别名 `--prefix`）—— bucket 内的键前缀。
- `--name <name>` —— 覆盖上传对象的名称。
- `--markdown` —— 打印 Markdown 图片 / 链接片段。
- `--output json` —— 输出机器可读 JSON（适合脚本与智能体）。
- `--dry-run` —— 仅规划上传，不实际发送。

运行 `filelift <command> --help` 查看完整参数。

## 凭证与安全模型

目标元数据（bucket、endpoint、region、公开访问基址）属于非敏感信息，存储在 `~/.filelift/targets.toml`。访问密钥则**单独**保存在加密文件 `~/.filelift/secrets.enc`（权限 `0600`）。

需要凭证时，filelift 按以下优先级解析：

1. **显式 CLI 参数 / stdin** —— `--access-key-id`、`--secret-access-key` 或 `--secret-access-key-stdin`。
2. **环境变量** —— 先看目标级 `FILELIFT_<TARGET>_ACCESS_KEY_ID` / `FILELIFT_<TARGET>_SECRET_ACCESS_KEY`，再看全局 `FILELIFT_ACCESS_KEY_ID` / `FILELIFT_SECRET_ACCESS_KEY`。
3. **加密文件存储**。

通过管道传入密钥，避免它出现在 shell 历史或 argv 中：

```bash
echo "$SECRET" | filelift target add r2-blog --bucket ... --access-key-id AKIA... --secret-access-key-stdin
```

为 CI 或智能体沙箱导出已存凭证：

```bash
filelift credentials export r2-blog --format shell   # 或：--format dotenv
eval "$(filelift credentials export r2-blog --format shell)"
```

加密所用的密钥派生自稳定的机器标识（可用 `FILELIFT_MASTER_KEY_HEX` 覆盖）。它可以防止**意外泄露**——提交进仓库、备份、文件拷贝等——但**无法**防御以同一用户身份运行的恶意进程。若需更强隔离，请用你自己的密钥管理系统通过环境变量注入凭证。

### 非交互使用

为 `target add` / `target update` 传入 `--non-interactive`，当缺少必填项时会快速失败而非提示输入，非常适合 CI 与自动化场景。

## 环境变量

| 变量 | 用途 |
| --- | --- |
| `FILELIFT_ACCESS_KEY_ID` / `FILELIFT_SECRET_ACCESS_KEY` | 全局凭证。 |
| `FILELIFT_<TARGET>_ACCESS_KEY_ID` / `FILELIFT_<TARGET>_SECRET_ACCESS_KEY` | 目标级凭证。 |
| `FILELIFT_MASTER_KEY_HEX` | 覆盖密钥存储的加密密钥。 |
| `FILELIFT_VERSION` | 为安装脚本指定版本。 |
| `FILELIFT_INSTALL_DIR` | 覆盖安装目录。 |
| `FILELIFT_NO_UPDATE_CHECK` | 设为 `1` 可禁用更新检查。 |

## 交互模式

不带子命令运行 `filelift` 即可进入交互式 shell：

```text
filelift> target update
```

当命令缺少目标名时，filelift 会在短暂停顿后显示一条暗色提示，且不打断你的输入。按 `Tab` 从已配置的目标中选择，或在目标名仍缺失时按 `Enter` 打开选择器。输入 `exit` 或 `quit` 退出。

## 更新

直接从 CLI 更新（会为你的平台重新运行官方安装脚本）：

```bash
filelift upgrade               # 或：filelift update
filelift upgrade --version v0.3.0
```

filelift 每天最多向 GitHub 检查一次新版本（仅在交互式终端中），有更新时打印一行提示。设置 `FILELIFT_NO_UPDATE_CHECK=1` 可关闭该检查。

### 安装渠道与 PATH 优先级

filelift 有两种安装方式，`filelift upgrade` 会沿用当前运行二进制所属的渠道：

- **预编译（安装脚本）** → 安装到上文的用户目录。`upgrade` 会重新运行安装脚本。
- **`cargo install`** → 安装到 `~/.cargo/bin`。`upgrade` 会运行 `cargo install filelift --force`（Windows 下因正在运行的 `.exe` 无法自我替换，会改为打印该命令让你手动执行）。

如果两种都装了，PATH 中靠前的那个生效（通常是 `~/.cargo/bin`）。当安装脚本发现刚装的版本会被另一份遮蔽时会给出警告。用 `where filelift`（Windows）或 `which -a filelift`（Unix）可查看当前生效的是哪一份。

## 诊断日志

filelift 可将加密诊断日志写入 `~/.filelift/logs/events.log.enc`。日志加密密钥保存在加密的密钥存储中。排障时可导出为可读的 JSONL：

```bash
filelift log export --output filelift-debug-log.jsonl
filelift log clear
```

分享导出日志前请先检查内容；密钥会被脱敏，且不会被有意写入日志。

## 架构

- `cli` —— 命令定义与参数解析。
- `target` / `target_command` —— 目标存储与目标命令处理。
- `secret` —— 加密的本地密钥存储与凭证解析。
- `secret_file` —— 加固（`0600`）的密钥文件读写辅助。
- `machine_key` —— 密钥存储加密密钥的派生。
- `credentials_command` —— `credentials export` 处理。
- `upload` —— 上传规划与执行。
- `storage` / `storage::s3` —— 存储提供方接口与 S3 实现。
- `diagnostic_log` / `log_command` —— 加密诊断日志与导出。
- `update_check` / `upgrade_command` —— 版本检查与自更新。
- `output` —— URL 与 Markdown 格式化。

## 贡献

欢迎提交 Issue 和 PR。在提交 PR 前，请先运行：

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

## 许可证

基于 [MIT 许可证](LICENSE) 授权。
