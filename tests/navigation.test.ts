import assert from "node:assert/strict";
import test from "node:test";
import {
  navigatePanes,
  navigateTabs,
  navigateWorkspaces,
  type Direction,
} from "../src/workspace/navigation.ts";
import { singlePaneLayout, splitPane } from "../src/workspace/paneLayout.ts";
import type { AgentTabState, WorkspaceState } from "../src/workspace/types.ts";

function workspaceWithTabs(
  ids: string[],
  selectedTabId: string,
): WorkspaceState {
  return {
    id: "ws-1",
    path: "/tmp/ws-1",
    title: "WS",
    selectedTabId,
    tabs: ids.map((id, index) => ({
      id,
      title: `Tab ${index + 1}`,
      layout: singlePaneLayout(`pane-${id}`),
      selectedPaneId: `pane-${id}`,
      panes: [],
    })),
  };
}

function tabWithPanes(ids: string[], selectedPaneId: string): AgentTabState {
  let layout = singlePaneLayout(ids[0]);
  for (let index = 1; index < ids.length; index += 1) {
    layout =
      splitPane(layout, ids[index - 1], "horizontal", ids[index]) ?? layout;
  }
  return {
    id: "tab-1",
    title: "Tab",
    layout,
    selectedPaneId,
    panes: [],
  };
}

test("navigateTabs moves forward and backward with wrap", () => {
  const workspace = workspaceWithTabs(["a", "b", "c"], "b");
  assert.equal(navigateTabs(workspace, 1), "c");
  assert.equal(navigateTabs(workspace, -1), "a");
  // Wraps at both edges.
  const first = workspaceWithTabs(["a", "b", "c"], "a");
  assert.equal(navigateTabs(first, -1), "c");
  const last = workspaceWithTabs(["a", "b", "c"], "c");
  assert.equal(navigateTabs(last, 1), "a");
});

test("navigateTabs handles empty or stale selection", () => {
  const empty = workspaceWithTabs([], "a");
  assert.equal(navigateTabs(empty, 1), null);
  const stale = workspaceWithTabs(["a", "b"], "missing");
  assert.equal(navigateTabs(stale, -1), null);
});

test("navigateTabs with a single tab stays on it", () => {
  const workspace = workspaceWithTabs(["only"], "only");
  assert.equal(navigateTabs(workspace, 1), "only");
  assert.equal(navigateTabs(workspace, -1), "only");
});

test("navigatePanes moves in document order with wrap", () => {
  const tab = tabWithPanes(["p1", "p2", "p3"], "p2");
  assert.equal(navigatePanes(tab, 1), "p3");
  assert.equal(navigatePanes(tab, -1), "p1");
  const first = tabWithPanes(["p1", "p2", "p3"], "p1");
  assert.equal(navigatePanes(first, -1), "p3");
  const last = tabWithPanes(["p1", "p2", "p3"], "p3");
  assert.equal(navigatePanes(last, 1), "p1");
});

test("navigatePanes handles stale selection", () => {
  const tab = tabWithPanes(["p1", "p2"], "missing");
  assert.equal(navigatePanes(tab, 1), null);
});

test("navigatePanes with a single pane stays on it", () => {
  const tab = tabWithPanes(["only"], "only");
  assert.equal(navigatePanes(tab, 1), "only");
  assert.equal(navigatePanes(tab, -1), "only");
});

test("navigateWorkspaces moves with wrap and handles empty/unknown", () => {
  const workspaces = [
    workspaceWithTabs(["t"], "t"),
    workspaceWithTabs(["t"], "t"),
    workspaceWithTabs(["t"], "t"),
  ];
  workspaces[0].id = "w1";
  workspaces[1].id = "w2";
  workspaces[2].id = "w3";
  assert.equal(navigateWorkspaces(workspaces, "w2", 1), "w3");
  assert.equal(navigateWorkspaces(workspaces, "w2", -1), "w1");
  assert.equal(navigateWorkspaces(workspaces, "w3", 1), "w1");
  assert.equal(navigateWorkspaces(workspaces, "w1", -1), "w3");
  assert.equal(navigateWorkspaces([], "w1", 1), null);
  assert.equal(navigateWorkspaces(workspaces, "missing", 1), null);
});

test("Direction type is exhaustive for both values", () => {
  const directions: Direction[] = [1, -1];
  assert.deepEqual(directions.sort(), [-1, 1]);
});
