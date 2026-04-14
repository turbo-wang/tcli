#!/usr/bin/env bash
# Build tcli from source (release). Run from any cwd.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/tcli/Cargo.toml"

# shellcheck disable=SC1091
source "$ROOT/scripts/cargo.sh"

if [[ ! -f "$MANIFEST" ]]; then
  echo "error: expected Cargo project at $ROOT/tcli" >&2
  exit 1
fi

echo "info: building tcli (release)…"
(
  cd "$ROOT/tcli"
  cargo_exec build --release "$@"
) || exit $?

TARGET_DIR="$(
  cd "$ROOT/tcli" && cargo_exec metadata --format-version=1 --no-deps 2>/dev/null \
    | python3 -c 'import sys,json; print(json.load(sys.stdin)["target_directory"])' 2>/dev/null \
    || echo "$ROOT/tcli/target"
)"
BIN="${TARGET_DIR}/release/tcli"
if [[ ! -f "$BIN" ]]; then
  echo "error: binary not found at $BIN (check CARGO_TARGET_DIR / cargo metadata)" >&2
  exit 1
fi

echo "info: built $BIN"
if command -v strip >/dev/null 2>&1; then
  strip "$BIN" 2>/dev/null || true
fi
