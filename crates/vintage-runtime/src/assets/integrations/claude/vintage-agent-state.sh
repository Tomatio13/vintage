# VINTAGE_INTEGRATION_ID=claude
# VINTAGE_INTEGRATION_VERSION=4
# Installed by VINTAGE. Reports the native session id and state to VINTAGE's
# hook IPC. Does nothing when the VINTAGE hook environment is absent. Sends
# over TCP via node (works on any OS with node installed).
set -eu

action="${1:-}"
case "$action" in
session | idle | working | blocked | released) ;;
*) exit 0 ;;
esac

[ "${VINTAGE_HOOK_ENV:-}" = "1" ] || exit 0
[ -n "${VINTAGE_HOOK_PORT:-}" ] || exit 0
[ -n "${VINTAGE_HOOK_TOKEN:-}" ] || exit 0
[ -n "${VINTAGE_PANE_ID:-}" ] || exit 0

hook_input_file="$(mktemp "${TMPDIR:-/tmp}/VINTAGE-claude-hook.XXXXXX")" || exit 0
trap 'rm -f "$hook_input_file"' EXIT HUP INT TERM
cat >"$hook_input_file" 2>/dev/null || true

VINTAGE_ACTION="$action" VINTAGE_HOOK_INPUT_FILE="$hook_input_file" node - <<'JS'
const fs = require("node:fs");
const net = require("node:net");

const paneId = process.env.VINTAGE_PANE_ID;
const port = process.env.VINTAGE_HOOK_PORT;
const token = process.env.VINTAGE_HOOK_TOKEN;
const generation = process.env.VINTAGE_GENERATION;
const agent = process.env.VINTAGE_AGENT || "claude";
const action = process.env.VINTAGE_ACTION || "";
if (!paneId || !port || !token || !generation) process.exit(0);

let hook = {};
const path = process.env.VINTAGE_HOOK_INPUT_FILE;
if (path) {
  try {
    const content = fs.readFileSync(path, "utf8").trim();
    if (content) hook = JSON.parse(content);
  } catch {
    hook = {};
  }
}

// Subagent reports carry an agent_id and must not overwrite the parent pane.
if (hook.agent_id) process.exit(0);

// State actions report the mapped state; only session carries an id.
// released clears the pane's agent, so the agent name is omitted.
let state = null;
let sessionId = null;
let reportAgent = agent;
if (action === "released") {
  state = "released";
  reportAgent = "";
} else if (action === "session") {
  if (hook.hook_event_name !== "SessionStart") process.exit(0);
  sessionId = hook.session_id;
  if (typeof sessionId !== "string" || !sessionId) process.exit(0);
} else {
  state = action;
}

const report = {
  paneId,
  generation: Number(generation) || 0,
  source: `${agent}-hook`,
  ...(reportAgent ? { agent: reportAgent } : {}),
  ...(state ? { state } : {}),
  ...(sessionId ? { sessionId } : {}),
  authToken: token,
};
const payload = JSON.stringify(report) + "\n";

try {
  const client = net.createConnection(
    { host: "127.0.0.1", port: Number(port) },
    () => client.write(payload),
  );
  client.setTimeout(500);
  client.on("error", () => client.destroy());
  client.on("timeout", () => client.destroy());
  client.on("close", () => process.exit(0));
} catch {
  process.exit(0);
}
JS
