#!/usr/bin/env bash
# Initialize/sync uv environment and start the Open WebUI media sidecar.
# Run from anywhere:  ./start-sidecar.sh   or   bash demo/start-sidecar.sh
# Port: OPENCLAW_MEDIA_PORT (default 18790), must match tcli when logging in.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$HERE"

if ! command -v uv >/dev/null 2>&1; then
  echo "error: uv is required — install from https://docs.astral.sh/uv/" >&2
  exit 1
fi

# Create/update .venv and lock resolution from pyproject.toml (no deps: fast).
uv sync

export OPENCLAW_MEDIA_PORT="${OPENCLAW_MEDIA_PORT:-18790}"
exec uv run python openclaw_media_sidecar.py
