/**
 * Pure agent-activity aggregation.
 *
 * Two independent layers never share a state value:
 * - PTY state (starting / running / stopped / exited / error) tracks the
 *   terminal process.
 * - Agent activity (unknown / idle / working / blocked / done) tracks what
 *   the agent inside the pane is doing.
 *
 * `done` is derived locally, never reported by a hook, plugin, or screen
 * manifest: a hidden pane that was working and reaches idle (or exits
 * normally) raises `done` until the user views it.
 */

import type { AgentActivity, PtyState, ReportedActivity } from "./types.ts";

/**
 * Rollup priority: blocked > working > done > idle > unknown.
 * A workspace or tab shows the state needing the most attention below it.
 */
export const ACTIVITY_PRIORITY: Record<AgentActivity, number> = {
  blocked: 4,
  working: 3,
  done: 2,
  idle: 1,
  unknown: 0,
};

/** Returns whichever activity rolls up higher. Ties keep `a`. */
export function maxActivity(a: AgentActivity, b: AgentActivity): AgentActivity {
  return ACTIVITY_PRIORITY[b] > ACTIVITY_PRIORITY[a] ? b : a;
}

/** Rolls up zero or more activities; an empty set aggregates to unknown. */
export function aggregateActivities(
  activities: Iterable<AgentActivity>,
): AgentActivity {
  let current: AgentActivity = "unknown";
  for (const activity of activities) {
    current = maxActivity(current, activity);
  }
  return current;
}

/**
 * Per-pane activity bookkeeping. Pure callers fold reports and PTY
 * transitions through the functions below; React and the host keep their own
 * copies and never mutate these records in place.
 */
export interface PaneActivityState {
  ptyState: PtyState;
  /** Latest reported activity. Never holds `done`, which is derived. */
  activity: ReportedActivity;
  /** A `done` badge is pending acknowledgement by viewing the pane. */
  donePending: boolean;
}

export function initialPaneActivityState(
  ptyState: PtyState = "starting",
): PaneActivityState {
  return { ptyState, activity: "unknown", donePending: false };
}

/**
 * Folds one authoritative activity report into the pane state.
 *
 * `done` is generated only on a working -> idle transition while the pane is
 * hidden. Any move to working or blocked supersedes a pending `done`.
 */
export function applyActivityReport(
  state: PaneActivityState,
  reported: ReportedActivity,
  visible: boolean,
): PaneActivityState {
  let donePending = state.donePending;
  if (reported === "working" || reported === "blocked") {
    donePending = false;
  } else if (reported === "idle" && state.activity === "working" && !visible) {
    donePending = true;
  }
  return { ...state, activity: reported, donePending };
}

export interface PtyTransitionOptions {
  /** Only meaningful for `exited`: the process exited with a normal status. */
  exitNormal?: boolean;
  /** Whether the pane is on screen at the moment of the transition. */
  visible?: boolean;
}

/**
 * Folds a PTY lifecycle transition into the pane state.
 *
 * - `starting` resets derived activity: a restarted pane begins unknown.
 * - `exited` with a normal status raises `done` when the pane was working
 *   and hidden.
 * - `error` supersedes any pending `done`; the UI shows a separate error
 *   badge and rollups treat the pane as blocked-equivalent.
 */
export function applyPtyTransition(
  state: PaneActivityState,
  ptyState: PtyState,
  options: PtyTransitionOptions = {},
): PaneActivityState {
  const { exitNormal = false, visible = false } = options;
  if (ptyState === "starting") {
    return { ptyState, activity: "unknown", donePending: false };
  }
  if (ptyState === "error") {
    return { ...state, ptyState, donePending: false };
  }
  if (ptyState === "exited") {
    const donePending =
      exitNormal && state.activity === "working" && !visible
        ? true
        : state.donePending;
    return { ...state, ptyState, donePending };
  }
  return { ...state, ptyState };
}

/** The user viewed the pane: a pending `done` badge becomes idle. */
export function acknowledgeDone(state: PaneActivityState): PaneActivityState {
  if (!state.donePending) return state;
  return { ...state, donePending: false };
}

/** The badge a single pane shows: pending `done` only while hidden. */
export function effectiveActivity(
  state: PaneActivityState,
  visible: boolean,
): AgentActivity {
  if (state.donePending && !visible) return "done";
  return state.activity;
}

export interface PaneRollupInput {
  ptyState: PtyState;
  activity: ReportedActivity;
  donePending: boolean;
}

export interface RollupResult {
  activity: AgentActivity;
  /** At least one pane has a PTY error; the UI adds a separate badge. */
  hasPtyError: boolean;
}

/**
 * Rolls panes up to a tab or workspace badge.
 *
 * - PTY `error` counts as blocked-equivalent and sets `hasPtyError`.
 * - `stopped` and `exited` panes drop out of the rollup unless a `done`
 *   badge is still pending, in which case they contribute `done`.
 * - An empty (or fully stopped) set aggregates to `unknown`.
 */
export function rollupPaneStatuses(
  panes: Iterable<PaneRollupInput>,
): RollupResult {
  let hasPtyError = false;
  let activity: AgentActivity = "unknown";
  for (const pane of panes) {
    if (pane.ptyState === "error") {
      hasPtyError = true;
      activity = maxActivity(activity, "blocked");
      continue;
    }
    if (pane.ptyState === "stopped" || pane.ptyState === "exited") {
      if (pane.donePending) activity = maxActivity(activity, "done");
      continue;
    }
    activity = maxActivity(activity, pane.activity);
  }
  return { activity, hasPtyError };
}
