// XAGENT_INTEGRATION_ID=opencode
// XAGENT_INTEGRATION_VERSION=1
// Installed by xagent. Reports lifecycle state and session identity to
// xagent's hook IPC over TCP. Does nothing when the xagent hook environment
// is absent. Reports are serialized so send order is preserved.

import net from "node:net";

const SOURCE = "xagent:opencode";
let sendChain = Promise.resolve();

function hookEnv() {
  if (process.env.XAGENT_HOOK_ENV !== "1") return null;
  const paneId = process.env.XAGENT_PANE_ID;
  const port = process.env.XAGENT_HOOK_PORT;
  const token = process.env.XAGENT_HOOK_TOKEN;
  const generation = process.env.XAGENT_GENERATION;
  if (!paneId || !port || !token || !generation) return null;
  return { paneId, port, token, generation: Number(generation) };
}

function sessionIdFromProperties(properties) {
  return typeof properties?.sessionID === "string" && properties.sessionID
    ? properties.sessionID
    : undefined;
}

function reportState(state, sessionID) {
  const env = hookEnv();
  if (!env) return Promise.resolve();

  const params = {
    paneId: env.paneId,
    generation: env.generation,
    source: "opencode-plugin",
    agent: "opencode",
    ...(state ? { state } : {}),
    ...(sessionID ? { sessionId: sessionID } : {}),
    authToken: env.token,
  };
  const payload = JSON.stringify(params) + "\n";

  sendChain = sendChain.then(
    () =>
      new Promise((resolve) => {
        const client = net.createConnection(
          { host: "127.0.0.1", port: Number(env.port) },
          () => {
            client.write(payload);
          },
        );
        client.setTimeout(500);
        client.on("error", finish);
        client.on("close", finish);
        client.on("timeout", finish);
        function finish() {
          client.destroy();
          resolve();
        }
      }),
  );
  return sendChain;
}

export const XagentAgentStatePlugin = async () => {
  if (!hookEnv()) return {};

  return {
    dispose: async () => {
      await reportState(undefined);
    },
    event: async ({ event }) => {
      const type = event?.type;
      const properties = event?.properties ?? {};
      const sessionID = sessionIDFromProperties(properties);

      switch (type) {
        case "permission.asked":
        case "question.asked":
          await reportState("blocked", sessionID);
          break;
        case "permission.replied": {
          const reply = properties.reply ?? properties.response;
          if (reply === "reject") {
            await reportState("idle", sessionID);
          } else if (reply === "once" || reply === "always") {
            await reportState("working", sessionID);
          }
          break;
        }
        case "question.replied":
          await reportState("working", sessionID);
          break;
        case "question.rejected":
          await reportState("idle", sessionID);
          break;
        case "session.created":
        case "session.updated":
          if (sessionID) {
            await reportState("idle", sessionID);
          }
          break;
        case "session.status": {
          const status =
            typeof properties.status === "string"
              ? properties.status
              : properties.status?.type;
          if (status === "busy" || status === "retry") {
            await reportState("working", sessionID);
          } else if (status === "idle") {
            await reportState("idle", sessionID);
          }
          break;
        }
        case "session.idle":
          await reportState("idle", sessionID);
          break;
        case "session.deleted":
          // Release the plugin's authority over this pane's activity.
          await reportState(undefined, sessionID);
          break;
        default:
          break;
      }
    },
  };
};
