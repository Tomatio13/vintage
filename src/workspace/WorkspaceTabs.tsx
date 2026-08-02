/**
 * Tab bar for the selected workspace. One tab owns one split tree; closing a
 * tab stops its panes first (handled by the parent) before removal.
 */

import { useRef, useState } from "react";
import {
  LAYOUT_LIMITS,
  type AgentActivity,
  type AgentTabState,
} from "./types.ts";

export interface WorkspaceTabsProps {
  tabs: AgentTabState[];
  selectedTabId: string;
  tabBadges: Record<string, AgentActivity>;
  errorTabs: ReadonlySet<string>;
  onSelectTab: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onRenameTab: (tabId: string, title: string) => void;
  onAddTab: () => void;
}

export function WorkspaceTabs({
  tabs,
  selectedTabId,
  tabBadges,
  errorTabs,
  onSelectTab,
  onCloseTab,
  onRenameTab,
  onAddTab,
}: WorkspaceTabsProps) {
  const [editingTabId, setEditingTabId] = useState<string | null>(null);
  const [draftTitle, setDraftTitle] = useState("");
  const cancellingEdit = useRef(false);

  function beginRename(tab: AgentTabState) {
    cancellingEdit.current = false;
    setDraftTitle(tab.title);
    setEditingTabId(tab.id);
    onSelectTab(tab.id);
  }

  function commitRename(tab: AgentTabState) {
    if (cancellingEdit.current) {
      cancellingEdit.current = false;
      return;
    }
    setEditingTabId(null);
    const title = draftTitle.trim();
    if (title) onRenameTab(tab.id, title);
  }

  return (
    <div className="ws-tabs" role="tablist" aria-label="Workspace tabs">
      {tabs.map((tab) => {
        const selected = tab.id === selectedTabId;
        return (
          <div
            key={tab.id}
            className="ws-tab"
            data-selected={selected}
            role="presentation"
          >
            {editingTabId === tab.id ? (
              <div className="ws-tab-editor">
                <span
                  className="ws-badge"
                  data-activity={tabBadges[tab.id] ?? "unknown"}
                  data-pty={errorTabs.has(tab.id) ? "error" : undefined}
                />
                <input
                  className="ws-tab-title-input"
                  value={draftTitle}
                  aria-label={`Rename tab ${tab.title}`}
                  autoFocus
                  onChange={(event) =>
                    setDraftTitle(
                      Array.from(event.target.value)
                        .slice(0, LAYOUT_LIMITS.maxTitleCodePoints)
                        .join(""),
                    )
                  }
                  onBlur={() => commitRename(tab)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      event.currentTarget.blur();
                    } else if (event.key === "Escape") {
                      event.preventDefault();
                      cancellingEdit.current = true;
                      setEditingTabId(null);
                    }
                  }}
                />
              </div>
            ) : (
              <button
                className="ws-tab-trigger"
                type="button"
                role="tab"
                data-selected={selected}
                aria-selected={selected}
                onClick={() => onSelectTab(tab.id)}
                onDoubleClick={() => beginRename(tab)}
                onKeyDown={(event) => {
                  if (event.key === "F2") {
                    event.preventDefault();
                    beginRename(tab);
                  }
                }}
                title="Double-click to rename"
              >
                <span
                  className="ws-badge"
                  data-activity={tabBadges[tab.id] ?? "unknown"}
                  data-pty={errorTabs.has(tab.id) ? "error" : undefined}
                />
                <span className="ws-tab-title">{tab.title}</span>
              </button>
            )}
            <button
              className="ws-tab-close"
              type="button"
              aria-label={`Close tab ${tab.title}`}
              onClick={() => onCloseTab(tab.id)}
            >
              ✕
            </button>
          </div>
        );
      })}
      <button
        className="ws-tab-add"
        type="button"
        title="New tab"
        aria-label="New tab"
        onClick={onAddTab}
      >
        +
      </button>
    </div>
  );
}
