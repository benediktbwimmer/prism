#!/bin/zsh

set -euo pipefail

ROOT="/Users/bene/code/prism"
BIN="$ROOT/target/release/prism-mcp"
URI_FILE="$ROOT/.prism/prism-mcp-http-uri"
LOG="$ROOT/.prism/prism-mcp-daemon.log"
HTTP_PATH="/mcp"
HEALTH_PATH="/healthz"

mkdir -p "$ROOT/.prism"

is_daemon_ready() {
  python3 - "$URI_FILE" "$HEALTH_PATH" <<'PY'
import pathlib
import sys
import urllib.error
import urllib.parse
import urllib.request

uri_file = pathlib.Path(sys.argv[1])
health_path = sys.argv[2]
if not uri_file.exists():
    raise SystemExit(1)

uri = uri_file.read_text().strip()
if not uri:
    raise SystemExit(1)

parts = urllib.parse.urlsplit(uri)
health_url = urllib.parse.urlunsplit((parts.scheme, parts.netloc, health_path, "", ""))
try:
    with urllib.request.urlopen(health_url, timeout=0.2) as response:
        raise SystemExit(0 if 200 <= response.status < 300 else 1)
except (OSError, urllib.error.URLError, ValueError):
    raise SystemExit(1)
PY
}

if ! is_daemon_ready; then
  rm -f "$URI_FILE"
  /bin/sh -c 'log_path="$1"; shift; exe="$1"; shift; nohup "$exe" "$@" >>"$log_path" 2>&1 </dev/null &' \
    prism-mcp-daemon-launcher \
    "$LOG" \
    "$BIN" \
    --mode daemon \
    --root "$ROOT" \
    --http-uri-file "$URI_FILE" \
    --http-path "$HTTP_PATH" \
    --health-path "$HEALTH_PATH" \
    --no-coordination

  for _ in {1..1200}; do
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

exec "$BIN" --mode bridge --root "$ROOT" --http-uri-file "$URI_FILE" --no-coordination
