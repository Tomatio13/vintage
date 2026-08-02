/**
 * Ephemeral per-pane PTY runtime state.
 *
 * None of this is persisted: after a restart every restored pane shows as
 * stopped until the user starts it again. Generations guard against stale
 * host events — each (re)start mints a fresh terminal id and generation.
 */

import type { PtyState } from "./types.ts";

export interface PaneRuntime {
  /** Host terminal id of the current generation. */
  terminalId: string;
  /** Increments on every (re)start; events carry their generation. */
  generation: number;
  ptyState: PtyState;
  /** Human-readable detail for the status card (exit code, spawn error). */
  detail: string | null;
}

export type PaneRuntimeMap = Record<string, PaneRuntime>;

export function createTerminalId(): string {
  return `terminal-${crypto.randomUUID()}`;
}

/** A pane with no runtime record is a restored pane: stopped, restartable. */
export function paneRuntimeOrStopped(
  map: PaneRuntimeMap,
  paneId: string,
): PaneRuntime {
  return (
    map[paneId] ?? {
      terminalId: "",
      generation: -1,
      ptyState: "stopped",
      detail: null,
    }
  );
}

/** Starts or restarts a pane: fresh terminal id, next generation. */
export function startPaneRuntime(
  map: PaneRuntimeMap,
  paneId: string,
): PaneRuntimeMap {
  const previous = map[paneId];
  return {
    ...map,
    [paneId]: {
      terminalId: createTerminalId(),
      generation: previous ? previous.generation + 1 : 0,
      ptyState: "starting",
      detail: null,
    },
  };
}

export function setPanePtyState(
  map: PaneRuntimeMap,
  paneId: string,
  ptyState: PtyState,
  detail: string | null = null,
): PaneRuntimeMap {
  const current = map[paneId];
  if (!current) return map;
  return { ...map, [paneId]: { ...current, ptyState, detail } };
}

/** Drops runtime state once the pane definition itself is gone. */
export function removePaneRuntime(
  map: PaneRuntimeMap,
  paneId: string,
): PaneRuntimeMap {
  if (!(paneId in map)) return map;
  const next = { ...map };
  delete next[paneId];
  return next;
}
