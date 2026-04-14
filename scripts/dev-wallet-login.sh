#!/usr/bin/env bash
# Start the Python auth mock, then run `tcli wallet login` against it (good for manual self-test).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${MOCK_AUTH_PORT:-8000}"
export MOCK_AUTH_PORT="$PORT"
export MOCK_AUTH_HOST="${MOCK_AUTH_HOST:-127.0.0.1}"
export TCLI_AUTH_BASE="${TCLI_AUTH_BASE:-http://127.0.0.1:${PORT}}"
export NO_PROXY="127.0.0.1,localhost,::1${NO_PROXY:+,${NO_PROXY}}"
export no_proxy="${NO_PROXY}"

python3 "$ROOT/mock_backend/auth_service/main.py" &
PID=$!
cleanup() { kill "$PID" 2>/dev/null || true; }
trap cleanup EXIT

sleep 0.4
if ! kill -0 "$PID" 2>/dev/null; then
  echo "error: mock exited (port ${PORT} in use? try MOCK_AUTH_PORT=18080)" >&2
  exit 1
fi

echo "Mock: ${TCLI_AUTH_BASE}"

run_wallet_login() {
  if [[ -n "${TCLI_BIN:-}" && -x "${TCLI_BIN}" ]]; then
    echo "Using TCLI_BIN=${TCLI_BIN}"
    "${TCLI_BIN}" wallet login "$@"
    return $?
  fi
  local release_bin="$ROOT/tcli/target/release/tcli"
  local debug_bin="$ROOT/tcli/target/debug/tcli"
  if [[ -x "$release_bin" ]]; then
    echo "Using $release_bin (release build)"
    "$release_bin" wallet login "$@"
    return $?
  fi
  if [[ -x "$debug_bin" ]]; then
    echo "Using $debug_bin (debug build)"
    "$debug_bin" wallet login "$@"
    return $?
  fi

  cd "$ROOT/tcli"
  if command -v rustup >/dev/null 2>&1 && rustup run stable cargo --version >/dev/null 2>&1; then
    echo "Running: rustup run stable cargo run -- wallet login"
    rustup run stable cargo run -- wallet login "$@"
    return $?
  fi
  if cargo --version >/dev/null 2>&1; then
    echo "Running: cargo run -- wallet login"
    cargo run -- wallet login "$@"
    return $?
  fi

  echo "error: could not run tcli." >&2
  echo "  Fix rustup:  rustup default stable" >&2
  echo "  Or build once:  (cd tcli && rustup run stable cargo build --release)  then re-run this script." >&2
  exit 1
}

echo ""
run_wallet_login "$@"
