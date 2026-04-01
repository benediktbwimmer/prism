#!/bin/zsh

set -euo pipefail

ROOT="/Users/bene/code/prism"
CLI_BIN="$ROOT/target/release/prism-cli"
BIN="$ROOT/target/release/prism-mcp"
HTTP_PATH="/mcp"
HEALTH_PATH="/healthz"

resolve_status_path() {
  python3 - "$CLI_BIN" "$ROOT" "$1" <<'PY'
import subprocess
import sys

cli_bin, root, field = sys.argv[1:4]
output = subprocess.check_output(
    [cli_bin, "--root", root, "mcp", "status"],
    text=True,
)
for line in output.splitlines():
    if not line.startswith(field + ": "):
        continue
    value = line.split(": ", 1)[1]
    if field == "log_path":
        value = value.split(" (", 1)[0]
    print(value)
    raise SystemExit(0)
raise SystemExit(1)
PY
}

URI_FILE="$(resolve_status_path uri_file)"
LOG="$(resolve_status_path log_path)"

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
  "$CLI_BIN" --root "$ROOT" mcp restart --no-coordination >/dev/null
fi

if ! is_daemon_ready; then
  echo "prism daemon failed to start; see $LOG" >&2
  exit 1
fi

exec "$BIN" --mode bridge --root "$ROOT" --http-uri-file "$URI_FILE" --no-coordination
