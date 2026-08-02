/**
 * Pure keyboard navigation across the workspace → tab → pane hierarchy.
 *
 * These helpers are DOM- and React-free so they can be unit tested with the
 * Node test runner and shared by the renderer. All moves wrap around: the
 * first item follows the last and vice versa.
 */

import { listPaneIds } from "./paneLayout.ts";
import type { AgentTabState, WorkspaceState } from "./types.ts";

/** +1 moves forward, -1 moves backward. */
export type Direction = 1 | -1;

function cyclicIndex(
  length: number,
  current: number,
  direction: Direction,
): number {
  return (current + direction + length) % length;
}

/**
 * The id of the tab before/after the workspace's selected tab, or null when
 * the workspace has no tabs or its selection is stale.
 */
export function navigateTabs(
  workspace: WorkspaceState,
  direction: Direction,
): string | null {
  if (workspace.tabs.length === 0) return null;
  const index = workspace.tabs.findIndex(
    (tab) => tab.id === workspace.selectedTabId,
  );
  if (index < 0) return null;
  return (
    workspace.tabs[cyclicIndex(workspace.tabs.length, index, direction)]?.id ??
    null
  );
}

/**
 * The id of the pane before/after the tab's selected pane, in document order
 * (first subtree before second). Returns null when the tab has no panes or
 * its selection is stale.
 */
export function navigatePanes(
  tab: AgentTabState,
  direction: Direction,
): string | null {
  const ids = listPaneIds(tab.layout);
  if (ids.length === 0) return null;
  const index = ids.indexOf(tab.selectedPaneId);
  if (index < 0) return null;
  return ids[cyclicIndex(ids.length, index, direction)] ?? null;
}

/**
 * The id of the workspace before/after the given one, or null when there is
 * no workspace or the current id is not present.
 */
export function navigateWorkspaces(
  workspaces: WorkspaceState[],
  currentId: string,
  direction: Direction,
): string | null {
  if (workspaces.length === 0) return null;
  const index = workspaces.findIndex((workspace) => workspace.id === currentId);
  if (index < 0) return null;
  return (
    workspaces[cyclicIndex(workspaces.length, index, direction)]?.id ?? null
  );
}
