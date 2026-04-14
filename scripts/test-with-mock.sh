#!/usr/bin/env bash
# Start Python auth mock, then run `cargo test`. Integration tests read TCLI_TEST_MOCK_BASE.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${MOCK_AUTH_PORT:-18080}"
export MOCK_AUTH_PORT="$PORT"
export MOCK_AUTH_HOST="${MOCK_AUTH_HOST:-127.0.0.1}"
export TCLI_TEST_MOCK_BASE="${TCLI_TEST_MOCK_BASE:-http://127.0.0.1:${PORT}}"
# Avoid corporate proxies breaking localhost in curl / reqwest.
export NO_PROXY="127.0.0.1,localhost,::1${NO_PROXY:+,${NO_PROXY}}"
export no_proxy="${NO_PROXY}"

python3 "$ROOT/mock_backend/auth_service/main.py" &
PID=$!
cleanup() { kill "$PID" 2>/dev/null || true; }
trap cleanup EXIT

sleep 0.3
if ! kill -0 "$PID" 2>/dev/null; then
  echo "error: mock process exited immediately (port ${PORT} in use? try MOCK_AUTH_PORT=)" >&2
  exit 1
fi

echo "Waiting for mock at ${TCLI_TEST_MOCK_BASE} …"
OK=0
for _ in $(seq 1 100); do
  if curl -fsS "${TCLI_TEST_MOCK_BASE}/verify" >/dev/null 2>&1; then
    OK=1
    break
  fi
  sleep 0.05
done
if [[ "$OK" != 1 ]]; then
  echo "error: mock did not become ready at ${TCLI_TEST_MOCK_BASE}" >&2
  exit 1
fi

(cd "$ROOT/tcli" && cargo test "$@" -- --test-threads=1)
