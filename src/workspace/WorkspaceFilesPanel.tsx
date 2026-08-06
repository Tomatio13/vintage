/**
 * Right panel for the terminal workspace: Files tree and File preview.
 *
 * Closing the panel unmounts the explorer, which tears down its file watch —
 * the "Files panel closed stops watching" lifecycle rule.
 */

import { FileExplorer } from "../FileExplorer.tsx";
import { ErrorBoundary } from "../ui/ErrorBoundary.tsx";
import type { WorkspaceState } from "./types.ts";

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
  return (
    <aside
      className="ws-files-panel"
      aria-label="Workspace files"
      hidden={!active}
    >
      <div
        className="ws-files-tabs"
        aria-label="Workspace files"
      >
        <span className="ws-files-tab-label">Files</span>
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
      </div>
    </aside>
  );
}
