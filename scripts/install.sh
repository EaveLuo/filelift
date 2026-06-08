#!/usr/bin/env sh
set -eu

REPO="EaveLuo/filelift"
VERSION="${FILELIFT_VERSION:-latest}"
INSTALL_DIR="${FILELIFT_INSTALL_DIR:-$HOME/.local/bin}"
BINARY_NAME="filelift"

say() {
  printf '%s\n' "$1"
}

fail() {
  say "filelift install failed: $1" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

download() {
  url="$1"
  output="$2"

  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$output"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$output"
  else
    fail "missing curl or wget"
  fi
}

fetch_text() {
  url="$1"

  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O -
  else
    fail "missing curl or wget"
  fi
}

resolve_tag() {
  if [ "$VERSION" = "latest" ]; then
    fetch_text "https://api.github.com/repos/$REPO/releases/latest" |
      sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' |
      head -n 1
    return
  fi

  case "$VERSION" in
    v*) printf '%s\n' "$VERSION" ;;
    *) printf 'v%s\n' "$VERSION" ;;
  esac
}

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin)
      case "$arch" in
        x86_64) printf '%s\n' "x86_64-apple-darwin" ;;
        arm64|aarch64) printf '%s\n' "aarch64-apple-darwin" ;;
        *) fail "unsupported macOS architecture: $arch" ;;
      esac
      ;;
    Linux)
      case "$arch" in
        x86_64|amd64) printf '%s\n' "x86_64-unknown-linux-gnu" ;;
        *) fail "unsupported Linux architecture: $arch" ;;
      esac
      ;;
    *)
      fail "unsupported operating system: $os"
      ;;
  esac
}

path_contains() {
  case ":$PATH:" in
    *":$INSTALL_DIR:"*) return 0 ;;
    *) return 1 ;;
  esac
}

profile_has_path() {
  profile="$1"
  [ -f "$profile" ] && grep -F "$INSTALL_DIR" "$profile" >/dev/null 2>&1
}

add_path_to_profile() {
  profile="$1"

  if profile_has_path "$profile"; then
    return
  fi

  {
    printf '\n# filelift\n'
    printf 'case ":$PATH:" in\n'
    printf '  *":%s:"*) ;;\n' "$INSTALL_DIR"
    printf '  *) export PATH="%s:$PATH" ;;\n' "$INSTALL_DIR"
    printf 'esac\n'
  } >> "$profile"
}

need_cmd tar
need_cmd sed
need_cmd head
need_cmd mktemp
need_cmd find

TAG="$(resolve_tag)"
[ -n "$TAG" ] || fail "could not resolve latest release tag"

TARGET="$(detect_target)"
ASSET="filelift-$TARGET.tar.gz"
URL="https://github.com/$REPO/releases/download/$TAG/$ASSET"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

say "Installing or updating filelift $TAG for $TARGET"
download "$URL" "$TMP_DIR/$ASSET"
tar -xzf "$TMP_DIR/$ASSET" -C "$TMP_DIR"

FOUND_BINARY="$(find "$TMP_DIR" -type f -name "$BINARY_NAME" | head -n 1)"
[ -n "$FOUND_BINARY" ] || fail "release asset did not contain $BINARY_NAME"

mkdir -p "$INSTALL_DIR"
cp -f "$FOUND_BINARY" "$INSTALL_DIR/$BINARY_NAME"
chmod +x "$INSTALL_DIR/$BINARY_NAME"

add_path_to_profile "$HOME/.profile"
add_path_to_profile "$HOME/.zshrc"

say "Installed to $INSTALL_DIR/$BINARY_NAME"

# Warn if another filelift earlier on PATH (for example a `cargo install` copy in
# ~/.cargo/bin) will shadow the binary we just installed, so the user is not
# surprised by an unchanged version.
RESOLVED="$(command -v filelift 2>/dev/null || true)"
if [ -n "$RESOLVED" ] && [ "$RESOLVED" != "$INSTALL_DIR/$BINARY_NAME" ]; then
  say "Warning: another filelift is earlier on your PATH and will be used instead of this install:" >&2
  say "  in use:    $RESOLVED" >&2
  say "  installed: $INSTALL_DIR/$BINARY_NAME" >&2
  case "$RESOLVED" in
    *"/.cargo/bin/"*)
      say "  That copy was installed with cargo. Upgrade it with: cargo install filelift --force" >&2
      ;;
    *)
      say "  Remove it or reorder your PATH so $INSTALL_DIR comes first." >&2
      ;;
  esac
fi

if path_contains; then
  "$INSTALL_DIR/$BINARY_NAME" --version
else
  say "Added $INSTALL_DIR to your shell profile. Open a new terminal, then run:"
  say "  filelift --version"
fi
