#!/usr/bin/env bash
# Shared by build.sh / install.sh: invoke cargo when rustup has no default, or only nightly, etc.
# shellcheck shell=bash

cargo_exec() {
  if command -v rustup >/dev/null 2>&1; then
    if rustup run stable cargo --version >/dev/null 2>&1; then
      rustup run stable cargo "$@"
      return
    fi
    # Any other installed toolchain (e.g. nightly only)
    local tc
    tc="$(rustup toolchain list 2>/dev/null | awk '/-/{print $1; exit}')"
    if [[ -n "$tc" ]] && rustup run "$tc" cargo --version >/dev/null 2>&1; then
      rustup run "$tc" cargo "$@"
      return
    fi
  fi
  if command -v cargo >/dev/null 2>&1 && cargo --version >/dev/null 2>&1; then
    cargo "$@"
    return
  fi
  echo "error: no usable Rust toolchain (cargo cannot run)." >&2
  echo "error: install one toolchain, e.g.:" >&2
  echo "  rustup default stable" >&2
  echo "error: or see https://rustup.rs" >&2
  return 127
}
