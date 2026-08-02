/**
 * Pure mirrors of the host persistence contracts.
 *
 * The Rust host owns the real reads and writes of workspace-layouts.json and
 * working-directories.json. These functions mirror the same validation and
 * migration rules so the renderer can pre-validate, and so both sides stay
 * covered by tests without Tauri. Change them together with
 * src-tauri/src/workspaces.rs.
 */

import { listPaneIds, validatePaneLayout } from "./paneLayout.ts";
import {
  countCodePoints,
  LAYOUT_LIMITS,
  WORKSPACE_LAYOUT_VERSION,
  type AgentTabState,
  type PaneDefinition,
  type PaneLaunchSpec,
  type WorkspaceState,
} from "./types.ts";

/** NUL is rejected anywhere in persisted values, matching the host. */
const NUL = String.fromCharCode(0);

// ---------------------------------------------------------------------------
// Layout file (workspace-layouts.json)
// ---------------------------------------------------------------------------

export interface WorkspaceLayoutFile {
  version: number;
  workspaces: WorkspaceState[];
}

export type ParseLayoutResult =
  | { status: "ok"; file: WorkspaceLayoutFile }
  | { status: "invalid"; reason: string };

export function emptyLayoutFile(): WorkspaceLayoutFile {
  return { version: WORKSPACE_LAYOUT_VERSION, workspaces: [] };
}

/**
 * Parses and fully validates layout file text. Mirrors the host: any single
 * violation rejects the whole file, and the caller must leave the original
 * file untouched and stop autosaving.
 */
export function parseWorkspaceLayoutFile(text: string): ParseLayoutResult {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return { status: "invalid", reason: "Workspace layout is damaged." };
  }
  const issues = validateLayoutFileValue(parsed);
  if (issues.length > 0) {
    return { status: "invalid", reason: issues[0] };
  }
  return { status: "ok", file: parsed as WorkspaceLayoutFile };
}

/** Full-file validation. Returns every violation found, English messages. */
export function validateLayoutFileValue(value: unknown): string[] {
  const issues: string[] = [];
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return ["Workspace layout is damaged."];
  }
  const file = value as Record<string, unknown>;
  if (file.version !== WORKSPACE_LAYOUT_VERSION) {
    issues.push("Workspace layout has an unsupported version.");
    return issues;
  }
  if (!Array.isArray(file.workspaces)) {
    issues.push("Workspace layout is damaged.");
    return issues;
  }
  if (file.workspaces.length > LAYOUT_LIMITS.maxWorkspaces) {
    issues.push("Workspace layout exceeds the supported number of workspaces.");
  }
  const workspaceIds = new Set<string>();
  for (const workspace of file.workspaces) {
    issues.push(...validateWorkspaceValue(workspace));
    if (isRecord(workspace) && isValidIdentifier(workspace.id)) {
      const id = workspace.id as string;
      if (workspaceIds.has(id)) {
        issues.push("Workspace layout contains a duplicate workspace id.");
      }
      workspaceIds.add(id);
    }
  }
  return issues;
}

function validateWorkspaceValue(value: unknown): string[] {
  const issues: string[] = [];
  if (!isRecord(value)) {
    issues.push("Workspace layout is damaged.");
    return issues;
  }
  const workspace = value as Partial<WorkspaceState>;
  if (!isValidIdentifier(workspace.id)) {
    issues.push("Workspace layout contains an invalid workspace id.");
  }
  if (
    typeof workspace.path !== "string" ||
    workspace.path.length === 0 ||
    workspace.path.includes(NUL)
  ) {
    issues.push("Workspace layout contains an unusable workspace path.");
  }
  if (!isValidTitle(workspace.title)) {
    issues.push(
      "Workspace layout contains a workspace title that is too long.",
    );
  }
  if (!Array.isArray(workspace.tabs)) {
    issues.push("Workspace layout is damaged.");
    return issues;
  }
  if (workspace.tabs.length > LAYOUT_LIMITS.maxTabsPerWorkspace) {
    issues.push("Workspace layout exceeds the supported number of tabs.");
  }
  const tabIds = new Set<string>();
  for (const tab of workspace.tabs) {
    issues.push(...validateTabValue(tab));
    if (isRecord(tab) && isValidIdentifier(tab.id)) {
      const id = tab.id as string;
      if (tabIds.has(id)) {
        issues.push("Workspace layout contains a duplicate tab id.");
      }
      tabIds.add(id);
    }
  }
  if (
    workspace.tabs.length > 0 &&
    (typeof workspace.selectedTabId !== "string" ||
      !tabIds.has(workspace.selectedTabId))
  ) {
    issues.push("Workspace layout references a missing tab.");
  }
  return issues;
}

function validateTabValue(value: unknown): string[] {
  const issues: string[] = [];
  if (!isRecord(value)) {
    issues.push("Workspace layout is damaged.");
    return issues;
  }
  const tab = value as Partial<AgentTabState>;
  if (!isValidIdentifier(tab.id)) {
    issues.push("Workspace layout contains an invalid tab id.");
  }
  if (!isValidTitle(tab.title)) {
    issues.push("Workspace layout contains a tab title that is too long.");
  }
  const treeValidation = validatePaneLayout(tab.layout);
  if (!treeValidation.ok) {
    for (const issue of treeValidation.issues) {
      issues.push(issue.message);
    }
  }
  const leafIds = treeValidation.ok
    ? new Set(listPaneIds(tab.layout!))
    : new Set<string>();
  if (
    typeof tab.selectedPaneId !== "string" ||
    (leafIds.size > 0 && !leafIds.has(tab.selectedPaneId))
  ) {
    issues.push("Workspace layout references a missing pane.");
  }
  if (!Array.isArray(tab.panes)) {
    issues.push("Workspace layout is damaged.");
    return issues;
  }
  const definitionIds = new Set<string>();
  for (const pane of tab.panes) {
    issues.push(...validatePaneDefinitionValue(pane));
    if (isRecord(pane) && isValidIdentifier(pane.id)) {
      const id = pane.id as string;
      if (definitionIds.has(id)) {
        issues.push("Workspace layout contains a duplicate pane definition.");
      }
      definitionIds.add(id);
    }
  }
  if (treeValidation.ok && !setsEqual(leafIds, definitionIds)) {
    issues.push("Workspace layout panes and split tree disagree.");
  }
  return issues;
}

function validatePaneDefinitionValue(value: unknown): string[] {
  const issues: string[] = [];
  if (!isRecord(value)) {
    issues.push("Workspace layout is damaged.");
    return issues;
  }
  const pane = value as Partial<PaneDefinition>;
  if (!isValidIdentifier(pane.id)) {
    issues.push("Workspace layout contains an invalid pane id.");
  }
  if (!isValidTitle(pane.title)) {
    issues.push("Workspace layout contains a pane title that is too long.");
  }
  if (!isValidIdentifier(pane.shellId)) {
    issues.push("Workspace layout contains an invalid shell reference.");
  }
  if (
    pane.workingDirectory !== null &&
    pane.workingDirectory !== undefined &&
    (typeof pane.workingDirectory !== "string" ||
      pane.workingDirectory.includes(NUL))
  ) {
    issues.push("Workspace layout contains an unusable pane directory.");
  }
  if (
    pane.resumeSessionId !== null &&
    pane.resumeSessionId !== undefined &&
    (typeof pane.resumeSessionId !== "string" ||
      pane.resumeSessionId.length === 0 ||
      pane.resumeSessionId.includes(NUL))
  ) {
    issues.push("Workspace layout contains an unusable session reference.");
  }
  issues.push(...validateLaunchSpecValue(pane.launch));
  return issues;
}

function validateLaunchSpecValue(value: unknown): string[] {
  const issues: string[] = [];
  if (!isRecord(value)) {
    issues.push("Workspace layout is damaged.");
    return issues;
  }
  const launch = value as Partial<PaneLaunchSpec>;
  if (launch.type === "shell") {
    if (
      typeof launch.shellId !== "string" ||
      launch.shellId.length === 0 ||
      launch.shellId.includes(NUL)
    ) {
      issues.push("Launch specification references an invalid shell.");
    }
    return issues;
  }
  if (launch.type === "agent") {
    if (
      typeof launch.shellId !== "string" ||
      launch.shellId.length === 0 ||
      launch.shellId.includes(NUL)
    ) {
      issues.push("Launch specification references an invalid shell.");
    }
    issues.push(...validateArgumentList(launch.args));
    if (
      launch.resumeSessionId !== undefined &&
      (typeof launch.resumeSessionId !== "string" ||
        launch.resumeSessionId.length === 0 ||
        launch.resumeSessionId.includes(NUL))
    ) {
      issues.push(
        "Launch specification contains an unusable session reference.",
      );
    }
    return issues;
  }
  if (launch.type === "custom") {
    if (
      typeof launch.program !== "string" ||
      launch.program.length === 0 ||
      utf8Length(launch.program) > LAYOUT_LIMITS.maxProgramBytes ||
      launch.program.includes(NUL)
    ) {
      issues.push("Launch specification contains an invalid program.");
    }
    issues.push(...validateArgumentList(launch.args));
    return issues;
  }
  issues.push("Workspace layout is damaged.");
  return issues;
}

function validateArgumentList(args: unknown): string[] {
  if (!Array.isArray(args)) return ["Workspace layout is damaged."];
  if (args.length > LAYOUT_LIMITS.maxArgs) {
    return ["Launch specification contains too many arguments."];
  }
  let total = 0;
  for (const arg of args) {
    if (
      typeof arg !== "string" ||
      arg.includes(NUL) ||
      utf8Length(arg) > LAYOUT_LIMITS.maxArgBytes
    ) {
      return ["Launch specification contains an unusable argument."];
    }
    total += utf8Length(arg);
  }
  if (total > LAYOUT_LIMITS.maxArgvBytes) {
    return ["Launch specification arguments exceed the supported total size."];
  }
  return [];
}

/** Backup file name for a damaged layout: workspace-layouts.invalid-<UTC>.json */
export function invalidBackupName(utcTimestamp: string): string {
  return `workspace-layouts.invalid-${utcTimestamp}.json`;
}

// ---------------------------------------------------------------------------
// Registry (working-directories.json)
// ---------------------------------------------------------------------------

export interface WorkspaceRootRecord {
  id: string;
  path: string;
  title: string;
  createdAt: number;
}

export interface RegistryFile {
  version: number;
  roots: WorkspaceRootRecord[];
}

export type RegistryMigrationResult =
  | { status: "ok"; roots: WorkspaceRootRecord[]; migrated: boolean }
  | { status: "invalid"; reason: string };

/**
 * Parses registry text in any known shape and migrates legacy entries by
 * assigning ids from `generateId`. `migrated` is true when the host must
 * rewrite the file in the versioned shape.
 */
export function migrateRegistryText(
  text: string,
  generateId: () => string,
): RegistryMigrationResult {
  if (text.trim().length === 0) {
    return { status: "ok", roots: [], migrated: false };
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return { status: "invalid", reason: "Workspace registry is damaged." };
  }

  if (Array.isArray(parsed)) {
    const roots: WorkspaceRootRecord[] = [];
    for (const entry of parsed) {
      let path: string | null = null;
      let createdAt = 0;
      if (typeof entry === "string") {
        path = entry;
      } else if (isRecord(entry) && typeof entry.path === "string") {
        path = entry.path;
        if (typeof entry.createdAt === "number") createdAt = entry.createdAt;
      }
      if (path === null) {
        return { status: "invalid", reason: "Workspace registry is damaged." };
      }
      const record = legacyRecord(path, createdAt, generateId);
      if (record === null) {
        return {
          status: "invalid",
          reason: "Workspace registry contains an unusable path.",
        };
      }
      roots.push(record);
    }
    dedupeByPath(roots);
    const issues = validateRegistryRoots(roots);
    if (issues.length > 0) return { status: "invalid", reason: issues[0] };
    return { status: "ok", roots, migrated: true };
  }

  if (isRecord(parsed)) {
    const file = parsed as Partial<RegistryFile>;
    if (
      file.version !== WORKSPACE_LAYOUT_VERSION ||
      !Array.isArray(file.roots)
    ) {
      return {
        status: "invalid",
        reason: "Workspace registry has an unsupported version.",
      };
    }
    const roots = file.roots as WorkspaceRootRecord[];
    const issues = validateRegistryRoots(roots);
    if (issues.length > 0) return { status: "invalid", reason: issues[0] };
    return { status: "ok", roots, migrated: false };
  }

  return { status: "invalid", reason: "Workspace registry is damaged." };
}

export function validateRegistryRoots(roots: WorkspaceRootRecord[]): string[] {
  if (roots.length > LAYOUT_LIMITS.maxWorkspaces) {
    return ["Workspace registry exceeds the supported number of workspaces."];
  }
  const ids = new Set<string>();
  for (const root of roots) {
    if (!isValidIdentifier(root.id) || ids.has(root.id)) {
      return ["Workspace registry contains an invalid or duplicate id."];
    }
    ids.add(root.id);
    if (
      typeof root.path !== "string" ||
      root.path.length === 0 ||
      root.path.includes(NUL)
    ) {
      return ["Workspace registry contains an unusable path."];
    }
    if (!isValidTitle(root.title)) {
      return ["Workspace registry contains a title that is too long."];
    }
  }
  return [];
}

function legacyRecord(
  path: string,
  createdAt: number,
  generateId: () => string,
): WorkspaceRootRecord | null {
  if (path.length === 0 || path.includes(NUL)) return null;
  return { id: generateId(), path, title: titleFromPath(path), createdAt };
}

function dedupeByPath(roots: WorkspaceRootRecord[]): void {
  const seen = new Set<string>();
  for (let index = 0; index < roots.length;) {
    const root = roots[index];
    if (seen.has(root.path)) {
      roots.splice(index, 1);
    } else {
      seen.add(root.path);
      index += 1;
    }
  }
}

/** Display title from the last path segment (portable across separators). */
export function titleFromPath(path: string): string {
  const segments = path.split(/[/\\]/).filter((segment) => segment.length > 0);
  const last = segments[segments.length - 1];
  return last !== undefined ? last : path;
}

// ---------------------------------------------------------------------------
// Shared validators (mirror workspaces.rs)
// ---------------------------------------------------------------------------

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Non-empty, at most 128 code points, no whitespace or control characters. */
export function isValidIdentifier(value: unknown): value is string {
  if (typeof value !== "string" || value.length === 0) return false;
  const points = Array.from(value);
  if (points.length > LAYOUT_LIMITS.maxIdLength) return false;
  return points.every(
    (point) => !/\s/u.test(point) && !isControlCodePoint(point),
  );
}

function isControlCodePoint(point: string): boolean {
  const code = point.codePointAt(0) ?? 0;
  return code <= 0x1f || (code >= 0x7f && code <= 0x9f);
}

function isValidTitle(value: unknown): value is string {
  return (
    typeof value === "string" &&
    countCodePoints(value) <= LAYOUT_LIMITS.maxTitleCodePoints
  );
}

function setsEqual(a: Set<string>, b: Set<string>): boolean {
  if (a.size !== b.size) return false;
  for (const value of a) {
    if (!b.has(value)) return false;
  }
  return true;
}

const encoder = new TextEncoder();

function utf8Length(value: string): number {
  return encoder.encode(value).byteLength;
}
