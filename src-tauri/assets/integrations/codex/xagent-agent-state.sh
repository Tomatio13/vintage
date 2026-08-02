# XAGENT_INTEGRATION_ID=codex
# XAGENT_INTEGRATION_VERSION=1
#!/bin/sh
# Installed by xagent. Reports the native session id to xagent's hook IPC.
# Does nothing when the xagent hook environment is absent.
set -eu

action="${1:-}"
case "$action" in
session) ;;
*) exit 0 ;;
esac

[ "${XAGENT_HOOK_ENV:-}" = "1" ] || exit 0
[ -n "${XAGENT_HOOK_PORT:-}" ] || exit 0
[ -n "${XAGENT_HOOK_TOKEN:-}" ] || exit 0
[ -n "${XAGENT_PANE_ID:-}" ] || exit 0

hook_input_file="$(mktemp "${TMPDIR:-/tmp}/xagent-codex-hook.XXXXXX")" || exit 0
trap 'rm -f "$hook_input_file"' EXIT HUP INT TERM
cat >"$hook_input_file" 2>/dev/null || true

XAGENT_ACTION="$action" XAGENT_HOOK_INPUT_FILE="$hook_input_file" python3 - <<'PY'
import json
import os
import socket

action = os.environ.get("XAGENT_ACTION", "")
pane_id = os.environ.get("XAGENT_PANE_ID", "")
port = os.environ.get("XAGENT_HOOK_PORT", "")
token = os.environ.get("XAGENT_HOOK_TOKEN", "")

if not pane_id or not port or not token:
    raise SystemExit(0)

hook_input = {}
path = os.environ.get("XAGENT_HOOK_INPUT_FILE", "")
if path:
    try:
        with open(path, encoding="utf-8") as handle:
            content = handle.read()
        if content.strip():
            hook_input = json.loads(content)
    except Exception:
        hook_input = {}

if hook_input.get("hook_event_name") != "SessionStart":
    raise SystemExit(0)

session_id = hook_input.get("session_id")
if not isinstance(session_id, str) or not session_id:
    raise SystemExit(0)

report = {
    "paneId": pane_id,
    "generation": int(os.environ.get("XAGENT_GENERATION", "0")),
    "source": "codex-hook",
    "agent": "codex",
    "sessionId": session_id,
    "authToken": token,
}

try:
    with socket.create_connection(("127.0.0.1", int(port)), timeout=0.5) as sock:
        sock.sendall((json.dumps(report) + "\n").encode("utf-8"))
except Exception:
    pass
PY
