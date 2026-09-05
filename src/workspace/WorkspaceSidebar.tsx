/**
 * Left sidebar: workspaces -> tabs -> panes, with attention badges rolled up
 * from pane state. The workspace with the most urgent pane reads as the most
 * urgent workspace (blocked > working > done > idle > unknown).
 */

import { useRef, useState } from "react";
import { Icon } from "../ui/Icon";
import { activityLabel } from "./agentState.ts";
import { LAYOUT_LIMITS } from "./types.ts";
import { listPaneIds } from "./paneLayout.ts";
import type { AgentActivity, WorkspaceState } from "./types.ts";

export interface WorkspaceSidebarProps {
  workspaces: WorkspaceState[];
  selectedWorkspaceId: string | null;
  paneBadges: Record<string, AgentActivity>;
  tabBadges: Record<string, AgentActivity>;
  errorTabs: ReadonlySet<string>;
  /** Native session ids reported by hooks, available in pane tooltips. */
  paneSessionIds: Record<string, string>;
  /** Agent CLI name (codex/claude/opencode) reported by a hook for a pane. */
  paneAgentNames: Record<string, string>;
  onSelectWorkspace: (workspaceId: string) => void;
  onSelectTab: (workspaceId: string, tabId: string) => void;
  onSelectPane: (workspaceId: string, tabId: string, paneId: string) => void;
  onRenamePane: (
    workspaceId: string,
    tabId: string,
    paneId: string,
    title: string,
  ) => void;
  onClosePane: (workspaceId: string, tabId: string, paneId: string) => void;
  onRemoveWorkspace: (workspaceId: string) => void;
  onAddWorkspace: () => void;
  onOpenSettings: () => void;
}

export function WorkspaceSidebar({
  workspaces,
  selectedWorkspaceId,
  paneBadges,
  tabBadges,
  errorTabs,
  paneSessionIds,
  paneAgentNames,
  onSelectWorkspace,
  onSelectTab,
  onSelectPane,
  onRenamePane,
  onClosePane,
  onRemoveWorkspace,
  onAddWorkspace,
  onOpenSettings,
}: WorkspaceSidebarProps) {
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [editingPaneId, setEditingPaneId] = useState<string | null>(null);
  const [draftTitle, setDraftTitle] = useState("");
  const cancellingEdit = useRef(false);

  function beginRenamePane(
    workspaceId: string,
    tabId: string,
    paneId: string,
    currentTitle: string,
  ) {
    cancellingEdit.current = false;
    setDraftTitle(currentTitle);
    setEditingPaneId(paneId);
    onSelectPane(workspaceId, tabId, paneId);
  }

  function commitRenamePane(
    workspaceId: string,
    tabId: string,
    paneId: string,
  ) {
    if (cancellingEdit.current) {
      cancellingEdit.current = false;
      return;
    }
    setEditingPaneId(null);
    const title = draftTitle.trim();
    if (title) onRenamePane(workspaceId, tabId, paneId, title);
  }

  function toggleCollapsed(workspaceId: string) {
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(workspaceId)) next.delete(workspaceId);
      else next.add(workspaceId);
      return next;
    });
  }

  return (
    <nav className="ws-sidebar" aria-label="Workspaces">
      <div className="ws-sidebar-header">
        <h2>Workspaces</h2>
        <div className="ws-sidebar-actions">
          <button
            className="ws-icon-button"
            type="button"
            title="Open a workspace folder"
            aria-label="Open a workspace folder"
            onClick={onAddWorkspace}
          >
            <Icon name="plus" />
          </button>
        </div>
      </div>

      <div className="ws-tree">
        {workspaces.length === 0 && (
          <p
            className="ws-empty-hint"
            style={{ color: "var(--ws-text-dim)", padding: "8px" }}
          >
            No workspaces yet. Use + to open a folder.
          </p>
        )}
        {workspaces.map((workspace) => {
          const selected = workspace.id === selectedWorkspaceId;
          const isCollapsed = collapsed.has(workspace.id);
          const selectedTab = workspace.tabs.find(
            (tab) => tab.id === workspace.selectedTabId,
          );
          return (
            <div className="ws-workspace" key={workspace.id}>
              <div
                className="ws-workspace-row"
                data-selected={selected}
              >
                <button
                  className="ws-icon-button ws-disclosure"
                  type="button"
                  aria-label={`${isCollapsed ? "Expand" : "Collapse"} ${workspace.title}`}
                  aria-expanded={!isCollapsed}
                  onClick={() => toggleCollapsed(workspace.id)}
                >
                  <Icon name="chevron-down" />
                </button>
                <button
                  className="ws-workspace-select ws-row-label"
                  type="button"
                  title={workspace.path}
                  aria-current={selected ? "true" : undefined}
                  onClick={() => onSelectWorkspace(workspace.id)}
                >
                  {workspace.title}
                </button>
                <button
                  className="ws-row-close"
                  type="button"
                  aria-label={`Remove workspace ${workspace.title}`}
                  title="Remove workspace (files are never deleted)"
                  onClick={(event) => {
                    event.stopPropagation();
                    onRemoveWorkspace(workspace.id);
                  }}
                >
                  <Icon name="x" />
                </button>
              </div>

              {!isCollapsed &&
                workspace.tabs.map((tab) => {
                  const paneIds = listPaneIds(tab.layout);
                  return (
                    <div key={tab.id}>
                      <button
                        className="ws-tab-row"
                        type="button"
                        data-selected={
                          selected && tab.id === workspace.selectedTabId
                        }
                        onClick={() => onSelectTab(workspace.id, tab.id)}
                      >
                        <span
                          className="ws-badge"
                          data-activity={tabBadges[tab.id] ?? "unknown"}
                          data-pty={errorTabs.has(tab.id) ? "error" : undefined}
                        />
                        <span className="ws-row-label">{tab.title}</span>
                      </button>
                      {selected &&
                        selectedTab?.id === tab.id &&
                        paneIds.map((paneId) => {
                          const pane = tab.panes.find(
                            (candidate) => candidate.id === paneId,
                          );
                          return (
                            <div
                              key={paneId}
                              className="ws-pane-row"
                              role="button"
                              tabIndex={0}
                              data-selected={tab.selectedPaneId === paneId}
                              title={
                                paneSessionIds[paneId]
                                  ? `Session ${paneSessionIds[paneId]}`
                                  : undefined
                              }
                              onClick={() =>
                                onSelectPane(workspace.id, tab.id, paneId)
                              }
                              onDoubleClick={() =>
                                beginRenamePane(
                                  workspace.id,
                                  tab.id,
                                  paneId,
                                  pane?.title ?? "Terminal",
                                )
                              }
                              onKeyDown={(event) => {
                                if (event.target !== event.currentTarget) return;
                                if (
                                  event.key === "Enter" ||
                                  event.key === " "
                                ) {
                                  event.preventDefault();
                                  onSelectPane(workspace.id, tab.id, paneId);
                                }
                              }}
                            >
                              <span
                                className="ws-badge"
                                data-activity={paneBadges[paneId] ?? "unknown"}
                              />
                              {paneAgentNames[paneId] && (
                                <span
                                  className="ws-pane-agent"
                                  title={`Agent ${paneAgentNames[paneId]}${
                                    paneSessionIds[paneId]
                                      ? ` · session ${paneSessionIds[paneId]}`
                                      : ""
                                  }`}
                                >
                                  {paneAgentNames[paneId]}
                                </span>
                              )}
                              {activityLabel(
                                paneBadges[paneId] ?? "unknown",
                              ) && (
                                <span
                                  className="ws-pane-state"
                                  data-activity={paneBadges[paneId]}
                                >
                                  {activityLabel(
                                    paneBadges[paneId] ?? "unknown",
                                  )}
                                </span>
                              )}
                              {editingPaneId === paneId ? (
                                <input
                                  className="ws-pane-title-input"
                                  value={draftTitle}
                                  aria-label={`Rename pane ${pane?.title ?? paneId}`}
                                  autoFocus
                                  maxLength={LAYOUT_LIMITS.maxTitleCodePoints}
                                  onChange={(event) =>
                                    setDraftTitle(
                                      Array.from(event.target.value)
                                        .slice(
                                          0,
                                          LAYOUT_LIMITS.maxTitleCodePoints,
                                        )
                                        .join(""),
                                    )
                                  }
                                  onBlur={() =>
                                    commitRenamePane(
                                      workspace.id,
                                      tab.id,
                                      paneId,
                                    )
                                  }
                                  onKeyDown={(event) => {
                                    if (event.key === "Enter") {
                                      event.preventDefault();
                                      event.currentTarget.blur();
                                    } else if (event.key === "Escape") {
                                      event.preventDefault();
                                      cancellingEdit.current = true;
                                      setEditingPaneId(null);
                                    }
                                  }}
                                  onClick={(event) => event.stopPropagation()}
                                />
                              ) : (
                                <span className="ws-row-label">
                                  {paneAgentNames[paneId] &&
                                  pane?.title === "Terminal"
                                    ? ""
                                    : (pane?.title ?? "Terminal")}
                                </span>
                              )}
                              <button
                                className="ws-row-close"
                                type="button"
                                aria-label={`Close pane ${pane?.title ?? paneId}`}
                                onClick={(event) => {
                                  event.stopPropagation();
                                  onClosePane(workspace.id, tab.id, paneId);
                                }}
                              >
                                <Icon name="x" />
                              </button>
                            </div>
                          );
                        })}
                    </div>
                  );
                })}
            </div>
          );
        })}
      </div>

      <div className="ws-sidebar-footer">
        <button type="button" onClick={onOpenSettings}>
          <Icon name="sliders" />
          Settings
        </button>
      </div>
    </nav>
  );
}
