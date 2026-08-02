/**
 * Screen-manifest matching for agent activity detection.
 *
 * The input is the live bottom of the xterm active buffer (80 logical lines
 * max), NOT the scrolled viewport. Logical lines join wrapped rows at the
 * terminal width and are terminated by newlines. ANSI control sequences are
 * stripped before matching; Unicode is preserved; trailing whitespace is
 * trimmed. English letters compare case-insensitively only when a rule
 * explicitly asks (inline `(?i)`).
 *
 * Manifests are Herdr's `codex.toml` / `claude.toml` / `opencode.toml` /
 * `grok.toml` at commit `26a7bc8`, ported to JSON without reordering,
 * re-prioritizing, or selecting rules.
 */

import codexManifest from "./manifests/codex.json" with { type: "json" };
import claudeManifest from "./manifests/claude.json" with { type: "json" };
import opencodeManifest from "./manifests/opencode.json" with { type: "json" };
import grokManifest from "./manifests/grok.json" with { type: "json" };

export type AgentActivity = "unknown" | "idle" | "working" | "blocked" | "done";

export type ScreenRegionKind =
  | "osc_title"
  | "osc_progress"
  | "whole_recent"
  | "after_last_prompt_marker"
  | "after_last_horizontal_rule"
  | "bottom_lines"
  | "top_lines"
  | "prompt_box_body";

export interface ScreenRegion {
  kind: ScreenRegionKind;
  n?: number;
}

export interface ManifestCondition {
  contains?: string[];
  regex?: string;
  lineRegex?: string[];
  any?: ManifestCondition[];
  all?: ManifestCondition[];
  not?: ManifestCondition[];
}

export interface ManifestRule {
  id: string;
  state: AgentActivity;
  priority: number;
  region: ScreenRegion;
  visibleBlocker?: boolean;
  visibleWorking?: boolean;
  visibleIdle?: boolean;
  skipStateUpdate?: boolean;
  condition: ManifestCondition;
}

export interface ScreenManifest {
  id: string;
  version: string;
  updatedAt?: string;
  rules: ManifestRule[];
}

export interface ScreenSnapshot {
  /** Logical lines, earliest first, no trailing whitespace, no ANSI. */
  lines: string[];
  /** Terminal title from OSC 0/2, if present. */
  title: string | null;
  /** OSC 9;4 progress payload (after "9;"). */
  progress: string | null;
}

export interface ScreenMatch {
  ruleId: string;
  state: AgentActivity;
  priority: number;
}

export const MANIFESTS: Record<string, ScreenManifest> = {
  codex: codexManifest as ScreenManifest,
  claude: claudeManifest as ScreenManifest,
  opencode: opencodeManifest as ScreenManifest,
  grok: grokManifest as ScreenManifest,
};

/** Max logical lines considered; older lines never affect a match. */
export const MAX_SNAPSHOT_LINES = 80;

export const ACTIVITY_PRIORITY: Record<AgentActivity, number> = {
  blocked: 4,
  working: 3,
  done: 2,
  idle: 1,
  unknown: 0,
};

// ---------------------------------------------------------------------------
// Snapshot building
// ---------------------------------------------------------------------------

/** Strips ANSI escape sequences (CSI, OSC), keeping plain text. */
export function stripAnsi(text: string): string {
  // eslint-disable-next-line no-control-regex
  return text.replace(/\[[0-9;?]*[a-zA-Z]/g, "").replace(/\][^]*(?:|\\)/g, "");
}

const PROMPT_MARKER = /^\s*❯/;
const HORIZONTAL_RULE = /^-{3,}\s*$/;

/** Applies ANSI stripping, normalizes whitespace, trims trailing whitespace. */
export function cleanLogicalLine(line: string): string {
  return stripAnsi(line).replace(/\r/g, "").replace(/\s+$/g, "");
}

/**
 * Converts raw terminal rows into logical lines: joins wrapped rows until a
 * newline ends the logical line, then keeps only the last `limit` lines.
 */
export function buildLogicalLines(
  rows: { text: string; newline: boolean }[],
  limit = MAX_SNAPSHOT_LINES,
): string[] {
  const logical: string[] = [];
  let buffer = "";
  for (const row of rows) {
    buffer += row.text;
    if (row.newline) {
      const cleaned = cleanLogicalLine(buffer);
      if (cleaned.length > 0 || logical.length === 0) logical.push(cleaned);
      buffer = "";
    }
  }
  if (buffer.length > 0) logical.push(cleanLogicalLine(buffer));
  return logical.slice(-limit);
}

/** Renders a snapshot for a region of the live bottom buffer. */
export function regionLines(
  snapshot: ScreenSnapshot,
  region: ScreenRegion,
): string[] {
  const lines = snapshot.lines;
  switch (region.kind) {
    case "whole_recent":
      return lines;
    case "bottom_lines": {
      const n = region.n ?? 0;
      return lines.slice(-n);
    }
    case "top_lines": {
      const n = region.n ?? 0;
      return lines.slice(0, n);
    }
    case "after_last_prompt_marker": {
      let index = -1;
      for (let i = lines.length - 1; i >= 0; i -= 1) {
        if (PROMPT_MARKER.test(lines[i])) {
          index = i;
          break;
        }
      }
      return index >= 0 ? lines.slice(index) : lines.slice(-1);
    }
    case "after_last_horizontal_rule": {
      let index = -1;
      for (let i = lines.length - 1; i >= 0; i -= 1) {
        if (HORIZONTAL_RULE.test(lines[i])) {
          index = i;
          break;
        }
      }
      return index >= 0 ? lines.slice(index + 1) : lines.slice(-1);
    }
    case "prompt_box_body":
      return lines;
    case "osc_title":
      return snapshot.title ? [snapshot.title] : [];
    case "osc_progress":
      return snapshot.progress ? [snapshot.progress] : [];
  }
}

// ---------------------------------------------------------------------------
// Condition evaluation
// ---------------------------------------------------------------------------

/**
 * Herdr manifests are TOML/PCRE-style: they use `(?i)`/`(?m)` inline flags
 * (unsupported by JavaScript) and `\u{...}` code-point escapes (only valid in
 * u-mode regexes, which forbid inline flags). Convert both to their plain
 * JS equivalents before compiling.
 */
function expandUnicodeEscapes(pattern: string): string {
  return pattern.replace(/\\u\{([0-9a-fA-F]{1,6})\}/g, (_match, hex: string) =>
    String.fromCodePoint(parseInt(hex, 16)),
  );
}

function compileRegex(pattern: string): RegExp | null {
  let source = expandUnicodeEscapes(pattern);
  let flags = "";
  while (/^\(\?([ims]+)\)/.test(source)) {
    const match = /^\(\?([ims]+)\)/.exec(source);
    if (!match) break;
    flags += match[1];
    source = source.slice(match[0].length);
  }
  try {
    return new RegExp(source, flags);
  } catch {
    return null;
  }
}

function matchesRegex(source: string, pattern: string): boolean {
  const compiled = compileRegex(pattern);
  if (compiled === null) return false;
  try {
    return compiled.test(source);
  } catch {
    return false;
  }
}

function evaluateCondition(
  lines: string[],
  condition: ManifestCondition,
): boolean {
  const { contains, regex, lineRegex, any, all, not } = condition;
  if (contains !== undefined) {
    // Herdr manifests write needles lowercase; real terminal output is mixed
    // case ("Do you want to proceed?"). contains matching is case-insensitive.
    const joined = lines.join("\n").toLowerCase();
    if (!contains.every((needle) => joined.includes(needle.toLowerCase())))
      return false;
  }
  if (regex !== undefined) {
    const joined = lines.join("\n");
    if (!matchesRegex(joined, regex)) return false;
  }
  if (lineRegex !== undefined) {
    if (
      !lines.some((line) =>
        lineRegex.some((pattern) => matchesRegex(line, pattern)),
      )
    ) {
      return false;
    }
  }
  if (any !== undefined && !any.some((sub) => evaluateCondition(lines, sub)))
    return false;
  if (all !== undefined && !all.every((sub) => evaluateCondition(lines, sub)))
    return false;
  if (not !== undefined && not.some((sub) => evaluateCondition(lines, sub)))
    return false;
  return true;
}

/**
 * Evaluates a manifest against a snapshot. Returns the highest-priority
 * matched rule (ties keep the first), or null when no rule matches.
 *
 * `skipStateUpdate` rules (transcript viewer, model picker) never author a
 * state: if only skip rules match, the result is `unknown`.
 */
export function matchManifest(
  manifest: ScreenManifest,
  snapshot: ScreenSnapshot,
): ScreenMatch | null {
  let best: ScreenMatch | null = null;
  let bestIsSkip = false;
  for (const rule of manifest.rules) {
    const lines = regionLines(snapshot, rule.region);
    if (lines.length === 0) continue;
    if (!evaluateCondition(lines, rule.condition)) continue;
    const candidate: ScreenMatch = {
      ruleId: rule.id,
      state: rule.state,
      priority: rule.priority,
    };
    const isSkip = rule.skipStateUpdate === true;
    if (best === null || candidate.priority > best.priority) {
      best = candidate;
      bestIsSkip = isSkip;
    }
  }
  if (best === null || bestIsSkip) return null;
  return best;
}

/** Detects the agent manifest for a preset id. */
export function manifestForAgent(agent: string): ScreenManifest | undefined {
  return MANIFESTS[agent];
}

/** Evaluates the default activity for a snapshot, aggregating known agents. */
export function detectScreenActivity(
  agent: string,
  snapshot: ScreenSnapshot,
): ScreenMatch | null {
  const manifest = manifestForAgent(agent);
  if (!manifest) return null;
  return matchManifest(manifest, snapshot);
}
