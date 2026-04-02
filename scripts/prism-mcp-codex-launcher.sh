#!/bin/zsh

set -euo pipefail

SCRIPT_PATH="${0:A}"
SCRIPT_NAME="${0:t}"
SCRIPT_DIR="${SCRIPT_PATH:h}"

resolve_root() {
  if git -C "${PWD:A}" rev-parse --show-toplevel >/dev/null 2>&1; then
    git -C "${PWD:A}" rev-parse --show-toplevel
    return 0
  fi

  print -r -- "${PWD:A}"
}

ROOT="${PRISM_ROOT_OVERRIDE:-$(resolve_root)}"
LOCAL_SCRIPT="$ROOT/scripts/$SCRIPT_NAME"

if [[ "${PRISM_CODEX_LAUNCHER_REEXEC:-0}" != "1" && -f "$LOCAL_SCRIPT" && "$LOCAL_SCRIPT:A" != "$SCRIPT_PATH" ]]; then
  exec env PRISM_CODEX_LAUNCHER_REEXEC=1 /bin/zsh "$LOCAL_SCRIPT" "$@"
fi

ensure_release_binaries() {
  local cli_bin="$ROOT/target/release/prism-cli"
  local mcp_bin="$ROOT/target/release/prism-mcp"

  if [[ -x "$cli_bin" && -x "$mcp_bin" ]]; then
    return 0
  fi

  echo "Building PRISM release binaries in $ROOT" >&2
  (
    cd "$ROOT"
    cargo build --release -p prism-cli -p prism-mcp
  )
}

ensure_release_binaries

exec "$ROOT/target/release/prism-cli" --root "$ROOT" mcp bridge --internal-developer "$@"
