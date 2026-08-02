import assert from "node:assert/strict";
import test from "node:test";
import {
  buildLogicalLines,
  cleanLogicalLine,
  detectScreenActivity,
  matchManifest,
  MAX_SNAPSHOT_LINES,
  MANIFESTS,
  regionLines,
  stripAnsi,
  type ScreenSnapshot,
} from "../src/workspace/screenDetection.ts";

function snapshot(
  lines: string[],
  title: string | null = null,
  progress: string | null = null,
): ScreenSnapshot {
  return { lines, title, progress };
}

// --- Snapshot building ----------------------------------------------------

test("stripAnsi removes CSI and OSC sequences", () => {
  assert.equal(stripAnsi("[31mred[0m"), "red");
  // OSC 0 title terminated by BEL.
  assert.equal(stripAnsi("]0;my titlerest"), "rest");
});

test("cleanLogicalLine strips trailing whitespace and CR", () => {
  assert.equal(cleanLogicalLine("  hello  "), "  hello");
  assert.equal(cleanLogicalLine("hello\r\n"), "hello");
});

test("buildLogicalLines joins wrapped rows at newlines", () => {
  const rows = [
    { text: "abc", newline: true },
    { text: "def", newline: false },
    { text: "ghi", newline: true },
    { text: "tail", newline: false },
  ];
  const lines = buildLogicalLines(rows);
  assert.deepEqual(lines, ["abc", "defghi", "tail"]);
});

test("buildLogicalLines caps at the snapshot limit", () => {
  const rows = Array.from({ length: MAX_SNAPSHOT_LINES + 5 }, (_, i) => ({
    text: `line-${i}`,
    newline: true,
  }));
  const lines = buildLogicalLines(rows);
  assert.equal(lines.length, MAX_SNAPSHOT_LINES);
  assert.equal(lines[0], `line-${MAX_SNAPSHOT_LINES + 5 - MAX_SNAPSHOT_LINES}`);
});

// --- Region extraction ----------------------------------------------------

test("regionLines selects bottom/top N non-empty lines", () => {
  const s = snapshot(["a", "b", "c", "d"]);
  assert.deepEqual(regionLines(s, { kind: "bottom_lines", n: 2 }), ["c", "d"]);
  assert.deepEqual(regionLines(s, { kind: "top_lines", n: 1 }), ["a"]);
  assert.deepEqual(regionLines(s, { kind: "whole_recent" }), [
    "a",
    "b",
    "c",
    "d",
  ]);
});

test("regionLines finds text after the last prompt marker", () => {
  const s = snapshot(["❯ prompt", "working…", "❯ next"]);
  assert.deepEqual(regionLines(s, { kind: "after_last_prompt_marker" }), [
    "❯ next",
  ]);
});

test("regionLines finds text after the last horizontal rule", () => {
  const s = snapshot(["header", "-----", "content", "-----", "result"]);
  assert.deepEqual(regionLines(s, { kind: "after_last_horizontal_rule" }), [
    "result",
  ]);
});

// --- Codex manifest -------------------------------------------------------

test("codex: live strong blocker wins over weaker rules", () => {
  const s = snapshot([
    "Some context",
    "  press enter to confirm or esc to cancel",
  ]);
  const match = matchManifest(MANIFESTS.codex, s);
  assert.equal(match?.state, "blocked");
  assert.equal(match?.ruleId, "live_strong_blocker");
});

test("codex: working fallback matches the esc-to-interrupt line", () => {
  const s = snapshot(["• Working (Use esc to interrupt)", "…"]);
  const match = matchManifest(MANIFESTS.codex, s);
  assert.equal(match?.state, "working");
});

test("codex: transcript viewer does not author a state", () => {
  const s = snapshot([
    "↑/↓ to scroll · pgup/pgdn to · q to quit",
    "esc to edit prev",
  ]);
  const match = matchManifest(MANIFESTS.codex, s);
  assert.equal(match, null);
});

// --- OpenCode manifest ----------------------------------------------------

test("opencode: permission required is blocked", () => {
  const s = snapshot([
    "△ Permission required",
    "The agent wants to run a command.",
  ]);
  const match = matchManifest(MANIFESTS.opencode, s);
  assert.equal(match?.state, "blocked");
});

test("opencode: esc-to-interrupt hint is working", () => {
  const s = snapshot(["…", "press esc to interrupt"]);
  const match = matchManifest(MANIFESTS.opencode, s);
  assert.equal(match?.state, "working");
});

test("opencode: progress bar is working", () => {
  const s = snapshot(["■■■■■■ 42%"]);
  const match = matchManifest(MANIFESTS.opencode, s);
  assert.equal(match?.state, "working");
});

// --- Grok manifest --------------------------------------------------------

test("grok: action-required title is blocked", () => {
  const s = snapshot([], "grok ⚠ Action Required", null);
  const match = matchManifest(MANIFESTS.grok, s);
  assert.equal(match?.state, "blocked");
});

test("grok: idle title resolves to idle", () => {
  const s = snapshot([], "my-project - grok", "4;0;0");
  const match = matchManifest(MANIFESTS.grok, s);
  assert.equal(match?.state, "idle");
});

test("grok: option dialog is blocked", () => {
  const s = snapshot([
    "  ┃  2 (○) Yes, proceed",
    "  ┃  z (○) Type your answer here",
  ]);
  const match = matchManifest(MANIFESTS.grok, s);
  assert.equal(match?.state, "blocked");
});

test("grok: working spinner with [stop] chip", () => {
  const s = snapshot(["⠧ Waiting on subagent… 2.8s   13s ⇣29.7k [stop]"]);
  const match = matchManifest(MANIFESTS.grok, s);
  assert.equal(match?.state, "working");
});

// --- Claude manifest ------------------------------------------------------

test("claude: bash permission prompt is blocked", () => {
  const s = snapshot(["Do you want to proceed?", "  1. Yes", "  2. No"]);
  const match = matchManifest(MANIFESTS.claude, s);
  assert.equal(match?.state, "blocked");
});

test("claude: live prompt box is idle", () => {
  const s = snapshot(["   ❯", "Some previous output"]);
  const match = matchManifest(MANIFESTS.claude, s);
  assert.equal(match?.state, "idle");
});

test("claude: no matching rule falls back to unknown", () => {
  const s = snapshot(["nothing interesting here"]);
  const match = matchManifest(MANIFESTS.claude, s);
  assert.equal(match, null);
});

// --- detectScreenActivity -------------------------------------------------

test("detectScreenActivity routes to the agent manifest", () => {
  assert.equal(
    detectScreenActivity(
      "codex",
      snapshot(["• Working (Use esc to interrupt)"]),
    )?.state,
    "working",
  );
  assert.equal(
    detectScreenActivity("opencode", snapshot(["press esc to interrupt"]))
      ?.state,
    "working",
  );
  assert.equal(detectScreenActivity("unknown-agent", snapshot(["x"])), null);
});
