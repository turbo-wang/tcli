#!/usr/bin/env bash
# 调用已实现的 tcli 能力联调真实后台（不重复拼 URL / curl）。
#
# 文档: agentic-mpp-requirements-and-api.md — POST ${BASE}/api/v1/agentic/mpp/pay
# Base 与登录同源: ~/.tcli/config.toml 的 [auth].base 或环境变量 TCLI_AUTH_BASE
#
# 用法:
#   ./scripts/test-payment.sh pay --amount 1.0
#       → 执行: tcli agentic-mpp pay --amount 1.0
#
#   ./scripts/test-payment.sh request -- 'https://会402+Payment的真实URL'
#       → 执行: tcli request <url>（402 后走 agentic/mpp/pay + Credential 重试）
#
#   ./scripts/test-payment.sh --help
#
#   ./scripts/test-payment.sh selftest
#       自检：解析到的 tcli、`agentic-mpp pay --help`、以及 `cargo test -q`（tcli crate）
#
# 可选: TCLI_BIN、TCLI_HOME、VERBOSE=1（传给 request 子命令时加 -v）

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# 未设置 TCLI_BIN：优先本仓库通过 `cargo metadata` 得到的 target 目录（与 Cursor/沙箱 CARGO_TARGET_DIR 一致），再回退到源码树 tcli/target
resolve_tcli_built() {
  local td
  td="$(cd "${REPO_ROOT}/tcli" 2>/dev/null && cargo metadata --format-version 1 --no-deps 2>/dev/null \
    | python3 -c "import sys,json; print(json.load(sys.stdin)['target_directory'])" 2>/dev/null)" || true
  if [[ -n "${td}" ]]; then
    if [[ -x "${td}/debug/tcli" ]]; then
      printf '%s' "${td}/debug/tcli"
      return
    fi
    if [[ -x "${td}/release/tcli" ]]; then
      printf '%s' "${td}/release/tcli"
      return
    fi
  fi
  if [[ -x "${REPO_ROOT}/tcli/target/debug/tcli" ]]; then
    printf '%s' "${REPO_ROOT}/tcli/target/debug/tcli"
    return
  fi
  if [[ -x "${REPO_ROOT}/tcli/target/release/tcli" ]]; then
    printf '%s' "${REPO_ROOT}/tcli/target/release/tcli"
    return
  fi
  printf ''
}

if [[ -z "${TCLI_BIN:-}" ]]; then
  TCLI_BIN="$(resolve_tcli_built)"
  if [[ -z "${TCLI_BIN}" ]]; then
    TCLI_BIN="tcli"
  fi
fi
if [[ ! -x "${TCLI_BIN}" ]] && ! command -v "${TCLI_BIN}" >/dev/null 2>&1; then
  echo "找不到可执行的 tcli: ${TCLI_BIN}（请 cd tcli && cargo build 或设置 TCLI_BIN）" >&2
  exit 1
fi

if [[ "${1:-}" == "selftest" ]]; then
  echo "== selftest: TCLI_BIN = ${TCLI_BIN}"
  "${TCLI_BIN}" --version
  echo "== agentic-mpp pay --help (head) =="
  "${TCLI_BIN}" agentic-mpp pay --help 2>&1 | head -30
  echo "== cargo test -q (tcli) =="
  (cd "${REPO_ROOT}/tcli" && cargo test -q)
  echo "== selftest OK =="
  exit 0
fi

usage() {
  sed -n '1,35p' "$0"
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

sub="${1:?子命令: pay | request}"
shift

case "${sub}" in
  pay)
    exec "${TCLI_BIN}" agentic-mpp pay "$@"
    ;;
  request)
    req=(request)
    if [[ -n "${VERBOSE:-}" ]]; then
      req+=(-v)
    fi
    if [[ "$#" -lt 1 ]]; then
      echo "用法: $0 request <URL>" >&2
      echo "示例: $0 request 'https://你的域/受保护资源'" >&2
      exit 1
    fi
    req+=("$@")
    exec "${TCLI_BIN}" "${req[@]}"
    ;;
  *)
    echo "未知子命令: ${sub}（使用 pay | request | selftest）" >&2
    usage
    exit 1
    ;;
esac
