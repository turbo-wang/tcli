#!/usr/bin/env bash
# TCLI install — inspired by tempo's tempoup: build from source and install into a bin dir.
# Usage:
#   ./scripts/install.sh
#   TCLI_BIN_DIR=/usr/local/bin ./scripts/install.sh
# Optional GitHub release install (when you publish assets):
#   TCLI_RELEASE_REPO=owner/repo TCLI_VERSION=v0.1.0 ./scripts/install.sh --release
set -euo pipefail

TCLIUP_INSTALLER_VERSION="0.1.2"
REPO="${TCLI_RELEASE_REPO:-}"
TCLI_DIR="${TCLI_DIR:-$HOME/.tcli}"
BIN_DIR="${TCLI_BIN_DIR:-$TCLI_DIR/bin}"
ENV_FILE="$TCLI_DIR/env"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { echo -e "${GREEN}info${NC}: $1"; }
warn() { echo -e "${YELLOW}warn${NC}: $1"; }
error() { echo -e "${RED}error${NC}: $1" >&2; exit 1; }

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck disable=SC1091
source "$ROOT/scripts/cargo.sh"

FROM_RELEASE=0
VERSION=""

usage() {
  cat <<EOF
Usage: install.sh [OPTIONS]

Install the tcli binary (default: build from source with cargo, copy to bin dir).

Options:
  -h, --help       Show this help
  -v, --version    Print installer script version
  -r, --release    Download prebuilt archive from GitHub releases (needs TCLI_RELEASE_REPO)
  -i, --install    Version tag to install with --release (e.g. v0.1.0); default: latest

Environment:
  TCLI_DIR              Config/state directory (default: \$HOME/.tcli); holds env file for PATH
  TCLI_BIN_DIR          Install directory (default: \$TCLI_DIR/bin)
  TCLI_RELEASE_REPO     GitHub repo for --release, e.g. turbo-wang/tcli
  TCLI_NO_MODIFY_PROFILE  Set to 1 to skip editing ~/.zshrc, ~/.bashrc, etc.
  GITHUB_TOKEN          Optional token for private repos / rate limits

Examples:
  ./scripts/install.sh
  TCLI_BIN_DIR=~/.local/bin ./scripts/install.sh
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    -v|--version) echo "tcli install script $TCLIUP_INSTALLER_VERSION"; exit 0 ;;
    -r|--release) FROM_RELEASE=1; shift ;;
    -i|--install)
      [[ $# -ge 2 ]] || error "--install requires a version tag (e.g. v0.1.0)"
      VERSION="$2"
      shift 2
      ;;
    *) error "Unknown option: $1. Use --help." ;;
  esac
done

detect_platform() {
  local p
  p="$(uname -s | tr '[:upper:]' '[:lower:]')"
  case "$p" in
    linux*) echo linux ;;
    darwin*) echo darwin ;;
    mingw*|msys*|cygwin*) echo win32 ;;
    *) error "Unsupported platform: $p" ;;
  esac
}

detect_arch() {
  local a
  a="$(uname -m)"
  case "$a" in
    x86_64|x64|amd64) echo amd64 ;;
    arm64|aarch64) echo arm64 ;;
    *) warn "Unknown arch $a; defaulting to amd64"; echo amd64 ;;
  esac
}

detect_target() {
  local platform="$1" arch="$2"
  case "$platform" in
    darwin)
      case "$arch" in
        arm64) echo aarch64-apple-darwin ;;
        amd64) echo x86_64-apple-darwin ;;
        *) error "Unsupported Darwin arch: $arch" ;;
      esac
      ;;
    linux)
      case "$arch" in
        arm64) echo aarch64-unknown-linux-gnu ;;
        amd64) echo x86_64-unknown-linux-gnu ;;
        *) error "Unsupported Linux arch: $arch" ;;
      esac
      ;;
    win32)
      case "$arch" in
        arm64) echo aarch64-pc-windows-msvc ;;
        amd64) echo x86_64-pc-windows-msvc ;;
        *) error "Unsupported Windows arch: $arch" ;;
      esac
      ;;
    *) error "Unsupported platform: $platform" ;;
  esac
}

get_latest_tag() {
  [[ -n "$REPO" ]] || error "Set TCLI_RELEASE_REPO=owner/repo for --release installs."
  command -v curl >/dev/null 2>&1 || error "curl required for release install"
  local url json tag code tmp
  tmp="$(mktemp)"
  url="https://api.github.com/repos/$REPO/releases/latest"
  if [[ -n "${GITHUB_TOKEN:-}" ]]; then
    code="$(curl -sS -o "$tmp" -w "%{http_code}" -H "Authorization: token $GITHUB_TOKEN" "$url")"
  else
    code="$(curl -sS -o "$tmp" -w "%{http_code}" "$url")"
  fi
  if [[ "$code" == "200" ]]; then
    json="$(cat "$tmp")"
  else
    url="https://api.github.com/repos/$REPO/releases?per_page=20"
    if [[ -n "${GITHUB_TOKEN:-}" ]]; then
      code="$(curl -sS -o "$tmp" -w "%{http_code}" -H "Authorization: token $GITHUB_TOKEN" "$url")"
    else
      code="$(curl -sS -o "$tmp" -w "%{http_code}" "$url")"
    fi
    [[ "$code" == "200" ]] || error "GitHub API error ($code) for $REPO releases. Try GITHUB_TOKEN if rate-limited."
    json="$(cat "$tmp")"
  fi
  rm -f "$tmp"
  tag="$(echo "$json" | grep '"tag_name":' | head -n1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
  [[ -n "$tag" ]] || error "Could not resolve a release tag for $REPO"
  echo "$tag"
}

install_from_release() {
  [[ -n "$REPO" ]] || error "Set TCLI_RELEASE_REPO (e.g. export TCLI_RELEASE_REPO=org/repo)"
  command -v curl >/dev/null 2>&1 || error "curl required"
  local platform arch target tag ver
  platform="$(detect_platform)"
  arch="$(detect_arch)"
  target="$(detect_target "$platform" "$arch")"
  if [[ -z "$VERSION" ]]; then
    tag="$(get_latest_tag)"
  else
    tag="$VERSION"
  fi
  ver="${tag#v}"

  # Convention: asset name tcli-${tag}-${target}.tar.gz (adjust when you publish)
  local asset="tcli-${tag}-${target}.tar.gz"
  local base="https://github.com/$REPO/releases/download/${tag}"
  local url="$base/$asset"

  info "Downloading $url …"
  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  if ! curl -fsSL "$url" -o "$tmp/$asset"; then
    error "Download failed. Publish release assets as $asset or build from source without --release."
  fi

  tar -xzf "$tmp/$asset" -C "$tmp"
  local bin_path
  bin_path="$(find "$tmp" -type f \( -name tcli -o -name tcli.exe \) | head -n1)"
  [[ -n "$bin_path" ]] || error "Could not find tcli binary in archive"
  mkdir -p "$BIN_DIR"
  local out="$BIN_DIR/tcli"
  if [[ "$platform" == win32 ]]; then out="$BIN_DIR/tcli.exe"; fi
  cp -f "$bin_path" "$out"
  chmod 755 "$out" 2>/dev/null || true
  info "Installed $out ($tag)"
  # Clear EXIT trap before returning: `tmp` is local; the trap would run at
  # script exit when `tmp` is out of scope and fail under `set -u`.
  rm -rf "$tmp"
  trap - EXIT
}

install_from_source() {
  info "Installing tcli from source…"
  "$ROOT/scripts/build.sh"
  local target_dir built
  target_dir="$(
    cd "$ROOT/tcli" && cargo_exec metadata --format-version=1 --no-deps 2>/dev/null \
      | python3 -c 'import sys,json; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null \
      || echo "$ROOT/tcli/target"
  )"
  built="${target_dir}/release/tcli"
  [[ -f "$built" ]] || error "build did not produce $built"
  mkdir -p "$BIN_DIR"
  local dest="$BIN_DIR/tcli"
  info "Copying to $dest …"
  cp -f "$built" "$dest"
  chmod 755 "$dest"
  info "Installed $dest"
}

write_env_files() {
  mkdir -p "$TCLI_DIR"
  cat > "$ENV_FILE" <<EOF
# tcli shell setup (added by install.sh)
export PATH="$BIN_DIR:\$PATH"
EOF
  cat > "$ENV_FILE.fish" <<EOF
# tcli shell setup (added by install.sh)
fish_add_path -g '$BIN_DIR'
EOF
}

configure_shell() {
  local source_line=". \"$ENV_FILE\""
  local shell_configs=()
  if [[ -n "${ZDOTDIR:-}" ]]; then
    shell_configs+=("$ZDOTDIR/.zshenv")
  fi
  if [[ -f "$HOME/.zshenv" ]] || [[ "$(basename "${SHELL:-}")" == "zsh" ]]; then
    shell_configs+=("$HOME/.zshenv")
  fi
  if [[ -f "$HOME/.bashrc" ]] || [[ "$(basename "${SHELL:-}")" == "bash" ]]; then
    shell_configs+=("$HOME/.bashrc")
  fi
  if [[ -f "$HOME/.bash_profile" ]]; then
    shell_configs+=("$HOME/.bash_profile")
  fi
  if [[ -f "$HOME/.profile" ]]; then
    shell_configs+=("$HOME/.profile")
  fi
  local unique_configs=() seen=""
  for cfg in "${shell_configs[@]}"; do
    case "$seen" in
      *"|$cfg|"*) ;;
      *) seen="$seen|$cfg|"; unique_configs+=("$cfg") ;;
    esac
  done
  local modified=0
  for cfg in "${unique_configs[@]}"; do
    if [[ -f "$cfg" ]] && grep -qF "$ENV_FILE" "$cfg" 2>/dev/null; then
      continue
    fi
    echo >> "$cfg"
    echo "# Added by tcli installer" >> "$cfg"
    echo "$source_line" >> "$cfg"
    info "Added tcli to PATH in $cfg"
    modified=1
  done
  local fish_config="${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/tcli.fish"
  if [[ -d "$(dirname "$fish_config")" ]] || [[ "$(basename "${SHELL:-}")" == "fish" ]]; then
    if [[ ! -f "$fish_config" ]] || ! grep -qF "$ENV_FILE.fish" "$fish_config" 2>/dev/null; then
      mkdir -p "$(dirname "$fish_config")"
      echo "# Added by tcli installer" > "$fish_config"
      echo "source $ENV_FILE.fish" >> "$fish_config"
      info "Added tcli to PATH in $fish_config"
      modified=1
    fi
  fi
  if [[ $modified -eq 0 ]]; then
    info "tcli PATH is already configured in your shell startup files (or run again after creating a shell config)"
  fi
}

main() {
  info "tcli installer $TCLIUP_INSTALLER_VERSION"
  mkdir -p "$TCLI_DIR"
  if [[ "$FROM_RELEASE" -eq 1 ]]; then
    install_from_release
  else
    install_from_source
  fi

  write_env_files
  export PATH="$BIN_DIR:$PATH"

  if [[ "${TCLI_NO_MODIFY_PROFILE:-}" != "1" ]]; then
    configure_shell
  fi

  echo ""
  if command -v tcli >/dev/null 2>&1; then
    info "tcli is on PATH in this session. Try: tcli --help"
  else
    warn "Could not find tcli on PATH in this session."
  fi
  if [[ "$(basename "${SHELL:-}")" == "fish" ]]; then
    info "New terminals: restart the terminal or run: source $ENV_FILE.fish"
  else
    info "New terminals: restart the terminal or run: source $ENV_FILE"
  fi
}

main
