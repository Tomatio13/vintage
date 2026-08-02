/**
 * React binding for the persisted workspace layout.
 *
 * The host owns the file; this hook loads it, exposes pure structural
 * operations, and autosaves changes (debounced). A damaged file is never
 * rewritten: autosave stops until the user backs up and resets explicitly.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { host } from "../host/index.ts";
import type { WorkspaceRootRecord } from "../host/types.ts";
import {
  closePane as closePaneInLayout,
  listPaneIds,
  resizeSplit,
  resizeSplitAtPath,
  singlePaneLayout,
  splitPane as splitPaneInLayout,
  type SplitPath,
} from "./paneLayout.ts";
import { emptyLayoutFile, type WorkspaceLayoutFile } from "./persistence.ts";
import {
  normalizeTabTitle,
  type AgentTabState,
  type PaneDefinition,
  type SplitDirection,
  type WorkspaceState,
} from "./types.ts";

const AUTOSAVE_DEBOUNCE_MS = 400;

export type LayoutStatus = "loading" | "ready" | "invalid";

export interface AddedTab {
  tabId: string;
  paneId: string;
}

export interface PaneSplitRequest {
  workspaceId: string;
  tabId: string;
  paneId: string;
  direction: SplitDirection;
  defaultShellId: string;
}

export interface UseWorkspaceLayoutResult {
  status: LayoutStatus;
  invalidReason: string | null;
  autosaveDisabled: boolean;
  layout: WorkspaceLayoutFile;
  lastSaveError: string | null;
  addWorkspaceRoot: (root: WorkspaceRootRecord) => void;
  removeWorkspaceRoot: (workspaceId: string) => void;
  addTab: (workspaceId: string, defaultShellId: string) => AddedTab;
  closeTab: (workspaceId: string, tabId: string) => string[];
  selectTab: (workspaceId: string, tabId: string) => void;
  renameTab: (workspaceId: string, tabId: string, title: string) => void;
  splitPane: (request: PaneSplitRequest) => string | null;
  closePane: (workspaceId: string, tabId: string, paneId: string) => string[];
  selectPane: (workspaceId: string, tabId: string, paneId: string) => void;
  resizeDivider: (
    workspaceId: string,
    tabId: string,
    paneId: string,
    direction: SplitDirection,
    ratio: number,
  ) => void;
  resizeDividerByPath: (
    workspaceId: string,
    tabId: string,
    path: SplitPath,
    ratio: number,
  ) => void;
  backupAndResetLayout: () => Promise<void>;
}

function createPaneDefinition(id: string, shellId: string): PaneDefinition {
  return {
    id,
    title: "Terminal",
    shellId,
    agentKind: null,
    launch: { type: "shell", shellId },
    workingDirectory: null,
    resumeSessionId: null,
  };
}

function createTab(
  id: string,
  paneId: string,
  shellId: string,
  title: string,
): AgentTabState {
  return {
    id,
    title,
    layout: singlePaneLayout(paneId),
    selectedPaneId: paneId,
    panes: [createPaneDefinition(paneId, shellId)],
  };
}

function updateWorkspace(
  layout: WorkspaceLayoutFile,
  workspaceId: string,
  update: (workspace: WorkspaceState) => WorkspaceState | null,
): WorkspaceLayoutFile {
  const workspaces: WorkspaceState[] = [];
  for (const workspace of layout.workspaces) {
    if (workspace.id !== workspaceId) {
      workspaces.push(workspace);
      continue;
    }
    const next = update(workspace);
    if (next !== null) workspaces.push(next);
  }
  return { ...layout, workspaces };
}

function updateTab(
  layout: WorkspaceLayoutFile,
  workspaceId: string,
  tabId: string,
  update: (tab: AgentTabState) => AgentTabState | null,
): WorkspaceLayoutFile {
  return updateWorkspace(layout, workspaceId, (workspace) => {
    const tabs: AgentTabState[] = [];
    let removedSelected = false;
    for (const tab of workspace.tabs) {
      if (tab.id !== tabId) {
        tabs.push(tab);
        continue;
      }
      const next = update(tab);
      if (next === null) {
        if (workspace.selectedTabId === tab.id) removedSelected = true;
      } else {
        tabs.push(next);
      }
    }
    let selectedTabId = workspace.selectedTabId;
    if (removedSelected || !tabs.some((tab) => tab.id === selectedTabId)) {
      selectedTabId = tabs[tabs.length - 1]?.id ?? "";
    }
    return { ...workspace, tabs, selectedTabId };
  });
}

export function useWorkspaceLayout(): UseWorkspaceLayoutResult {
  const [status, setStatus] = useState<LayoutStatus>("loading");
  const [invalidReason, setInvalidReason] = useState<string | null>(null);
  const [autosaveDisabled, setAutosaveDisabled] = useState(false);
  const [layout, setLayout] = useState<WorkspaceLayoutFile>(() =>
    emptyLayoutFile(),
  );
  const [lastSaveError, setLastSaveError] = useState<string | null>(null);
  const loaded = useRef(false);

  useEffect(() => {
    if (loaded.current) return;
    loaded.current = true;
    let cancelled = false;
    void (async () => {
      try {
        const outcome = await host.workspaces.loadLayout();
        if (cancelled) return;
        if (outcome.status === "ok") {
          setLayout(outcome.layout);
          setStatus("ready");
        } else if (outcome.status === "empty") {
          setLayout(emptyLayoutFile());
          setStatus("ready");
        } else {
          setLayout(emptyLayoutFile());
          setInvalidReason(outcome.reason);
          setAutosaveDisabled(true);
          setStatus("invalid");
        }
      } catch {
        if (cancelled) return;
        setLayout(emptyLayoutFile());
        setInvalidReason("Workspace layout could not be loaded.");
        setAutosaveDisabled(true);
        setStatus("invalid");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (status !== "ready" || autosaveDisabled) return;
    const timer = window.setTimeout(() => {
      host.workspaces
        .saveLayout(layout)
        .then(() => setLastSaveError(null))
        .catch((error: unknown) => {
          const message =
            typeof error === "object" && error !== null && "message" in error
              ? String((error as { message: unknown }).message)
              : "Workspace layout could not be saved.";
          setLastSaveError(message);
        });
    }, AUTOSAVE_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [layout, status, autosaveDisabled]);

  const addWorkspaceRoot = useCallback((root: WorkspaceRootRecord) => {
    setLayout((current) => {
      if (current.workspaces.some((workspace) => workspace.id === root.id)) {
        return current;
      }
      const workspace: WorkspaceState = {
        id: root.id,
        path: root.path,
        title: root.title,
        tabs: [],
        selectedTabId: "",
      };
      return { ...current, workspaces: [...current.workspaces, workspace] };
    });
  }, []);

  const removeWorkspaceRoot = useCallback((workspaceId: string) => {
    setLayout((current) => ({
      ...current,
      workspaces: current.workspaces.filter(
        (workspace) => workspace.id !== workspaceId,
      ),
    }));
  }, []);

  const addTab = useCallback(
    (workspaceId: string, defaultShellId: string): AddedTab => {
      const tabId = crypto.randomUUID();
      const paneId = crypto.randomUUID();
      setLayout((current) =>
        updateWorkspace(current, workspaceId, (workspace) => {
          const tab = createTab(
            tabId,
            paneId,
            defaultShellId,
            `Terminal ${workspace.tabs.length + 1}`,
          );
          return {
            ...workspace,
            tabs: [...workspace.tabs, tab],
            selectedTabId: tab.id,
          };
        }),
      );
      return { tabId, paneId };
    },
    [],
  );

  const closeTab = useCallback(
    (workspaceId: string, tabId: string): string[] => {
      const tab = layout.workspaces
        .find((workspace) => workspace.id === workspaceId)
        ?.tabs.find((candidate) => candidate.id === tabId);
      const removedPaneIds = tab ? listPaneIds(tab.layout) : [];
      setLayout((current) =>
        updateTab(current, workspaceId, tabId, () => null),
      );
      return removedPaneIds;
    },
    [layout],
  );

  const selectTab = useCallback((workspaceId: string, tabId: string) => {
    setLayout((current) =>
      updateWorkspace(current, workspaceId, (workspace) => ({
        ...workspace,
        selectedTabId: tabId,
      })),
    );
  }, []);

  const renameTab = useCallback(
    (workspaceId: string, tabId: string, title: string) => {
      const normalized = normalizeTabTitle(title);
      if (normalized === null) return;
      setLayout((current) =>
        updateTab(current, workspaceId, tabId, (tab) =>
          tab.title === normalized ? tab : { ...tab, title: normalized },
        ),
      );
    },
    [],
  );

  const splitPane = useCallback(
    (request: PaneSplitRequest): string | null => {
      const { workspaceId, tabId, paneId, direction, defaultShellId } = request;
      const tab = layout.workspaces
        .find((workspace) => workspace.id === workspaceId)
        ?.tabs.find((candidate) => candidate.id === tabId);
      if (!tab) return null;

      const newPaneId = crypto.randomUUID();
      if (splitPaneInLayout(tab.layout, paneId, direction, newPaneId) === null) {
        return null;
      }

      setLayout((current) =>
        updateTab(current, workspaceId, tabId, (currentTab) => {
          const nextLayout = splitPaneInLayout(
            currentTab.layout,
            paneId,
            direction,
            newPaneId,
          );
          if (nextLayout === null) return currentTab;
          return {
            ...currentTab,
            layout: nextLayout,
            selectedPaneId: newPaneId,
            panes: [
              ...currentTab.panes,
              createPaneDefinition(newPaneId, defaultShellId),
            ],
          };
        }),
      );
      return newPaneId;
    },
    [layout],
  );

  const closePane = useCallback(
    (workspaceId: string, tabId: string, paneId: string): string[] => {
      const tab = layout.workspaces
        .find((workspace) => workspace.id === workspaceId)
        ?.tabs.find((candidate) => candidate.id === tabId);
      if (!tab) return [];
      const before = listPaneIds(tab.layout);
      const nextLayout = closePaneInLayout(tab.layout, paneId);
      const removedPaneIds =
        nextLayout === null ? before : before.filter((id) => id === paneId);
      setLayout((current) =>
        updateTab(current, workspaceId, tabId, (currentTab) => {
          if (nextLayout === null) return null;
          const surviving = new Set(listPaneIds(nextLayout));
          const selection = surviving.has(currentTab.selectedPaneId)
            ? currentTab.selectedPaneId
            : nextLayout.type === "leaf"
              ? nextLayout.paneId
              : paneId;
          return {
            ...currentTab,
            layout: nextLayout,
            selectedPaneId: surviving.has(selection)
              ? selection
              : (listPaneIds(nextLayout)[0] ?? ""),
            panes: currentTab.panes.filter((pane) => surviving.has(pane.id)),
          };
        }),
      );
      return removedPaneIds;
    },
    [layout],
  );

  const selectPane = useCallback(
    (workspaceId: string, tabId: string, paneId: string) => {
      setLayout((current) =>
        updateTab(current, workspaceId, tabId, (tab) => ({
          ...tab,
          selectedPaneId: paneId,
        })),
      );
    },
    [],
  );

  const resizeDivider = useCallback(
    (
      workspaceId: string,
      tabId: string,
      paneId: string,
      direction: SplitDirection,
      ratio: number,
    ) => {
      setLayout((current) =>
        updateTab(current, workspaceId, tabId, (tab) => {
          const nextLayout = resizeSplit(tab.layout, paneId, direction, ratio);
          return nextLayout === null ? tab : { ...tab, layout: nextLayout };
        }),
      );
    },
    [],
  );

  const resizeDividerByPath = useCallback(
    (workspaceId: string, tabId: string, path: SplitPath, ratio: number) => {
      setLayout((current) =>
        updateTab(current, workspaceId, tabId, (tab) => {
          const nextLayout = resizeSplitAtPath(tab.layout, path, ratio);
          return nextLayout === null ? tab : { ...tab, layout: nextLayout };
        }),
      );
    },
    [],
  );

  const backupAndResetLayout = useCallback(async () => {
    await host.workspaces.backupAndResetLayout();
    setLayout(emptyLayoutFile());
    setInvalidReason(null);
    setAutosaveDisabled(false);
    setLastSaveError(null);
    setStatus("ready");
  }, []);

  return {
    status,
    invalidReason,
    autosaveDisabled,
    layout,
    lastSaveError,
    addWorkspaceRoot,
    removeWorkspaceRoot,
    addTab,
    closeTab,
    selectTab,
    renameTab,
    splitPane,
    closePane,
    selectPane,
    resizeDivider,
    resizeDividerByPath,
    backupAndResetLayout,
  };
}
