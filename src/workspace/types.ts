/**
 * Pure state model for the terminal workspace.
 *
 * Everything in src/workspace/ must stay free of React and Tauri imports so
 * it can be unit tested with the Node test runner. Host DTOs that mirror
 * these shapes live in src/host/types.ts and must change together with them.
 */

/** Persistence schema version required at the root of workspace-layouts.json. */
export const WORKSPACE_LAYOUT_VERSION = 1;

/**
 * Hard limits for persisted layout data and launch specs. Load validation
 * rejects the whole file on any violation; there is no partial recovery.
 */
export const LAYOUT_LIMITS = {
  maxWorkspaces: 64,
  maxTabsPerWorkspace: 64,
  maxPanesPerTab: 64,
  /**
   * Maximum split-tree depth in levels (a lone leaf has depth 1).
   * Trees reaching level maxSplitDepth + 1 are rejected.
   */
  maxSplitDepth: 16,
  minSplitRatio: 0.2,
  maxSplitRatio: 0.8,
  /** Display titles are bounded in Unicode scalar values, not UTF-16 units. */
  maxTitleCodePoints: 128,
  /** Pane, tab and workspace ids are bounded opaque strings. */
  maxIdLength: 128,
  maxProgramBytes: 32 * 1024,
  maxArgs: 256,
  maxArgBytes: 32 * 1024,
  maxArgvBytes: 64 * 1024,
} as const;

export type SplitDirection = "horizontal" | "vertical";

export type PaneLayout =
  | { type: "leaf"; paneId: string }
  | {
      type: "split";
      direction: SplitDirection;
      ratio: number;
      first: PaneLayout;
      second: PaneLayout;
    };

/** Lifecycle of the PTY process backing a pane. Never persisted. */
export type PtyState = "starting" | "running" | "stopped" | "exited" | "error";

/** Agent activity surfaced in sidebar badges. Wire and storage values are lowercase. */
export type AgentActivity = "unknown" | "idle" | "working" | "blocked" | "done";

/** Activity values external sources can report. `done` is derived, never reported. */
export type ReportedActivity = Exclude<AgentActivity, "done">;

export type AgentPreset = "grok" | "codex" | "claude" | "opencode";

export type ShellKind =
  "powershell" | "pwsh" | "git-bash" | "bash" | "zsh" | "posix" | "custom";

export type PaneLaunchSpec =
  | { type: "shell"; shellId: string }
  | {
      type: "agent";
      preset: AgentPreset;
      shellId: string;
      args: string[];
      resumeSessionId?: string;
    }
  | { type: "custom"; program: string; args: string[] };

/**
 * Persisted pane definition. Runtime handles (PTY id, generation, process
 * handles, hook tokens) are never persisted and never appear here.
 */
export interface PaneDefinition {
  id: string;
  title: string;
  shellId: string;
  agentKind: AgentPreset | null;
  launch: PaneLaunchSpec;
  /** Normalized workspace-relative directory; null means the workspace root. */
  workingDirectory: string | null;
  /** Native session reference used only for an explicit restart-with-resume. */
  resumeSessionId: string | null;
}

export interface AgentTabState {
  id: string;
  title: string;
  layout: PaneLayout;
  selectedPaneId: string;
  /** Pane definitions owned by this tab; must match the layout leaves exactly. */
  panes: PaneDefinition[];
}

export interface WorkspaceState {
  id: string;
  /** Normalized absolute path owned by the host workspace registry. */
  path: string;
  title: string;
  tabs: AgentTabState[];
  selectedTabId: string;
}

/** Counts Unicode scalar values (code points), not UTF-16 code units. */
export function countCodePoints(value: string): number {
  return Array.from(value).length;
}

/** Trims and validates a user-editable tab title before persistence. */
export function normalizeTabTitle(value: string): string | null {
  const title = value.trim();
  if (!title || countCodePoints(title) > LAYOUT_LIMITS.maxTitleCodePoints) {
    return null;
  }
  return title;
}
