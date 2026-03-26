#!/bin/zsh

set -euo pipefail

ROOT="/Users/bene/code/prism"
BIN="$ROOT/target/release/prism-mcp"
SOCKET="$ROOT/.prism/prism-mcp.sock"
LOG="$ROOT/.prism/prism-mcp-daemon.log"

mkdir -p "$ROOT/.prism"

is_daemon_ready() {
  python3 - "$SOCKET" <<'PY'
import socket
import sys

path = sys.argv[1]
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.settimeout(0.2)
try:
    sock.connect(path)
except OSError:
    raise SystemExit(1)
else:
    raise SystemExit(0)
finally:
    sock.close()
PY
}

if ! is_daemon_ready; then
  rm -f "$SOCKET"
  /bin/sh -c 'log_path="$1"; shift; exe="$1"; shift; nohup "$exe" "$@" >>"$log_path" 2>&1 </dev/null &' \
    prism-mcp-daemon-launcher \
    "$LOG" \
    "$BIN" \
    --mode daemon \
    --root "$ROOT" \
    --no-coordination

  for _ in {1..200}; do
    if is_daemon_ready; then
      break
    fi
    sleep 0.05
  done
fi

if ! is_daemon_ready; then
  echo "prism daemon failed to start; see $LOG" >&2
  exit 1
fi

exec "$BIN" --mode bridge --root "$ROOT" --no-coordination
