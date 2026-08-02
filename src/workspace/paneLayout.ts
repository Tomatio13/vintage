/**
 * Pure split-tree operations for terminal panes.
 *
 * All functions are total and side-effect free: they return a new tree (or
 * null when the operation is invalid) so React and the host can share the
 * same semantics without importing each other.
 */

import {
  LAYOUT_LIMITS,
  type PaneLayout,
  type SplitDirection,
} from "./types.ts";

export type LayoutIssueCode =
  | "invalid_structure"
  | "invalid_pane_id"
  | "duplicate_pane_id"
  | "invalid_ratio"
  | "tree_too_deep"
  | "too_many_panes"
  | "cycle_detected";

export interface LayoutIssue {
  code: LayoutIssueCode;
  message: string;
}

export type LayoutValidationResult =
  { ok: true } | { ok: false; issues: LayoutIssue[] };

/** Pane ids are opaque but must stay printable and bounded. */
const PANE_ID_PATTERN = /^[^\s\x00-\x1f\x7f]{1,128}$/u;

export function isValidPaneId(value: unknown): value is string {
  return (
    typeof value === "string" &&
    PANE_ID_PATTERN.test(value) &&
    value.length <= LAYOUT_LIMITS.maxIdLength
  );
}

export function singlePaneLayout(paneId: string): PaneLayout {
  return { type: "leaf", paneId };
}

export function clampSplitRatio(ratio: number): number {
  return Math.min(
    LAYOUT_LIMITS.maxSplitRatio,
    Math.max(LAYOUT_LIMITS.minSplitRatio, ratio),
  );
}

/** Returns the deepest split level reached by the tree; a lone leaf is level 1. */
export function splitDepth(layout: PaneLayout): number {
  return depthOf(layout, 1, new Set());
}

function depthOf(node: PaneLayout, level: number, seen: Set<object>): number {
  if (node.type === "leaf") return level;
  if (seen.has(node)) return level;
  seen.add(node);
  return Math.max(
    depthOf(node.first, level + 1, seen),
    depthOf(node.second, level + 1, seen),
  );
}

/** Pane ids in document order (first subtree before second subtree). */
export function listPaneIds(layout: PaneLayout): string[] {
  const ids: string[] = [];
  collectPaneIds(layout, ids, new Set());
  return ids;
}

function collectPaneIds(
  node: PaneLayout,
  ids: string[],
  seen: Set<object>,
): void {
  if (node.type === "leaf") {
    ids.push(node.paneId);
    return;
  }
  if (seen.has(node)) return;
  seen.add(node);
  collectPaneIds(node.first, ids, seen);
  collectPaneIds(node.second, ids, seen);
}

export function containsPane(layout: PaneLayout, paneId: string): boolean {
  return listPaneIds(layout).includes(paneId);
}

/**
 * Replaces `paneId` with a split that keeps the existing pane as `first` and
 * appends `newPaneId` as `second`. Returns null when the target pane is
 * missing, either id is invalid, the new id collides, or the pane limit
 * would be exceeded.
 */
export function splitPane(
  layout: PaneLayout,
  paneId: string,
  direction: SplitDirection,
  newPaneId: string,
  ratio = 0.5,
): PaneLayout | null {
  if (!isValidPaneId(paneId) || !isValidPaneId(newPaneId)) return null;
  if (paneId === newPaneId) return null;
  if (!Number.isFinite(ratio)) return null;
  if (containsPane(layout, newPaneId)) return null;
  if (listPaneIds(layout).length >= LAYOUT_LIMITS.maxPanesPerTab) return null;
  const clamped = clampSplitRatio(ratio);
  return replacePaneNode(layout, paneId, (leaf) => ({
    type: "split",
    direction,
    ratio: clamped,
    first: leaf,
    second: { type: "leaf", paneId: newPaneId },
  }));
}

/**
 * Removes `paneId`, collapsing its parent split onto the surviving sibling.
 * Returns null when the pane is missing or when it is the last pane of the
 * tree (closing it removes the whole tab, which the caller decides).
 */
export function closePane(
  layout: PaneLayout,
  paneId: string,
): PaneLayout | null {
  if (!isValidPaneId(paneId)) return null;
  if (!containsPane(layout, paneId)) return null;
  return removePaneNode(layout, paneId);
}

/**
 * Adjusts the divider ratio of the nearest ancestor split of `paneId` whose
 * direction matches. The ratio is clamped to the allowed range. Returns null
 * when the pane is missing, the ratio is not finite, or no matching split
 * exists (a lone pane has no divider).
 */
export function resizeSplit(
  layout: PaneLayout,
  paneId: string,
  direction: SplitDirection,
  ratio: number,
): PaneLayout | null {
  if (!isValidPaneId(paneId)) return null;
  if (!Number.isFinite(ratio)) return null;
  if (!containsPane(layout, paneId)) return null;
  const clamped = clampSplitRatio(ratio);
  const result = resizeOnPath(layout, paneId, direction, clamped);
  if (!result.found || !result.applied) return null;
  return result.node;
}

interface ResizeOutcome {
  node: PaneLayout;
  /** The target pane is inside this subtree. */
  found: boolean;
  /** A split on the path to the pane already received the new ratio. */
  applied: boolean;
}

function resizeOnPath(
  node: PaneLayout,
  paneId: string,
  direction: SplitDirection,
  ratio: number,
): ResizeOutcome {
  if (node.type === "leaf") {
    return { node, found: node.paneId === paneId, applied: false };
  }
  const first = resizeOnPath(node.first, paneId, direction, ratio);
  if (first.found) {
    const applyHere = !first.applied && node.direction === direction;
    return {
      node: {
        ...node,
        ratio: applyHere ? ratio : node.ratio,
        first: first.node,
      },
      found: true,
      applied: first.applied || applyHere,
    };
  }
  const second = resizeOnPath(node.second, paneId, direction, ratio);
  if (second.found) {
    const applyHere = !second.applied && node.direction === direction;
    return {
      node: {
        ...node,
        ratio: applyHere ? ratio : node.ratio,
        second: second.node,
      },
      found: true,
      applied: second.applied || applyHere,
    };
  }
  return { node, found: false, applied: false };
}

/**
 * Structural address of a split node: the child choices taken from the root.
 * Divider dragging uses paths because same-direction nesting makes leaf
 * based addressing ambiguous; `resizeSplit` (nearest matching ancestor)
 * stays the keyboard-resize semantics.
 */
export type SplitPath = readonly ("first" | "second")[];

/**
 * Sets the ratio of the split node addressed by `path` (clamped). Returns
 * null when the path is invalid or does not land on a split node.
 */
export function resizeSplitAtPath(
  layout: PaneLayout,
  path: SplitPath,
  ratio: number,
): PaneLayout | null {
  if (!Number.isFinite(ratio)) return null;
  const clamped = clampSplitRatio(ratio);
  return updateSplitAtPath(layout, path, clamped);
}

function updateSplitAtPath(
  node: PaneLayout,
  path: SplitPath,
  ratio: number,
): PaneLayout | null {
  if (path.length === 0) {
    return node.type === "split" ? { ...node, ratio } : null;
  }
  if (node.type !== "split") return null;
  const [head, ...rest] = path;
  if (head === "first") {
    const first = updateSplitAtPath(node.first, rest, ratio);
    return first === null ? null : { ...node, first };
  }
  const second = updateSplitAtPath(node.second, rest, ratio);
  return second === null ? null : { ...node, second };
}

function replacePaneNode(
  node: PaneLayout,
  paneId: string,
  replace: (leaf: PaneLayout) => PaneLayout,
): PaneLayout | null {
  if (node.type === "leaf") {
    return node.paneId === paneId ? replace(node) : null;
  }
  const first = replacePaneNode(node.first, paneId, replace);
  if (first !== null) return { ...node, first };
  const second = replacePaneNode(node.second, paneId, replace);
  if (second !== null) return { ...node, second };
  return null;
}

function removePaneNode(node: PaneLayout, paneId: string): PaneLayout | null {
  if (node.type === "leaf") {
    return node.paneId === paneId ? null : node;
  }
  const first = removePaneNode(node.first, paneId);
  if (first === null) return node.second;
  const second = removePaneNode(node.second, paneId);
  if (second === null) return first;
  if (first === node.first && second === node.second) return node;
  return { ...node, first, second };
}

/**
 * Validates untrusted data (typically freshly parsed JSON) against the
 * persisted layout contract. Any single issue rejects the whole tree; there
 * is no partial recovery. Traversal is iterative over an explicit stack with
 * a visited set, so hostile cyclic input cannot exhaust the call stack.
 */
export function validatePaneLayout(value: unknown): LayoutValidationResult {
  const issues: LayoutIssue[] = [];
  const paneIds = new Set<string>();
  const seen = new Set<object>();
  let leafCount = 0;

  interface Frame {
    node: unknown;
    level: number;
  }
  const stack: Frame[] = [{ node: value, level: 1 }];

  while (stack.length > 0) {
    const { node, level } = stack.pop()!;

    if (typeof node !== "object" || node === null || Array.isArray(node)) {
      issues.push({
        code: "invalid_structure",
        message: `layout node at level ${level} is not an object`,
      });
      continue;
    }
    if (seen.has(node)) {
      issues.push({
        code: "cycle_detected",
        message: `layout node at level ${level} participates in a cycle`,
      });
      continue;
    }
    seen.add(node);

    if (level > LAYOUT_LIMITS.maxSplitDepth) {
      issues.push({
        code: "tree_too_deep",
        message: `layout exceeds maximum depth of ${LAYOUT_LIMITS.maxSplitDepth} levels`,
      });
      continue;
    }

    const candidate = node as Record<string, unknown>;
    if (candidate.type === "leaf") {
      if (!isValidPaneId(candidate.paneId)) {
        issues.push({
          code: "invalid_pane_id",
          message:
            "leaf pane id must be a non-empty printable string of at most 128 characters",
        });
        continue;
      }
      const paneId = candidate.paneId;
      if (paneIds.has(paneId)) {
        issues.push({
          code: "duplicate_pane_id",
          message: `pane id "${paneId}" appears more than once`,
        });
        continue;
      }
      paneIds.add(paneId);
      leafCount += 1;
      if (leafCount > LAYOUT_LIMITS.maxPanesPerTab) {
        issues.push({
          code: "too_many_panes",
          message: `tab contains more than ${LAYOUT_LIMITS.maxPanesPerTab} panes`,
        });
      }
      continue;
    }

    if (candidate.type === "split") {
      if (
        candidate.direction !== "horizontal" &&
        candidate.direction !== "vertical"
      ) {
        issues.push({
          code: "invalid_structure",
          message: `split direction at level ${level} must be "horizontal" or "vertical"`,
        });
      }
      if (
        typeof candidate.ratio !== "number" ||
        !Number.isFinite(candidate.ratio) ||
        candidate.ratio < LAYOUT_LIMITS.minSplitRatio ||
        candidate.ratio > LAYOUT_LIMITS.maxSplitRatio
      ) {
        issues.push({
          code: "invalid_ratio",
          message: `split ratio at level ${level} must be a finite number between ${LAYOUT_LIMITS.minSplitRatio} and ${LAYOUT_LIMITS.maxSplitRatio}`,
        });
      }
      if (candidate.first === undefined || candidate.second === undefined) {
        issues.push({
          code: "invalid_structure",
          message: `split node at level ${level} requires both first and second children`,
        });
        continue;
      }
      stack.push({ node: candidate.first, level: level + 1 });
      stack.push({ node: candidate.second, level: level + 1 });
      continue;
    }

    issues.push({
      code: "invalid_structure",
      message: `layout node at level ${level} must have type "leaf" or "split"`,
    });
  }

  if (issues.length > 0) return { ok: false, issues };
  return { ok: true };
}
