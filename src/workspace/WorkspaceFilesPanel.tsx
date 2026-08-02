/**
 * Right panel for the terminal workspace: Files tree and File preview.
 *
 * Closing the panel unmounts the explorer, which tears down its file watch —
 * the "Files panel closed stops watching" lifecycle rule.
 */

import { useState } from "react";
import { FileExplorer } from "../FileExplorer.tsx";
import { ErrorBoundary } from "../ui/ErrorBoundary.tsx";
import type { WorkspaceState } from "./types.ts";

export type FilesPanelTab = "files" | "preview";

export interface WorkspaceFilesPanelProps {
  workspace: WorkspaceState;
  active: boolean;
  onClose: () => void;
}

export function WorkspaceFilesPanel({
  workspace,
  active,
  onClose,
}: WorkspaceFilesPanelProps) {
  const [tab, setTab] = useState<FilesPanelTab>("files");

  return (
    <aside
      className="ws-files-panel"
      aria-label="Workspace files"
      hidden={!active}
    >
      <div
        className="ws-files-tabs"
        role="tablist"
        aria-label="Files panel views"
      >
        <div className="ws-files-tab-rail">
          <button
            className="ws-files-tab"
            type="button"
            role="tab"
            aria-selected={tab === "files"}
            onClick={() => setTab("files")}
          >
            Files
          </button>
          <button
            className="ws-files-tab"
            type="button"
            role="tab"
            aria-selected={tab === "preview"}
            onClick={() => setTab("preview")}
          >
            Preview
          </button>
        </div>
        <button
          className="ws-icon-button"
          type="button"
          aria-label="Close files panel"
          title="Close files panel"
          onClick={onClose}
        >
          ✕
        </button>
      </div>

      <div className="ws-files-content">
        {tab === "files" && (
          <ErrorBoundary
            fallback={(error, onReset) => (
              <div className="ws-files-error" role="alert">
                <strong>Files could not be shown.</strong>
                {error.message && <p>{error.message}</p>}
                <button type="button" onClick={onReset}>
                  Reload
                </button>
              </div>
            )}
          >
            <FileExplorer
              active={active}
              workspaceId={workspace.id}
              workspaceTitle={workspace.title}
            />
          </ErrorBoundary>
        )}
        {tab === "preview" && (
          <div className="ws-files-preview-empty">
            <strong>File preview</strong>
            <p>Select a file in the Files tab to preview it here.</p>
          </div>
        )}
      </div>
    </aside>
  );
}
