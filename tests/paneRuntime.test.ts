import assert from "node:assert/strict";
import test from "node:test";
import {
  createTerminalId,
  paneRuntimeOrStopped,
  removePaneRuntime,
  setPanePtyState,
  startPaneRuntime,
  type PaneRuntimeMap,
} from "../src/workspace/paneRuntime.ts";

test("createTerminalId produces host-safe identifiers", () => {
  const id = createTerminalId();
  assert.match(id, /^terminal-[0-9a-f-]{36}$/);
  assert.notEqual(createTerminalId(), id);
});

test("a pane without runtime state reads as stopped and restartable", () => {
  const runtime = paneRuntimeOrStopped({}, "pane-1");
  assert.equal(runtime.ptyState, "stopped");
  assert.equal(runtime.generation, -1);
  assert.equal(runtime.terminalId, "");
});

test("starting a pane mints a terminal id and generation zero", () => {
  const map = startPaneRuntime({}, "pane-1");
  const runtime = map["pane-1"];
  assert.ok(runtime);
  assert.equal(runtime.ptyState, "starting");
  assert.equal(runtime.generation, 0);
  assert.match(runtime.terminalId, /^terminal-/);
});

test("restarting increments the generation and rotates the terminal id", () => {
  let map: PaneRuntimeMap = startPaneRuntime({}, "pane-1");
  const first = map["pane-1"];
  map = setPanePtyState(map, "pane-1", "running");
  map = startPaneRuntime(map, "pane-1");
  const second = map["pane-1"];
  assert.equal(second.generation, 1);
  assert.notEqual(second.terminalId, first.terminalId);
  assert.equal(second.ptyState, "starting");
});

test("pty state transitions only touch the named pane", () => {
  let map: PaneRuntimeMap = startPaneRuntime({}, "pane-1");
  map = startPaneRuntime(map, "pane-2");
  map = setPanePtyState(map, "pane-1", "exited", "Shell exited.");
  assert.equal(map["pane-1"].ptyState, "exited");
  assert.equal(map["pane-1"].detail, "Shell exited.");
  assert.equal(map["pane-2"].ptyState, "starting");

  const untouched = setPanePtyState(map, "missing", "error");
  assert.equal(untouched, map);
});

test("removing a pane drops its runtime record", () => {
  let map: PaneRuntimeMap = startPaneRuntime({}, "pane-1");
  map = removePaneRuntime(map, "pane-1");
  assert.equal("pane-1" in map, false);
  const same = removePaneRuntime(map, "pane-1");
  assert.equal(same, map);
});
