import type { WorkspaceLayoutFile } from "../workspace/persistence.ts";
import type { ShellKind } from "../workspace/types.ts";

export type { ShellKind };

export interface AppUpdateInfo {
  currentVersion: string;
  version: string;
  body: string | null;
  date: string | null;
}

export interface AppUpdateProgress {
  stage: "downloading" | "downloaded" | "installing";
  downloaded: number;
  total: number | null;
}

export interface ShellDescriptor {
  id: string;
  label: string;
  kind: ShellKind;
  executable: string;
  platform: "windows" | "unix";
  available: boolean;
  supportsAgentWrapper: boolean;
}

export interface TerminalInfo {
  terminalId: string;
  paneId: string;
  generation: number;
  workingDirectory: string;
  shell: ShellDescriptor;
  processId: number;
}

export interface TerminalOutputEvent {
  terminalId: string;
  generation: number;
  data: number[];
}

export interface TerminalExitEvent {
  terminalId: string;
  generation: number;
  exitCode: number | null;
  signal: string | null;
}

export type AgentActivity = "unknown" | "idle" | "working" | "blocked" | "done";

export interface AgentActivityEvent {
  paneId: string;
  generation: number;
  activity: Exclude<AgentActivity, "done">;
  source: "screen" | "opencode-plugin" | "runtime";
  /** The CLI preset the reporting hook/plugin belongs to, e.g. "codex". */
  agent: string | null;
  sessionId: string | null;
}

export type WorkspaceFileKind = "directory" | "file" | "symlink";
export type WorkspacePreviewKind =
  "font" | "image" | "pdf" | "text" | "unsupported";

export interface WorkspaceFileEntry {
  name: string;
  path: string;
  kind: WorkspaceFileKind;
  size: number | null;
  modifiedAt: number | null;
  hidden: boolean;
}

export interface WorkspaceDirectoryListing {
  path: string;
  entries: WorkspaceFileEntry[];
  truncated: boolean;
}

export interface WorkspaceChangedEvent {
  watchId: string;
  paths: string[];
}

export type WorkspaceFileTarget = { workspaceId: string };

export interface WorkspaceFilePreview {
  path: string;
  name: string;
  kind: WorkspacePreviewKind;
  mimeType: string | null;
  size: number;
  content: string | null;
  dataUrl: string | null;
  truncated: boolean;
}

export interface WorkspaceFileAttachment {
  path: string;
  name: string;
  size: number;
  mimeType?: string | null;
}

// ---------------------------------------------------------------------------
// Terminal workspace wire contracts (mirrors src-tauri/src/workspaces.rs)
// ---------------------------------------------------------------------------

/** Structured error returned by the new workspace commands. */
export type HostErrorCode =
  | "invalid_request"
  | "not_found"
  | "conflict"
  | "unavailable"
  | "io_error"
  | "invalid_config"
  | "stale_generation";

export interface HostError {
  code: HostErrorCode;
  message: string;
}

/** A registered workspace root. Only `id` crosses into feature code as a handle. */
export type { WorkspaceRootRecord } from "../workspace/persistence.ts";

/** Persisted layout file shape; the domain type lives in src/workspace/. */
export type { WorkspaceLayoutFile } from "../workspace/persistence.ts";

export type LoadLayoutOutcome =
  | { status: "ok"; layout: WorkspaceLayoutFile }
  | { status: "empty" }
  | { status: "invalid"; reason: string };

export interface LayoutResetResult {
  backupPath: string | null;
}

// ---------------------------------------------------------------------------
// Integration management (mirrors src-tauri/src/integrations.rs)
// ---------------------------------------------------------------------------

export type IntegrationAgent = "codex" | "claude" | "opencode";

export type IntegrationState =
  "not_installed" | "installed" | "outdated" | "conflict";

export interface IntegrationStatus {
  agent: IntegrationAgent;
  state: IntegrationState;
  scriptPath: string | null;
  message: string;
}
