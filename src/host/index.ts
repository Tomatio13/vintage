import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AgentActivity,
  AgentActivityEvent,
  AppUpdateInfo,
  AppUpdateProgress,
  IntegrationAgent,
  IntegrationStatus,
  LayoutResetResult,
  LoadLayoutOutcome,
  ShellDescriptor,
  TerminalExitEvent,
  TerminalInfo,
  TerminalOutputEvent,
  WorkspaceChangedEvent,
  WorkspaceDirectoryListing,
  WorkspaceFileAttachment,
  WorkspaceFilePreview,
  WorkspaceFileTarget,
  WorkspaceLayoutFile,
  WorkspaceRootRecord,
} from "./types";
import type { PaneLaunchSpec } from "../workspace/types.ts";

type EventHandler<T> = (payload: T) => void;

function command<T>(name: string, payload?: Record<string, unknown>) {
  return invoke<T>(name, payload);
}

function subscribe<T>(event: string, handler: EventHandler<T>) {
  return listen<T>(event, ({ payload }) => handler(payload));
}

function workspaceFilePayload(target: WorkspaceFileTarget) {
  return { workspaceId: target.workspaceId };
}

export const host = {
  configureNativeTitlebar: () =>
    command<number | null>("configure_native_titlebar"),

  updates: {
    check: () => command<AppUpdateInfo | null>("check_app_update"),
    install: () => command<void>("install_app_update"),
    onProgress: (handler: EventHandler<AppUpdateProgress>) =>
      subscribe("vintage://update-progress", handler),
  },

  terminal: {
    start: (options: {
      terminalId: string;
      paneId?: string;
      generation?: number;
      workspaceId?: string;
      launch?: PaneLaunchSpec;
      workingDirectory?: string | null;
      cols: number;
      rows: number;
    }) => command<TerminalInfo>("terminal_start", options),
    write: (terminalId: string, data: number[]) =>
      command<void>("terminal_write", { terminalId, data }),
    resize: (terminalId: string, cols: number, rows: number) =>
      command<void>("terminal_resize", { terminalId, cols, rows }),
    stop: (terminalId: string) =>
      command<void>("terminal_stop", { terminalId }),
    onOutput: (handler: EventHandler<TerminalOutputEvent>) =>
      subscribe("vintage://terminal-output", handler),
    onExit: (handler: EventHandler<TerminalExitEvent>) =>
      subscribe("vintage://terminal-exit", handler),
    reportScreenState: (options: {
      paneId: string;
      generation: number;
      activity: Exclude<AgentActivity, "done">;
    }) => command<void>("agent_report_screen_state", options),
    onActivity: (handler: EventHandler<AgentActivityEvent>) =>
      subscribe("vintage://agent-activity", handler),
    initializeHookIpc: (token: Uint8Array) =>
      command<number>("hook_ipc_initialize", { token: Array.from(token) }),
  },

  shells: {
    list: () => command<ShellDescriptor[]>("shell_list"),
  },

  integrations: {
    list: () => command<IntegrationStatus[]>("integration_list"),
    install: (agent: IntegrationAgent) =>
      command<IntegrationStatus>("integration_install", { agent }),
    uninstall: (agent: IntegrationAgent) =>
      command<IntegrationStatus>("integration_uninstall", { agent }),
  },

  workspaceFiles: {
    list: (target: WorkspaceFileTarget, path: string) =>
      command<WorkspaceDirectoryListing>("workspace_list_directory", {
        ...workspaceFilePayload(target),
        path,
      }),
    preview: (target: WorkspaceFileTarget, path: string) =>
      command<WorkspaceFilePreview>("workspace_preview_file", {
        ...workspaceFilePayload(target),
        path,
      }),
    write: (target: WorkspaceFileTarget, path: string, content: string) =>
      command<void>("workspace_write_file", {
        ...workspaceFilePayload(target),
        path,
        content,
      }),
    watch: (
      target: WorkspaceFileTarget,
      watchId: string,
      paths: readonly string[],
    ) =>
      command<void>("workspace_watch", {
        ...workspaceFilePayload(target),
        watchId,
        paths,
      }),
    unwatch: (watchId: string) =>
      command<void>("workspace_unwatch", { watchId }),
    openFolder: (target: WorkspaceFileTarget, path: string) =>
      command<void>("workspace_open_folder", {
        ...workspaceFilePayload(target),
        path,
      }),
    inspectAttachment: (target: WorkspaceFileTarget, path: string) =>
      command<WorkspaceFileAttachment>("workspace_inspect_attachment", {
        ...workspaceFilePayload(target),
        path,
      }),
    onChanged: (handler: EventHandler<WorkspaceChangedEvent>) =>
      subscribe("vintage://workspace-changed", handler),
  },

  workspaces: {
    listRoots: () => command<WorkspaceRootRecord[]>("workspace_list_roots"),
    chooseRoot: () =>
      command<WorkspaceRootRecord | null>("workspace_choose_root"),
    addRoot: (path: string) =>
      command<WorkspaceRootRecord>("workspace_add_root", { path }),
    removeRoot: (workspaceId: string) =>
      command<WorkspaceRootRecord[]>("workspace_remove_root", { workspaceId }),
    loadLayout: () => command<LoadLayoutOutcome>("workspace_layout_load"),
    saveLayout: (layout: WorkspaceLayoutFile) =>
      command<void>("workspace_layout_save", { layout }),
    backupAndResetLayout: () =>
      command<LayoutResetResult>("workspace_layout_backup_and_reset"),
  },
} as const;
