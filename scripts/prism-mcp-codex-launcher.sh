#!/bin/zsh

set -euo pipefail

SCRIPT_PATH="${0:A}"
SCRIPT_NAME="${0:t}"
SCRIPT_DIR="${SCRIPT_PATH:h}"
SCRIPT_ROOT="${SCRIPT_DIR:h}"

resolve_root() {
  if git -C "${PWD:A}" rev-parse --show-toplevel >/dev/null 2>&1; then
    git -C "${PWD:A}" rev-parse --show-toplevel
    return 0
  fi

  print -r -- "${PWD:A}"
}

ROOT="${PRISM_ROOT_OVERRIDE:-$(resolve_root)}"
LOCAL_SCRIPT="$ROOT/scripts/$SCRIPT_NAME"

BOOTSTRAP_ROOT="${PRISM_CODEX_LAUNCHER_BOOTSTRAP_ROOT:-$SCRIPT_ROOT}"

if [[ "${PRISM_CODEX_LAUNCHER_REEXEC:-0}" != "1" && -f "$LOCAL_SCRIPT" && "$LOCAL_SCRIPT:A" != "$SCRIPT_PATH" ]]; then
  exec env \
    PRISM_CODEX_LAUNCHER_REEXEC=1 \
    PRISM_CODEX_LAUNCHER_BOOTSTRAP_ROOT="$BOOTSTRAP_ROOT" \
    /bin/zsh "$LOCAL_SCRIPT" "$@"
fi

LOCAL_CLI="$ROOT/target/release/prism-cli"
LOCAL_MCP="$ROOT/target/release/prism-mcp"

if [[ -x "$LOCAL_CLI" && -x "$LOCAL_MCP" ]]; then
  exec "$LOCAL_CLI" --root "$ROOT" mcp bridge --internal-developer "$@"
fi

BOOTSTRAP_CLI="$BOOTSTRAP_ROOT/target/release/prism-cli"
if [[ ! -x "$BOOTSTRAP_CLI" ]]; then
  echo "Missing bootstrap PRISM CLI at $BOOTSTRAP_CLI" >&2
  echo "Build the bootstrap checkout once with: cargo build --release -p prism-cli -p prism-mcp" >&2
  exit 1
fi

exec "$BOOTSTRAP_CLI" --root "$ROOT" mcp bridge \
  --internal-developer \
  --bootstrap-build-worktree-release \
  --bridge-daemon-binary "$LOCAL_MCP" \
  "$@"
