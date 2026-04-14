#!/usr/bin/env bash
# Bootstrap installer for curl | bash (same idea as https://tempo.xyz/install).
#
# After pushing to GitHub, users can run (official repo defaults TCLI_RELEASE_REPO):
#   curl -fsSL https://raw.githubusercontent.com/turbo-wang/tcli/main/install.sh | bash
#
# Optional — override if you use a fork or mirror:
#   export TCLI_RELEASE_REPO=owner/repo
#   TCLI_RELEASE_REPO=owner/repo curl -fsSL https://raw.githubusercontent.com/turbo-wang/tcli/main/install.sh | bash
#
# Optional:
#   TCLI_INSTALL_REF=main|master|v0.1.0-branch   (default: main)
#   TCLI_BIN_DIR=~/.local/bin
#   TCLI_NO_MODIFY_PROFILE=1   (do not edit ~/.zshrc, ~/.bashrc, etc.)
#   TCLI_BUILD_FROM_SOURCE=1   (skip GitHub Release; build with cargo — requires Rust)
#
# After install: ~/.tcli/env is written and POSIX/fish shell configs get a PATH hook (tempo-style).
#
# Remote install tries prebuilt binaries first (no Rust). Publish v* releases with workflow
# assets (.github/workflows/release.yml) or pass TCLI_BUILD_FROM_SOURCE=1 to compile.
set -euo pipefail

TCLI_INSTALLER_VERSION="0.2.0"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'
info() { echo -e "${GREEN}info${NC}: $1"; }
warn() { echo -e "${YELLOW}warn${NC}: $1"; }
error() { echo -e "${RED}error${NC}: $1" >&2; exit 1; }

# Local clone: delegate to scripts/install.sh (developer running ./install.sh from repo).
_resolve_here() {
  local s="${BASH_SOURCE[0]:-}"
  if [[ -z "$s" || "$s" == bash ]]; then
    return 1
  fi
  if [[ ! -f "$s" ]]; then
    return 1
  fi
  cd "$(dirname "$s")" && pwd
}

if HERE="$(_resolve_here)" && [[ -n "$HERE" ]] && [[ -f "$HERE/scripts/install.sh" ]] && [[ -f "$HERE/tcli/Cargo.toml" ]]; then
  exec "$HERE/scripts/install.sh" "$@"
fi

# --- Remote: download source tarball from GitHub (no git clone required) ---
# Default release/source repo: https://github.com/turbo-wang/tcli (override with TCLI_RELEASE_REPO for forks)
TCLI_RELEASE_REPO="${TCLI_RELEASE_REPO:-turbo-wang/tcli}"

REF="${TCLI_INSTALL_REF:-main}"
TAR_URL="https://github.com/${TCLI_RELEASE_REPO}/archive/refs/heads/${REF}.tar.gz"

command -v curl >/dev/null 2>&1 || error "curl is required"
command -v tar >/dev/null 2>&1 || error "tar is required"

info "tcli remote installer $TCLI_INSTALLER_VERSION"
info "Fetching $TAR_URL …"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if ! curl -fsSL "$TAR_URL" | tar xz -C "$TMP"; then
  error "Download failed. Check TCLI_RELEASE_REPO and TCLI_INSTALL_REF (branch must exist on GitHub)."
fi

SRC="$(find "$TMP" -maxdepth 1 -mindepth 1 -type d | head -1)"
[[ -d "$SRC/scripts" ]] && [[ -f "$SRC/tcli/Cargo.toml" ]] || error "Unexpected archive layout under $TMP"

# Export TCLI_RELEASE_REPO for nested scripts (already set by caller).
export TCLI_RELEASE_REPO

if [[ "${TCLI_BUILD_FROM_SOURCE:-}" == "1" ]]; then
  info "TCLI_BUILD_FROM_SOURCE=1 — building from source (Rust required)…"
  command -v cargo >/dev/null 2>&1 || error "cargo not found. Install Rust: https://rustup.rs"
  exec bash "$SRC/scripts/install.sh" "$@"
fi

info "Trying prebuilt release (no Rust required)…"
set +e
bash "$SRC/scripts/install.sh" --release "$@"
rc=$?
set -e
if [[ "$rc" -eq 0 ]]; then
  exit 0
fi

warn "Prebuilt install failed (exit $rc). Falling back to source build…"
command -v cargo >/dev/null 2>&1 || error \
  "Need Rust to build from source: https://rustup.rs — or publish a GitHub Release with a matching asset (tcli-<tag>-<target>.tar.gz; see scripts/install.sh --release)."

exec bash "$SRC/scripts/install.sh" "$@"
