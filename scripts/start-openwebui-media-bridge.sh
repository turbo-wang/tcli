#!/usr/bin/env bash
# Start the Open WebUI media sidecar (demo/openclaw_media_sidecar.py) with uv.
# Default: 127.0.0.1:18790 — override with OPENCLAW_MEDIA_PORT (must match tcli).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT/demo"

if ! command -v uv >/dev/null 2>&1; then
  echo "error: uv is required (https://docs.astral.sh/uv/)" >&2
  exit 1
fi

export OPENCLAW_MEDIA_PORT="${OPENCLAW_MEDIA_PORT:-18790}"
exec uv run python openclaw_media_sidecar.py
