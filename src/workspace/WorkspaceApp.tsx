/**
 * Terminal workspace composition root: registry-backed sidebar, tabs, and
 * recursively split panes over the Phase 2/3 host contracts.
 *
 * Every tab stays mounted while the workspace is open — only visibility
 * flips — so switching tabs never tears down a PTY. Closing a pane or tab
 * stops its PTY first, then removes the state.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import type { ResolvedAppearance } from "../appearance";
import { host } from "../host/index.ts";
import type { ShellDescriptor, WorkspaceRootRecord } from "../host/types.ts";
import {
  applyActivityReport,
  effectiveActivity,
  rollupPaneStatuses,
} from "./agentState.ts";
import { listPaneIds, type SplitPath } from "./paneLayout.ts";
import {
  navigatePanes,
  navigateTabs,
  navigateWorkspaces,
} from "./navigation.ts";
import { resolvePreferredShellId } from "../settings/shells.ts";
import {
  paneRuntimeOrStopped,
  removePaneRuntime,
  setPanePtyState,
  startPaneRuntime,
  type PaneRuntimeMap,
} from "./paneRuntime.ts";
import { PaneTerminal } from "./PaneTerminal.tsx";
import {
  matchShortcut,
  type ShortcutAction,
  type ShortcutBinding,
} from "./shortcuts.ts";
import { SplitPaneView } from "./SplitPaneView.tsx";
import { useWorkspaceLayout } from "./useWorkspaceLayout.ts";
import { WorkspaceFilesPanel } from "./WorkspaceFilesPanel.tsx";
import { WorkspaceSidebar } from "./WorkspaceSidebar.tsx";
import { WorkspaceTabs } from "./WorkspaceTabs.tsx";
import type {
  AgentActivity,
  PtyState,
  ReportedActivity,
  WorkspaceState,
} from "./types.ts";
import "./workspace.css";

export function WorkspaceApp({
  appearance,
  active,
  bindings,
  fontFamily,
  fontSize,
  preferredShellId,
  onOpenSettings,
}: {
  appearance: ResolvedAppearance;
  /** Session view is visible; shortcuts are disabled on the settings screen. */
  active: boolean;
  /** User-customizable shortcut bindings; re-registered on change. */
  bindings: ShortcutBinding[];
  /** CSS font-family stack for terminal surfaces. */
  fontFamily: string;
  /** Terminal font size in pixels. */
  fontSize: number;
  /** User-chosen default shell id, or null for the platform default. */
  preferredShellId: string | null;
  onOpenSettings: () => void;
}) {
  const {
    status,
    invalidReason,
    layout,
    addWorkspaceRoot,
    removeWorkspaceRoot,
    addTab,
    closeTab,
    selectTab,
    renameTab,
    splitPane,
    closePane,
    selectPane,
    resizeDividerByPath,
    backupAndResetLayout,
  } = useWorkspaceLayout();

  const [roots, setRoots] = useState<WorkspaceRootRecord[] | null>(null);
  const [defaultShellId, setDefaultShellId] = useState("unix-default");
  const [runtimes, setRuntimes] = useState<PaneRuntimeMap>({});
  const runtimesRef = useRef(runtimes);
  runtimesRef.current = runtimes;
  const [activities, setActivities] = useState<
    Record<string, ReportedActivity>
  >({});
  /** Hidden panes that finished working while hidden raise a `done` badge. */
  const [donePending, setDonePending] = useState<Record<string, boolean>>({});
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState<string | null>(
    null,
  );
  const [filesOpen, setFilesOpen] = useState(false);
  const [workspaceActionError, setWorkspaceActionError] = useState<
    string | null
  >(null);
  const [sidebarWidth, setSidebarWidth] = useState(258);
  const [filesWidth, setFilesWidth] = useState(340);

  // Keep panel widths within the viewport as the window resizes. The sidebar
  // and files panel are grid columns, so a stale width makes the center pane
  // (or the files panel itself) overflow instead of tracking the window.
  useEffect(() => {
    const clampToViewport = () => {
      const width = window.innerWidth;
      setSidebarWidth((current) => Math.min(current, width - 200));
      setFilesWidth((current) => Math.min(current, width - 320));
    };
    clampToViewport();
    window.addEventListener("resize", clampToViewport);
    return () => window.removeEventListener("resize", clampToViewport);
  }, []);

  // Split-tree changes may remount a terminal surface, so pane components do
  // not own host PTY teardown. The workspace root stops every remaining PTY
  // when the entire terminal workspace unmounts.
  useEffect(
    () => () => {
      for (const runtime of Object.values(runtimesRef.current)) {
        if (
          runtime.terminalId &&
          (runtime.ptyState === "running" || runtime.ptyState === "starting")
        ) {
          void host.terminal.stop(runtime.terminalId).catch(() => undefined);
        }
      }
    },
    [],
  );

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      // Hand the hook IPC token to the host exactly once, then discard it.
      // The host injects port + token into PTY child environments.
      const token = crypto.getRandomValues(new Uint8Array(32));
      await host.terminal.initializeHookIpc(token).catch(() => undefined);
      const [rootList, shellList] = await Promise.all([
        host.workspaces.listRoots().catch(() => [] as WorkspaceRootRecord[]),
        host.shells.list().catch(() => [] as ShellDescriptor[]),
      ]);
      if (cancelled) return;
      setRoots(rootList);
      const preferred = resolvePreferredShellId(preferredShellId, shellList);
      if (preferred) setDefaultShellId(preferred);
    })();
    return () => {
      cancelled = true;
    };
  }, [preferredShellId]);

  // Registered roots that are missing from the layout join it. A root that is
  // explicitly removed is deleted from both the registry and saved layout.
  useEffect(() => {
    if (!roots) return;
    for (const root of roots) {
      if (!layout.workspaces.some((workspace) => workspace.id === root.id)) {
        addWorkspaceRoot(root);
      }
    }
  }, [roots, layout.workspaces, addWorkspaceRoot]);

  const workspaces = useMemo(() => {
    if (!roots) return [];
    const registered = new Set(roots.map((root) => root.id));
    return layout.workspaces.filter((workspace) =>
      registered.has(workspace.id),
    );
  }, [layout.workspaces, roots]);

  const selectedWorkspace =
    workspaces.find((workspace) => workspace.id === selectedWorkspaceId) ??
    workspaces[0] ??
    null;

  const handleStartPane = useCallback((paneId: string) => {
    setRuntimes((current) => startPaneRuntime(current, paneId));
  }, []);

  const handleAddTab = useCallback(
    (workspaceId: string) => {
      const { paneId } = addTab(workspaceId, defaultShellId);
      handleStartPane(paneId);
    },
    [addTab, defaultShellId, handleStartPane],
  );

  const handlePtyStateChange = useCallback(
    (paneId: string, state: PtyState, detail: string | null) => {
      setRuntimes((current) => setPanePtyState(current, paneId, state, detail));
    },
    [],
  );

  const isPaneVisible = useCallback(
    (paneId: string) => {
      if (!selectedWorkspace) return false;
      const tab = selectedWorkspace.tabs.find(
        (candidate) => candidate.id === selectedWorkspace.selectedTabId,
      );
      if (!tab) return false;
      return tab.panes.some((pane) => pane.id === paneId);
    },
    [selectedWorkspace],
  );

  // Keyboard navigation across the workspace → tab → pane hierarchy. The
  // keydown listener is capture-phase so it swallows chords before xterm can
  // forward them to the PTY.
  const handleShortcutAction = useCallback(
    (action: ShortcutAction) => {
      const current = selectedWorkspace;
      if (!current) return;

      if (action === "tabNext" || action === "tabPrevious") {
        const tabId = navigateTabs(current, action === "tabNext" ? 1 : -1);
        if (tabId) selectTab(current.id, tabId);
        return;
      }

      const tab = current.tabs.find(
        (candidate) => candidate.id === current.selectedTabId,
      );
      if (!tab) return;

      if (action === "paneNext" || action === "panePrevious") {
        const paneId = navigatePanes(tab, action === "paneNext" ? 1 : -1);
        if (paneId) {
          // Viewing a pane acknowledges any pending done badge, matching the
          // sidebar click handler.
          setDonePending((pending) =>
            pending[paneId] ? { ...pending, [paneId]: false } : pending,
          );
          selectPane(current.id, tab.id, paneId);
        }
        return;
      }

      if (action === "workspaceNext" || action === "workspacePrevious") {
        const nextId = navigateWorkspaces(
          workspaces,
          current.id,
          action === "workspaceNext" ? 1 : -1,
        );
        if (nextId) setSelectedWorkspaceId(nextId);
      }
    },
    [selectedWorkspace, workspaces, selectTab, selectPane],
  );

  const handleShortcutActionRef = useRef(handleShortcutAction);
  handleShortcutActionRef.current = handleShortcutAction;

  useEffect(() => {
    if (!active) return;
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target;
      // Skip while editing text outside a terminal (rename inputs, settings).
      if (
        target instanceof HTMLElement &&
        target.closest("input, textarea, [contenteditable]") &&
        !target.closest(".terminal-surface")
      ) {
        return;
      }
      const action = matchShortcut(
        bindings,
        {
          ctrlKey: event.ctrlKey,
          altKey: event.altKey,
          shiftKey: event.shiftKey,
          metaKey: event.metaKey,
        },
        event.code,
      );
      if (!action) return;
      event.preventDefault();
      event.stopPropagation();
      handleShortcutActionRef.current(action);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [active, bindings]);

  const handleActivityChange = useCallback(
    (paneId: string, activity: ReportedActivity) => {
      const visible = isPaneVisible(paneId);
      const previous = activities[paneId] ?? "unknown";
      const nextState = applyActivityReport(
        {
          ptyState: "running",
          activity: previous,
          donePending: donePending[paneId] ?? false,
        },
        activity,
        visible,
      );
      setDonePending((pending) =>
        pending[paneId] === nextState.donePending
          ? pending
          : { ...pending, [paneId]: nextState.donePending },
      );
      setActivities((current) => {
        if (current[paneId] === activity) return current;
        return { ...current, [paneId]: activity };
      });
      const runtime = runtimes[paneId];
      if (runtime) {
        void host.terminal
          .reportScreenState({
            paneId,
            generation: runtime.generation,
            activity,
          })
          .catch(() => undefined);
      }
    },
    [activities, donePending, isPaneVisible, runtimes],
  );

  const stopPanePty = useCallback((paneId: string) => {
    setRuntimes((current) => {
      const runtime = current[paneId];
      if (
        runtime &&
        (runtime.ptyState === "running" || runtime.ptyState === "starting")
      ) {
        void host.terminal.stop(runtime.terminalId).catch(() => undefined);
      }
      return removePaneRuntime(current, paneId);
    });
  }, []);

  const handleClosePane = useCallback(
    (workspaceId: string, tabId: string, paneId: string) => {
      stopPanePty(paneId);
      closePane(workspaceId, tabId, paneId);
    },
    [closePane, stopPanePty],
  );

  const handleCloseTab = useCallback(
    (workspaceId: string, tabId: string) => {
      const tab = layout.workspaces
        .find((workspace) => workspace.id === workspaceId)
        ?.tabs.find((candidate) => candidate.id === tabId);
      if (tab) {
        for (const paneId of listPaneIds(tab.layout)) stopPanePty(paneId);
      }
      closeTab(workspaceId, tabId);
    },
    [closeTab, layout.workspaces, stopPanePty],
  );

  const handleAddWorkspace = useCallback(async () => {
    const record = await host.workspaces.chooseRoot().catch(() => null);
    if (!record) return;
    setRoots((current) =>
      current && !current.some((root) => root.id === record.id)
        ? [...current, record]
        : current,
    );
    addWorkspaceRoot(record);
    setSelectedWorkspaceId(record.id);
  }, [addWorkspaceRoot]);

  const handleRemoveWorkspace = useCallback(
    async (workspaceId: string) => {
      const workspace = layout.workspaces.find(
        (candidate) => candidate.id === workspaceId,
      );
      const confirmed = window.confirm(
        `Remove "${workspace?.title ?? "this workspace"}" from VINTAGE?\n\nRunning terminals will be stopped. Files on disk will not be deleted.`,
      );
      if (!confirmed) return;

      setWorkspaceActionError(null);
      let nextRoots: WorkspaceRootRecord[];
      try {
        nextRoots = await host.workspaces.removeRoot(workspaceId);
      } catch (error: unknown) {
        const message =
          typeof error === "object" && error !== null && "message" in error
            ? String((error as { message: unknown }).message)
            : "The workspace could not be removed.";
        setWorkspaceActionError(message);
        return;
      }

      if (workspace) {
        for (const tab of workspace.tabs) {
          for (const paneId of listPaneIds(tab.layout)) stopPanePty(paneId);
        }
      }
      removeWorkspaceRoot(workspaceId);
      setRoots(nextRoots);
      setSelectedWorkspaceId((current) =>
        current === workspaceId ? (nextRoots[0]?.id ?? null) : current,
      );
    },
    [layout.workspaces, removeWorkspaceRoot, stopPanePty],
  );

  // Badges roll up from live PTY state; agent activity sources (screen
  // manifests, hooks) join in Phases 6-7 and feed the same aggregation.
  const { paneBadges, tabBadges, errorTabs } = useMemo(() => {
    const paneBadges: Record<string, AgentActivity> = {};
    const tabBadges: Record<string, AgentActivity> = {};
    const errorTabs = new Set<string>();
    for (const workspace of workspaces) {
      for (const tab of workspace.tabs) {
        const visible =
          selectedWorkspace?.id === workspace.id &&
          selectedWorkspace.selectedTabId === tab.id;
        const inputs = tab.panes.map((pane) => {
          const runtime = paneRuntimeOrStopped(runtimes, pane.id);
          const activity = activities[pane.id] ?? "unknown";
          const pending = donePending[pane.id] ?? false;
          paneBadges[pane.id] =
            runtime.ptyState === "error"
              ? "blocked"
              : effectiveActivity(
                  {
                    ptyState: runtime.ptyState,
                    activity,
                    donePending: pending,
                  },
                  visible,
                );
          return {
            ptyState: runtime.ptyState,
            activity,
            donePending: pending,
          };
        });
        const rollup = rollupPaneStatuses(inputs);
        tabBadges[tab.id] = rollup.activity;
        if (rollup.hasPtyError) errorTabs.add(tab.id);
      }
    }
    return { paneBadges, tabBadges, errorTabs };
  }, [workspaces, runtimes, activities, donePending, selectedWorkspace]);

  function renderPaneFor(workspace: WorkspaceState, tabId: string) {
    return (paneId: string) => {
      const tab = workspace.tabs.find((candidate) => candidate.id === tabId);
      const pane = tab?.panes.find((candidate) => candidate.id === paneId);
      if (!tab || !pane) return null;
      const active =
        workspace.id === selectedWorkspace?.id &&
        tab.id === workspace.selectedTabId;
      return (
        <PaneTerminal
          pane={pane}
          workspaceId={workspace.id}
          runtime={paneRuntimeOrStopped(runtimes, paneId)}
          active={active}
          selected={active && tab.selectedPaneId === pane.id}
          appearance={appearance}
          fontFamily={fontFamily}
          fontSize={fontSize}
          onStart={handleStartPane}
          onClose={(id) => handleClosePane(workspace.id, tab.id, id)}
          onSplit={(id, direction) => {
            const newPaneId = splitPane({
              workspaceId: workspace.id,
              tabId: tab.id,
              paneId: id,
              direction,
              defaultShellId,
            });
            if (newPaneId) handleStartPane(newPaneId);
          }}
          onPtyStateChange={handlePtyStateChange}
          onActivityChange={handleActivityChange}
        />
      );
    };
  }

  return (
    <div
      className={`workspace-app ${filesOpen ? "has-files-panel" : ""}`}
      data-appearance={appearance}
      style={
        {
          "--ws-sidebar-width": `${sidebarWidth}px`,
          "--ws-files-width": `${filesWidth}px`,
        } as CSSProperties
      }
    >
      <WorkspaceSidebar
        workspaces={workspaces}
        selectedWorkspaceId={selectedWorkspace?.id ?? null}
        paneBadges={paneBadges}
        tabBadges={tabBadges}
        errorTabs={errorTabs}
        onSelectWorkspace={setSelectedWorkspaceId}
        onSelectPane={(workspaceId, tabId, paneId) => {
          // Viewing a pane acknowledges any pending done badge.
          setDonePending((pending) =>
            pending[paneId] ? { ...pending, [paneId]: false } : pending,
          );
          selectPane(workspaceId, tabId, paneId);
        }}
        onSelectTab={(workspaceId, tabId) => {
          setSelectedWorkspaceId(workspaceId);
          selectTab(workspaceId, tabId);
        }}
        onClosePane={handleClosePane}
        onRemoveWorkspace={(workspaceId) =>
          void handleRemoveWorkspace(workspaceId)
        }
        onAddWorkspace={() => void handleAddWorkspace()}
        onOpenSettings={onOpenSettings}
      />

      <PanelResizer
        orientation="vertical"
        value={sidebarWidth}
        min={220}
        max={420}
        onChange={setSidebarWidth}
      />

      <div className="ws-center">
        {workspaceActionError && (
          <div className="ws-layout-alert" role="alert">
            <strong>Workspace removal failed.</strong> {workspaceActionError}
            <br />
            <button type="button" onClick={() => setWorkspaceActionError(null)}>
              Dismiss
            </button>
          </div>
        )}
        {status === "invalid" && (
          <div className="ws-layout-alert" role="alert">
            <strong>The saved layout could not be used.</strong> {invalidReason}{" "}
            Nothing was overwritten; autosave is paused.
            <br />
            <button type="button" onClick={() => void backupAndResetLayout()}>
              Back up and reset layout
            </button>
          </div>
        )}

        {selectedWorkspace && selectedWorkspace.tabs.length > 0 && (
          <div className="ws-tabbar">
            <WorkspaceTabs
              tabs={selectedWorkspace.tabs}
              selectedTabId={selectedWorkspace.selectedTabId}
              tabBadges={tabBadges}
              errorTabs={errorTabs}
              onSelectTab={(tabId) => selectTab(selectedWorkspace.id, tabId)}
              onCloseTab={(tabId) =>
                handleCloseTab(selectedWorkspace.id, tabId)
              }
              onRenameTab={(tabId, title) =>
                renameTab(selectedWorkspace.id, tabId, title)
              }
              onAddTab={() => handleAddTab(selectedWorkspace.id)}
            />
            <button
              className="ws-files-toggle"
              type="button"
              aria-pressed={filesOpen}
              title={filesOpen ? "Close files panel" : "Open files panel"}
              onClick={() => setFilesOpen((current) => !current)}
            >
              Files
            </button>
          </div>
        )}

        {workspaces.length === 0 ? (
          <div className="ws-empty">
            <span className="ws-empty-kicker">Terminal workspace</span>
            <strong>No workspace open</strong>
            <p>
              Open a project folder to start terminals, split panes, and run
              agents in it.
            </p>
            <button type="button" onClick={() => void handleAddWorkspace()}>
              Open a workspace
            </button>
          </div>
        ) : !selectedWorkspace || selectedWorkspace.tabs.length === 0 ? (
          <div className="ws-empty">
            <span className="ws-empty-kicker">
              {selectedWorkspace?.title ?? "Workspace"}
            </span>
            <strong>No tabs yet</strong>
            <p>Each tab holds a split tree of terminal panes.</p>
            {selectedWorkspace && (
              <button
                type="button"
                onClick={() => handleAddTab(selectedWorkspace.id)}
              >
                New tab
              </button>
            )}
          </div>
        ) : null}

        {workspaces.flatMap((workspace) =>
          workspace.tabs.map((tab) => (
            <div
              key={`${workspace.id}:${tab.id}`}
              className="ws-panes"
              hidden={
                workspace.id !== selectedWorkspace?.id ||
                tab.id !== workspace.selectedTabId
              }
            >
              <SplitPaneView
                layout={tab.layout}
                selectedPaneId={tab.selectedPaneId}
                renderPane={renderPaneFor(workspace, tab.id)}
                onSelectPane={(paneId) =>
                  selectPane(workspace.id, tab.id, paneId)
                }
                onResize={(path: SplitPath, ratio: number) =>
                  resizeDividerByPath(workspace.id, tab.id, path, ratio)
                }
              />
            </div>
          )),
        )}
      </div>

      {filesOpen && selectedWorkspace && (
        <>
          <PanelResizer
            orientation="vertical"
            value={filesWidth}
            reverse
            min={340}
            max={760}
            onChange={setFilesWidth}
          />
          <WorkspaceFilesPanel
            workspace={selectedWorkspace}
            active
            onClose={() => setFilesOpen(false)}
          />
        </>
      )}
    </div>
  );
}

function PanelResizer({
  orientation,
  value,
  min,
  max,
  reverse = false,
  onChange,
}: {
  orientation: "vertical" | "horizontal";
  value: number;
  min: number;
  max: number;
  reverse?: boolean;
  onChange: (value: number) => void;
}) {
  const dragging = useRef<{ pointer: number; base: number } | null>(null);

  function clamp(next: number) {
    return Math.min(max, Math.max(min, next));
  }

  return (
    <div
      className={`ws-panel-resizer ws-panel-resizer-${orientation}`}
      role="separator"
      aria-orientation={orientation === "vertical" ? "vertical" : "horizontal"}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={Math.round(value)}
      tabIndex={0}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.preventDefault();
        event.currentTarget.setPointerCapture(event.pointerId);
        dragging.current = {
          pointer: orientation === "vertical" ? event.clientX : event.clientY,
          base: value,
        };
        document.body.classList.add("is-resizing-ws-panel");
      }}
      onPointerMove={(event) => {
        const start = dragging.current;
        if (!start) return;
        const position =
          orientation === "vertical" ? event.clientX : event.clientY;
        const delta = position - start.pointer;
        onChange(clamp(start.base + delta * (reverse ? -1 : 1)));
      }}
      onPointerUp={(event) => {
        dragging.current = null;
        if (event.currentTarget.hasPointerCapture(event.pointerId)) {
          event.currentTarget.releasePointerCapture(event.pointerId);
        }
        document.body.classList.remove("is-resizing-ws-panel");
      }}
      onPointerCancel={() => {
        dragging.current = null;
        document.body.classList.remove("is-resizing-ws-panel");
      }}
      onKeyDown={(event) => {
        const step = event.shiftKey ? 32 : 12;
        let next: number | null = null;
        if (orientation === "vertical") {
          if (event.key === "ArrowLeft")
            next = value - step * (reverse ? -1 : 1);
          else if (event.key === "ArrowRight")
            next = value + step * (reverse ? -1 : 1);
        } else {
          if (event.key === "ArrowUp") next = value - step;
          else if (event.key === "ArrowDown") next = value + step;
        }
        if (event.key === "Home") next = min;
        else if (event.key === "End") next = max;
        if (next === null) return;
        event.preventDefault();
        onChange(clamp(next));
      }}
    />
  );
}
