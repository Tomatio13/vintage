/**
 * Left sidebar: workspaces -> tabs -> panes, with attention badges rolled up
 * from pane state. The workspace with the most urgent pane reads as the most
 * urgent workspace (blocked > working > done > idle > unknown).
 */

import { useState } from "react";
import { listPaneIds } from "./paneLayout.ts";
import type { AgentActivity, WorkspaceState } from "./types.ts";

export interface WorkspaceSidebarProps {
  workspaces: WorkspaceState[];
  selectedWorkspaceId: string | null;
  paneBadges: Record<string, AgentActivity>;
  tabBadges: Record<string, AgentActivity>;
  errorTabs: ReadonlySet<string>;
  onSelectWorkspace: (workspaceId: string) => void;
  onSelectTab: (workspaceId: string, tabId: string) => void;
  onSelectPane: (workspaceId: string, tabId: string, paneId: string) => void;
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
  onSelectWorkspace,
  onSelectTab,
  onSelectPane,
  onClosePane,
  onRemoveWorkspace,
  onAddWorkspace,
  onOpenSettings,
}: WorkspaceSidebarProps) {
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());

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
            +
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
                role="button"
                tabIndex={0}
                data-selected={selected}
                title={workspace.path}
                onClick={() => onSelectWorkspace(workspace.id)}
                onDoubleClick={() => toggleCollapsed(workspace.id)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    onSelectWorkspace(workspace.id);
                  }
                }}
              >
                <span aria-hidden="true">{isCollapsed ? "▸" : "▾"}</span>
                <span className="ws-row-label">{workspace.title}</span>
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
                  ✕
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
                              onClick={() =>
                                onSelectPane(workspace.id, tab.id, paneId)
                              }
                              onKeyDown={(event) => {
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
                              <span className="ws-row-label">
                                {pane?.title ?? "Terminal"}
                              </span>
                              <button
                                className="ws-row-close"
                                type="button"
                                aria-label={`Close pane ${pane?.title ?? paneId}`}
                                onClick={(event) => {
                                  event.stopPropagation();
                                  onClosePane(workspace.id, tab.id, paneId);
                                }}
                              >
                                ✕
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
          Settings
        </button>
      </div>
    </nav>
  );
}
