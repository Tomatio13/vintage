/**
 * Agent integration management for the Settings screen.
 *
 * Loads the current hook/plugin install state for each agent CLI from the host
 * and drives install / uninstall actions, refreshing the status after each
 * change. Follows the settings-hook pattern (shells.ts) but keeps no persisted
 * value — the host is the single source of truth for integration state.
 */

import { useCallback, useEffect, useState } from "react";
import { host } from "../host/index.ts";
import type { IntegrationAgent, IntegrationStatus } from "../host/types.ts";

const INTEGRATION_AGENTS: IntegrationAgent[] = ["codex", "claude", "opencode"];

/**
 * Extracts the host error message. Tauri v2 commands that return
 * `Result<T, String>` surface the string directly as the rejected value, so a
 * plain string is shown verbatim; an object with a `message` field is unwrapped
 * too. Anything else falls back to the caller's generic message.
 */
export function describeHostError(error: unknown, fallback: string): string {
  if (typeof error === "string" && error.length > 0) return error;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message: unknown }).message;
    if (typeof message === "string" && message.length > 0) return message;
  }
  return fallback;
}

/** Displays the agent name as it appears to the user. */
export function integrationAgentLabel(agent: IntegrationAgent): string {
  switch (agent) {
    case "codex":
      return "Codex";
    case "claude":
      return "Claude Code";
    case "opencode":
      return "OpenCode";
  }
}

export function useIntegrations() {
  const [integrations, setIntegrations] = useState<IntegrationStatus[] | null>(
    null,
  );
  const [actionError, setActionError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const statuses = await host.integrations.list();
      setIntegrations(statuses);
    } catch {
      setIntegrations(null);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const install = useCallback(
    async (agent: IntegrationAgent) => {
      setActionError(null);
      try {
        await host.integrations.install(agent);
      } catch (error: unknown) {
        setActionError(
          describeHostError(error, "The integration could not be installed."),
        );
      }
      await refresh();
    },
    [refresh],
  );

  const uninstall = useCallback(
    async (agent: IntegrationAgent) => {
      setActionError(null);
      try {
        await host.integrations.uninstall(agent);
      } catch (error: unknown) {
        setActionError(
          describeHostError(error, "The integration could not be removed."),
        );
      }
      await refresh();
    },
    [refresh],
  );

  return {
    integrations,
    actionError,
    install,
    uninstall,
    agents: INTEGRATION_AGENTS,
  };
}
