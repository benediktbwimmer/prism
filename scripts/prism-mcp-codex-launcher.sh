#!/bin/zsh

set -euo pipefail

SCRIPT_PATH="${0:A}"
SCRIPT_NAME="${0:t}"
SCRIPT_DIR="${SCRIPT_PATH:h}"
SCRIPT_ROOT="${SCRIPT_DIR:h}"
BOOTSTRAP_ROOT="${PRISM_CODEX_LAUNCHER_BOOTSTRAP_ROOT:-$SCRIPT_ROOT}"

git_root_for() {
  local repo_path="$1"
  if git -C "$repo_path" rev-parse --show-toplevel >/dev/null 2>&1; then
    git -C "$repo_path" rev-parse --show-toplevel
    return 0
  fi
  return 1
}

git_common_dir_for() {
  local repo_path="$1"
  git -C "$repo_path" rev-parse --path-format=absolute --git-common-dir 2>/dev/null
}

same_repo_as_bootstrap() {
  local candidate="$1"
  local bootstrap_common
  local candidate_common
  bootstrap_common="$(git_common_dir_for "$BOOTSTRAP_ROOT")" || return 1
  candidate_common="$(git_common_dir_for "$candidate")" || return 1
  [[ "$bootstrap_common" == "$candidate_common" ]]
}

codex_active_workspace_root() {
  local state_path="${PRISM_CODEX_GLOBAL_STATE_PATH_OVERRIDE:-$HOME/.codex/.codex-global-state.json}"
  [[ -f "$state_path" ]] || return 1

  python3 - "$state_path" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
try:
    obj = json.loads(path.read_text())
except Exception:
    sys.exit(1)
for value in obj.get("active-workspace-roots", []):
    if isinstance(value, str) and value:
        print(value)
PY
}

resolve_root() {
  local cwd_root
  local candidate

  cwd_root="$(git_root_for "${PWD:A}")" || true
  if [[ -n "${cwd_root:-}" && "$cwd_root" != "/" ]]; then
    print -r -- "$cwd_root"
    return 0
  fi

  while IFS= read -r candidate; do
    [[ -n "$candidate" ]] || continue
    cwd_root="$(git_root_for "$candidate")" || continue
    if [[ "$cwd_root" != "/" ]] && same_repo_as_bootstrap "$cwd_root"; then
      print -r -- "$cwd_root"
      return 0
    fi
  done < <(codex_active_workspace_root || true)

  print -r -- "$BOOTSTRAP_ROOT"
}

ROOT="${PRISM_ROOT_OVERRIDE:-$(resolve_root)}"
LOCAL_SCRIPT="$ROOT/scripts/$SCRIPT_NAME"

if [[ "${PRISM_CODEX_LAUNCHER_REEXEC:-0}" != "1" && -f "$LOCAL_SCRIPT" && "$LOCAL_SCRIPT:A" != "$SCRIPT_PATH" ]]; then
  exec env \
    PRISM_CODEX_LAUNCHER_REEXEC=1 \
    PRISM_CODEX_LAUNCHER_BOOTSTRAP_ROOT="$BOOTSTRAP_ROOT" \
    /bin/zsh "$LOCAL_SCRIPT" "$@"
fi

LOCAL_CLI="$ROOT/target/release/prism-cli"
LOCAL_MCP="$ROOT/target/release/prism-mcp"
CLI_EXEC="${PRISM_CODEX_LAUNCHER_CLI_OVERRIDE:-}"

if [[ -z "$CLI_EXEC" && -x "$LOCAL_CLI" && -x "$LOCAL_MCP" ]]; then
  exec "$LOCAL_CLI" --root "$ROOT" mcp bridge \
    --internal-developer \
    --runtime-mode coordination_only \
    "$@"
fi

BOOTSTRAP_CLI="$BOOTSTRAP_ROOT/target/release/prism-cli"
if [[ -z "$CLI_EXEC" && ! -x "$BOOTSTRAP_CLI" ]]; then
  echo "Missing bootstrap PRISM CLI at $BOOTSTRAP_CLI" >&2
  echo "Build the bootstrap checkout once with: cargo build --release -p prism-cli -p prism-mcp" >&2
  exit 1
fi

CLI_EXEC="${CLI_EXEC:-$BOOTSTRAP_CLI}"

exec "$CLI_EXEC" --root "$ROOT" mcp bridge \
  --internal-developer \
  --runtime-mode coordination_only \
  --bootstrap-build-worktree-release \
  --bridge-daemon-binary "$LOCAL_MCP" \
  "$@"
