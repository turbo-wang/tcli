#!/usr/bin/env bash
# TCLI install — inspired by tempo's tempoup: build from source and install into a bin dir.
# Usage:
#   ./scripts/install.sh
#   TCLI_BIN_DIR=/usr/local/bin ./scripts/install.sh
# Optional GitHub release install (when you publish assets):
#   TCLI_RELEASE_REPO=owner/repo TCLI_VERSION=v0.1.0 ./scripts/install.sh --release
set -euo pipefail

TCLIUP_INSTALLER_VERSION="0.1.0"
REPO="${TCLI_RELEASE_REPO:-}"
BIN_DIR="${TCLI_BIN_DIR:-$HOME/.tcli/bin}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { echo -e "${GREEN}info${NC}: $1"; }
warn() { echo -e "${YELLOW}warn${NC}: $1"; }
error() { echo -e "${RED}error${NC}: $1" >&2; exit 1; }

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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
  TCLI_BIN_DIR      Install directory (default: \$HOME/.tcli/bin)
  TCLI_RELEASE_REPO GitHub repo for --release, e.g. turbo-wang/tcli
  GITHUB_TOKEN      Optional token for private repos / rate limits

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
  local url="https://api.github.com/repos/$REPO/releases/latest"
  local tag json
  if [[ -n "${GITHUB_TOKEN:-}" ]]; then
    json="$(curl -fsSL -H "Authorization: token $GITHUB_TOKEN" "$url")"
  else
    json="$(curl -fsSL "$url")"
  fi
  tag="$(echo "$json" | grep '"tag_name":' | head -n1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
  [[ -n "$tag" ]] || error "Could not resolve latest release tag for $REPO"
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
}

install_from_source() {
  command -v cargo >/dev/null 2>&1 || error "cargo not found. Install Rust: https://rustup.rs"
  info "Installing tcli from source…"
  "$ROOT/scripts/build.sh"
  local target_dir built
  target_dir="$(
    cd "$ROOT/tcli" && cargo metadata --format-version=1 --no-deps 2>/dev/null \
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

main() {
  info "tcli installer $TCLIUP_INSTALLER_VERSION"
  if [[ "$FROM_RELEASE" -eq 1 ]]; then
    install_from_release
  else
    install_from_source
  fi

  echo ""
  if [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    warn "$BIN_DIR is not in your PATH"
    echo "  export PATH=\"$BIN_DIR:\$PATH\""
    echo ""
  else
    info "Run: tcli --help"
  fi
}

main
