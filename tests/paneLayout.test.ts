import assert from "node:assert/strict";
import test from "node:test";
import {
  clampSplitRatio,
  closePane,
  containsPane,
  isValidPaneId,
  listPaneIds,
  resizeSplit,
  resizeSplitAtPath,
  singlePaneLayout,
  splitDepth,
  splitPane,
  validatePaneLayout,
} from "../src/workspace/paneLayout.ts";
import { LAYOUT_LIMITS, type PaneLayout } from "../src/workspace/types.ts";

/** Builds a right-leaning chain of splits whose deepest leaf sits at `depth`. */
function chainOfDepth(depth: number): PaneLayout {
  let node: PaneLayout = { type: "leaf", paneId: "deepest" };
  for (let i = 0; i < depth - 1; i += 1) {
    node = {
      type: "split",
      direction: "horizontal",
      ratio: 0.5,
      first: { type: "leaf", paneId: `side-${i}` },
      second: node,
    };
  }
  return node;
}

/** Builds a balanced tree of `count` leaves (count must be a power of two). */
function balancedTree(count: number): PaneLayout {
  const ids = Array.from({ length: count }, (_, index) => `pane-${index}`);
  function build(slice: string[]): PaneLayout {
    if (slice.length === 1) return { type: "leaf", paneId: slice[0] };
    const mid = Math.floor(slice.length / 2);
    return {
      type: "split",
      direction: "vertical",
      ratio: 0.5,
      first: build(slice.slice(0, mid)),
      second: build(slice.slice(mid)),
    };
  }
  return build(ids);
}

test("splitPane creates horizontal and vertical splits recursively", () => {
  const root = singlePaneLayout("a");
  const two = splitPane(root, "a", "horizontal", "b");
  assert.deepEqual(two, {
    type: "split",
    direction: "horizontal",
    ratio: 0.5,
    first: { type: "leaf", paneId: "a" },
    second: { type: "leaf", paneId: "b" },
  });

  const three = splitPane(two, "b", "vertical", "c");
  assert.ok(three);
  assert.equal(listPaneIds(three).join(","), "a,b,c");
  assert.equal(splitDepth(three), 3);

  const four = splitPane(three, "a", "horizontal", "d", 0.4);
  assert.ok(four);
  assert.equal(listPaneIds(four).sort().join(","), "a,b,c,d");
  assert.equal(splitDepth(four), 3);
});

test("splitPane keeps the existing pane first and the new pane second", () => {
  const layout = splitPane(singlePaneLayout("old"), "old", "vertical", "new");
  assert.ok(layout && layout.type === "split");
  assert.deepEqual(layout.first, { type: "leaf", paneId: "old" });
  assert.deepEqual(layout.second, { type: "leaf", paneId: "new" });
});

test("splitPane clamps the ratio into the 0.2 to 0.8 range", () => {
  const low = splitPane(singlePaneLayout("a"), "a", "horizontal", "b", 0.05);
  const high = splitPane(singlePaneLayout("a"), "a", "horizontal", "b", 0.95);
  assert.ok(low && low.type === "split");
  assert.ok(high && high.type === "split");
  assert.equal(low.ratio, LAYOUT_LIMITS.minSplitRatio);
  assert.equal(high.ratio, LAYOUT_LIMITS.maxSplitRatio);
  assert.equal(clampSplitRatio(0.5), 0.5);
  assert.equal(clampSplitRatio(0.1), LAYOUT_LIMITS.minSplitRatio);
  assert.equal(clampSplitRatio(0.9), LAYOUT_LIMITS.maxSplitRatio);
});

test("splitPane rejects invalid operations without touching the tree", () => {
  const root = singlePaneLayout("a");
  assert.equal(splitPane(root, "missing", "horizontal", "b"), null);
  assert.equal(splitPane(root, "a", "horizontal", "a"), null);
  assert.equal(splitPane(root, "a", "horizontal", ""), null);
  assert.equal(splitPane(root, "a", "horizontal", "bad id"), null);
  assert.equal(splitPane(root, "a", "horizontal", "b", Number.NaN), null);

  const two = splitPane(root, "a", "horizontal", "b");
  assert.equal(splitPane(two, "a", "horizontal", "b"), null);
  assert.deepEqual(two && listPaneIds(two), ["a", "b"]);
});

test("splitPane refuses to exceed the per-tab pane limit", () => {
  let layout: PaneLayout | null = singlePaneLayout("pane-0");
  for (let i = 1; i < LAYOUT_LIMITS.maxPanesPerTab; i += 1) {
    layout = splitPane(layout, "pane-0", "horizontal", `pane-${i}`);
    assert.ok(layout, `split ${i} should succeed`);
  }
  assert.equal(listPaneIds(layout).length, LAYOUT_LIMITS.maxPanesPerTab);
  assert.equal(splitPane(layout, "pane-0", "horizontal", "overflow"), null);
});

test("closePane collapses the parent split onto the surviving sibling", () => {
  const two = splitPane(singlePaneLayout("a"), "a", "horizontal", "b");
  assert.ok(two);
  assert.deepEqual(closePane(two, "b"), { type: "leaf", paneId: "a" });
  assert.deepEqual(closePane(two, "a"), { type: "leaf", paneId: "b" });
});

test("closePane keeps the rest of a nested tree intact", () => {
  const three = splitPane(
    splitPane(singlePaneLayout("a"), "a", "horizontal", "b"),
    "b",
    "vertical",
    "c",
  );
  assert.ok(three);
  const closed = closePane(three, "b");
  assert.ok(closed && closed.type === "split");
  assert.equal(closed.direction, "horizontal");
  assert.deepEqual(closed.first, { type: "leaf", paneId: "a" });
  assert.deepEqual(closed.second, { type: "leaf", paneId: "c" });
  assert.equal(listPaneIds(closed).join(","), "a,c");
});

test("closePane returns null for the last pane and for unknown panes", () => {
  assert.equal(closePane(singlePaneLayout("a"), "a"), null);
  assert.equal(closePane(singlePaneLayout("a"), "zzz"), null);
  assert.equal(closePane(singlePaneLayout("a"), ""), null);
});

test("resizeSplit adjusts only the nearest ancestor split with the direction", () => {
  // horizontal(a, vertical(b, c)) — pane b has both a vertical and a
  // horizontal divider above it.
  const layout = splitPane(
    splitPane(singlePaneLayout("a"), "a", "horizontal", "b"),
    "b",
    "vertical",
    "c",
  );
  assert.ok(layout && layout.type === "split");

  const vertical = resizeSplit(layout, "b", "vertical", 0.7);
  assert.ok(vertical && vertical.type === "split");
  assert.equal(
    vertical.ratio,
    0.5,
    "outer horizontal split must keep its ratio",
  );
  assert.ok(vertical.second.type === "split");
  assert.equal(vertical.second.ratio, 0.7);

  const horizontal = resizeSplit(layout, "b", "horizontal", 0.3);
  assert.ok(horizontal && horizontal.type === "split");
  assert.equal(horizontal.ratio, 0.3);
  assert.ok(horizontal.second.type === "split");
  assert.equal(
    horizontal.second.ratio,
    0.5,
    "inner vertical split must keep its ratio",
  );
});

test("resizeSplit clamps ratios and rejects invalid targets", () => {
  const two = splitPane(singlePaneLayout("a"), "a", "horizontal", "b");
  assert.ok(two);
  const clamped = resizeSplit(two, "b", "horizontal", 5);
  assert.ok(clamped && clamped.type === "split");
  assert.equal(clamped.ratio, LAYOUT_LIMITS.maxSplitRatio);

  assert.equal(
    resizeSplit(two, "b", "vertical", 0.5),
    null,
    "no matching direction",
  );
  assert.equal(
    resizeSplit(singlePaneLayout("a"), "a", "horizontal", 0.5),
    null,
  );
  assert.equal(resizeSplit(two, "missing", "horizontal", 0.5), null);
  assert.equal(resizeSplit(two, "b", "horizontal", Number.NaN), null);
});

test("resizeSplitAtPath targets the exact divider under same-direction nesting", () => {
  // horizontal(A, horizontal(B, C)): two horizontal dividers exist.
  let layout = splitPane(singlePaneLayout("a"), "a", "horizontal", "b");
  layout = splitPane(layout, "b", "horizontal", "c");
  assert.ok(layout && layout.type === "split");

  // Path [] = outer divider (between A and [B|C]).
  const outer = resizeSplitAtPath(layout, [], 0.7);
  assert.ok(outer && outer.type === "split");
  assert.equal(outer.ratio, 0.7);
  assert.ok(outer.second.type === "split");
  assert.equal(outer.second.ratio, 0.5, "inner divider untouched");

  // Path ["second"] = inner divider (between B and C).
  const inner = resizeSplitAtPath(layout, ["second"], 0.3);
  assert.ok(inner && inner.type === "split");
  assert.equal(inner.ratio, 0.5, "outer divider untouched");
  assert.ok(inner.second.type === "split");
  assert.equal(inner.second.ratio, 0.3);

  // Clamping and invalid paths.
  const clamped = resizeSplitAtPath(layout, [], 9);
  assert.ok(clamped && clamped.type === "split");
  assert.equal(clamped.ratio, LAYOUT_LIMITS.maxSplitRatio);
  assert.equal(resizeSplitAtPath(layout, ["first"], 0.5), null, "leaf address");
  assert.equal(
    resizeSplitAtPath(layout, ["second", "first"], 0.5),
    null,
    "past a leaf",
  );
  assert.equal(resizeSplitAtPath(singlePaneLayout("a"), [], 0.5), null);
  assert.equal(resizeSplitAtPath(layout, [], Number.NaN), null);
});

test("containsPane and listPaneIds agree on tree membership", () => {
  const layout = balancedTree(4);
  assert.equal(listPaneIds(layout).length, 4);
  assert.ok(containsPane(layout, "pane-3"));
  assert.equal(containsPane(layout, "nope"), false);
});

test("validatePaneLayout accepts well-formed trees", () => {
  assert.deepEqual(validatePaneLayout(singlePaneLayout("a")), { ok: true });
  assert.deepEqual(validatePaneLayout(balancedTree(8)), { ok: true });
  assert.deepEqual(
    validatePaneLayout(chainOfDepth(LAYOUT_LIMITS.maxSplitDepth)),
    {
      ok: true,
    },
  );
});

test("validatePaneLayout rejects duplicate pane ids", () => {
  const layout = {
    type: "split",
    direction: "horizontal",
    ratio: 0.5,
    first: { type: "leaf", paneId: "same" },
    second: { type: "leaf", paneId: "same" },
  };
  const result = validatePaneLayout(layout);
  assert.equal(result.ok, false);
  if (!result.ok) {
    assert.ok(
      result.issues.some((issue) => issue.code === "duplicate_pane_id"),
    );
  }
});

test("validatePaneLayout rejects invalid pane ids", () => {
  for (const paneId of ["", "  ", "a b", "a\nb", " x", "x".repeat(129)]) {
    const result = validatePaneLayout({ type: "leaf", paneId });
    assert.equal(
      result.ok,
      false,
      `id ${JSON.stringify(paneId)} must be rejected`,
    );
    if (!result.ok) {
      assert.ok(
        result.issues.some((issue) => issue.code === "invalid_pane_id"),
      );
    }
  }
});

test("isValidPaneId bounds ids used by split operations", () => {
  assert.ok(isValidPaneId("pane-1.uuid"));
  assert.equal(isValidPaneId("x".repeat(129)), false);
  assert.equal(isValidPaneId(7), false);
  assert.equal(isValidPaneId(null), false);
});

test("validatePaneLayout rejects out-of-range ratios", () => {
  for (const ratio of [0.1, 0.9, Number.NaN, "0.5", null]) {
    const result = validatePaneLayout({
      type: "split",
      direction: "horizontal",
      ratio,
      first: { type: "leaf", paneId: "a" },
      second: { type: "leaf", paneId: "b" },
    });
    assert.equal(result.ok, false, `ratio ${String(ratio)} must be rejected`);
    if (!result.ok) {
      assert.ok(result.issues.some((issue) => issue.code === "invalid_ratio"));
    }
  }
});

test("validatePaneLayout rejects trees deeper than the limit", () => {
  const result = validatePaneLayout(
    chainOfDepth(LAYOUT_LIMITS.maxSplitDepth + 1),
  );
  assert.equal(result.ok, false);
  if (!result.ok) {
    assert.ok(result.issues.some((issue) => issue.code === "tree_too_deep"));
  }
});

test("validatePaneLayout rejects too many panes per tab", () => {
  // 128 leaves stay shallow (depth 8) so only the pane-count limit trips.
  const layout = balancedTree(128);
  const result = validatePaneLayout(layout);
  assert.equal(result.ok, false);
  if (!result.ok) {
    assert.ok(result.issues.some((issue) => issue.code === "too_many_panes"));
    assert.ok(
      result.issues.every((issue) => issue.code !== "tree_too_deep"),
      "depth limit must not be the reason",
    );
  }
});

test("validatePaneLayout rejects malformed structures", () => {
  const cases: unknown[] = [
    null,
    42,
    "leaf",
    [],
    {},
    { type: "branch", first: null, second: null },
    { type: "leaf" },
    {
      type: "split",
      direction: "diagonal",
      ratio: 0.5,
      first: { type: "leaf", paneId: "a" },
      second: { type: "leaf", paneId: "b" },
    },
    {
      type: "split",
      direction: "horizontal",
      ratio: 0.5,
      first: { type: "leaf", paneId: "a" },
    },
  ];
  for (const value of cases) {
    const result = validatePaneLayout(value);
    assert.equal(
      result.ok,
      false,
      `value ${JSON.stringify(value)} must be rejected`,
    );
  }
});

test("validatePaneLayout detects cycles without recursing forever", () => {
  type Mutable = {
    type: string;
    direction?: string;
    ratio?: number;
    first?: Mutable | null;
    second?: Mutable | null;
  };
  const cycle: Mutable = { type: "split", direction: "horizontal", ratio: 0.5 };
  cycle.first = cycle;
  cycle.second = { type: "leaf", paneId: "a" };
  const result = validatePaneLayout(cycle);
  assert.equal(result.ok, false);
  if (!result.ok) {
    assert.ok(result.issues.some((issue) => issue.code === "cycle_detected"));
  }
});
