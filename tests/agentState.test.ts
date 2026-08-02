import assert from "node:assert/strict";
import test from "node:test";
import {
  ACTIVITY_PRIORITY,
  acknowledgeDone,
  aggregateActivities,
  applyActivityReport,
  applyPtyTransition,
  effectiveActivity,
  initialPaneActivityState,
  maxActivity,
  rollupPaneStatuses,
} from "../src/workspace/agentState.ts";
import type { AgentActivity } from "../src/workspace/types.ts";

const ALL_ACTIVITIES: AgentActivity[] = [
  "unknown",
  "idle",
  "working",
  "blocked",
  "done",
];

test("rollup priority is blocked > working > done > idle > unknown", () => {
  assert.ok(ACTIVITY_PRIORITY.blocked > ACTIVITY_PRIORITY.working);
  assert.ok(ACTIVITY_PRIORITY.working > ACTIVITY_PRIORITY.done);
  assert.ok(ACTIVITY_PRIORITY.done > ACTIVITY_PRIORITY.idle);
  assert.ok(ACTIVITY_PRIORITY.idle > ACTIVITY_PRIORITY.unknown);

  assert.equal(maxActivity("idle", "working"), "working");
  assert.equal(maxActivity("working", "blocked"), "blocked");
  assert.equal(maxActivity("done", "idle"), "done");
  assert.equal(maxActivity("unknown", "done"), "done");
  assert.equal(maxActivity("blocked", "blocked"), "blocked");
});

test("aggregateActivities keeps the most attention-needing state", () => {
  assert.equal(aggregateActivities([]), "unknown");
  assert.equal(aggregateActivities(["idle", "working", "blocked"]), "blocked");
  assert.equal(aggregateActivities(["idle", "done"]), "done");
  assert.equal(aggregateActivities(["idle", "unknown"]), "idle");
  assert.equal(aggregateActivities(["unknown", "unknown"]), "unknown");
  assert.equal(aggregateActivities(ALL_ACTIVITIES), "blocked");
});

test("a hidden pane going working to idle raises a pending done badge", () => {
  let state = initialPaneActivityState("running");
  state = applyActivityReport(state, "working", false);
  state = applyActivityReport(state, "idle", false);
  assert.equal(state.donePending, true);
  assert.equal(state.activity, "idle");
  assert.equal(effectiveActivity(state, false), "done");
});

test("a visible pane going working to idle never raises done", () => {
  let state = initialPaneActivityState("running");
  state = applyActivityReport(state, "working", true);
  state = applyActivityReport(state, "idle", true);
  assert.equal(state.donePending, false);
  assert.equal(effectiveActivity(state, true), "idle");
});

test("done is only generated from a working transition", () => {
  let idle = initialPaneActivityState("running");
  idle = applyActivityReport(idle, "idle", false);
  idle = applyActivityReport(idle, "idle", false);
  assert.equal(idle.donePending, false);

  let blocked = initialPaneActivityState("running");
  blocked = applyActivityReport(blocked, "blocked", false);
  blocked = applyActivityReport(blocked, "idle", false);
  assert.equal(blocked.donePending, false);

  let unknown = initialPaneActivityState("running");
  unknown = applyActivityReport(unknown, "idle", false);
  assert.equal(unknown.donePending, false);
});

test("viewing the pane acknowledges the pending done badge", () => {
  let state = initialPaneActivityState("running");
  state = applyActivityReport(state, "working", false);
  state = applyActivityReport(state, "idle", false);
  assert.equal(effectiveActivity(state, false), "done");

  // The pane becomes visible: the badge shows the real activity again.
  assert.equal(effectiveActivity(state, true), "idle");
  const acknowledged = acknowledgeDone(state);
  assert.equal(acknowledged.donePending, false);
  assert.equal(effectiveActivity(acknowledged, false), "idle");
  // Acknowledging twice is a no-op returning the same record.
  assert.equal(acknowledgeDone(acknowledged), acknowledged);
});

test("new working or blocked reports supersede a pending done", () => {
  let state = initialPaneActivityState("running");
  state = applyActivityReport(state, "working", false);
  state = applyActivityReport(state, "idle", false);
  assert.equal(state.donePending, true);

  state = applyActivityReport(state, "working", false);
  assert.equal(state.donePending, false);

  state = applyActivityReport(state, "idle", false);
  assert.equal(state.donePending, true);
  state = applyActivityReport(state, "blocked", false);
  assert.equal(state.donePending, false);
});

test("a hidden working pane that exits normally raises done", () => {
  let state = initialPaneActivityState("running");
  state = applyActivityReport(state, "working", false);
  state = applyPtyTransition(state, "exited", {
    exitNormal: true,
    visible: false,
  });
  assert.equal(state.ptyState, "exited");
  assert.equal(state.donePending, true);
});

test("abnormal exits and visible panes do not raise done on exit", () => {
  let abnormal = initialPaneActivityState("running");
  abnormal = applyActivityReport(abnormal, "working", false);
  abnormal = applyPtyTransition(abnormal, "exited", {
    exitNormal: false,
    visible: false,
  });
  assert.equal(abnormal.donePending, false);

  let visible = initialPaneActivityState("running");
  visible = applyActivityReport(visible, "working", true);
  visible = applyPtyTransition(visible, "exited", {
    exitNormal: true,
    visible: true,
  });
  assert.equal(visible.donePending, false);

  let neverWorked = initialPaneActivityState("running");
  neverWorked = applyActivityReport(neverWorked, "idle", false);
  neverWorked = applyPtyTransition(neverWorked, "exited", {
    exitNormal: true,
    visible: false,
  });
  assert.equal(neverWorked.donePending, false);
});

test("restarting a pane resets activity to unknown", () => {
  let state = initialPaneActivityState("running");
  state = applyActivityReport(state, "working", false);
  state = applyActivityReport(state, "idle", false);
  assert.equal(state.donePending, true);

  state = applyPtyTransition(state, "starting");
  assert.equal(state.ptyState, "starting");
  assert.equal(state.activity, "unknown");
  assert.equal(state.donePending, false);
});

test("pty errors clear pending done and keep the last activity", () => {
  let state = initialPaneActivityState("running");
  state = applyActivityReport(state, "working", false);
  state = applyActivityReport(state, "idle", false);
  assert.equal(state.donePending, true);

  state = applyPtyTransition(state, "error");
  assert.equal(state.ptyState, "error");
  assert.equal(state.donePending, false);
  assert.equal(state.activity, "idle");
});

test("rollup excludes stopped and exited panes unless done is pending", () => {
  assert.deepEqual(
    rollupPaneStatuses([
      { ptyState: "stopped", activity: "working", donePending: false },
      { ptyState: "exited", activity: "working", donePending: false },
    ]),
    { activity: "unknown", hasPtyError: false },
  );

  assert.deepEqual(
    rollupPaneStatuses([
      { ptyState: "exited", activity: "idle", donePending: true },
      { ptyState: "running", activity: "idle", donePending: false },
    ]),
    { activity: "done", hasPtyError: false },
  );

  assert.deepEqual(
    rollupPaneStatuses([
      { ptyState: "stopped", activity: "idle", donePending: true },
      { ptyState: "running", activity: "working", donePending: false },
    ]),
    { activity: "working", hasPtyError: false },
  );
});

test("rollup treats pty errors as blocked-equivalent with a separate flag", () => {
  assert.deepEqual(
    rollupPaneStatuses([
      { ptyState: "error", activity: "idle", donePending: false },
      { ptyState: "running", activity: "working", donePending: false },
    ]),
    { activity: "blocked", hasPtyError: true },
  );

  assert.deepEqual(
    rollupPaneStatuses([
      { ptyState: "error", activity: "unknown", donePending: false },
    ]),
    { activity: "blocked", hasPtyError: true },
  );
});

test("rollup of running panes follows the attention priority", () => {
  assert.deepEqual(
    rollupPaneStatuses([
      { ptyState: "running", activity: "idle", donePending: false },
      { ptyState: "running", activity: "working", donePending: false },
      { ptyState: "running", activity: "unknown", donePending: false },
    ]),
    { activity: "working", hasPtyError: false },
  );

  assert.deepEqual(rollupPaneStatuses([]), {
    activity: "unknown",
    hasPtyError: false,
  });
});
