/**
 * One pane's terminal: owns the xterm surface for the current PTY generation
 * and the stopped/exited/error overlay with explicit restart controls.
 *
 * A restored pane has no runtime record and shows as stopped; starting it
 * mints a fresh generation through `onStart`. Session resume joins the
 * restart menu in Phase 6, when agent launch definitions land.
 */

import { useEffect, useRef, useState } from "react";
import type { ResolvedAppearance } from "../appearance";
import {
  TerminalSurface,
  type TerminalStatus,
} from "../terminal/TerminalSurface";
import type { AgentActivity, TerminalInfo } from "../host/types";
import type { PaneRuntime } from "./paneRuntime.ts";
import type { PaneDefinition, PtyState } from "./types.ts";

export interface PaneTerminalProps {
  pane: PaneDefinition;
  workspaceId: string;
  runtime: PaneRuntime;
  active: boolean;
  /** This pane is the selected pane of the active tab; it owns xterm focus. */
  selected: boolean;
  appearance: ResolvedAppearance;
  onStart: (paneId: string) => void;
  onClose: (paneId: string) => void;
  onSplit: (paneId: string, direction: "horizontal" | "vertical") => void;
  onPtyStateChange: (
    paneId: string,
    state: PtyState,
    detail: string | null,
  ) => void;
  /** Screen-derived agent activity to report to the host and roll up. */
  onActivityChange: (
    paneId: string,
    activity: Exclude<AgentActivity, "done">,
  ) => void;
}

const STATUS_TO_PTY: Record<TerminalStatus, PtyState> = {
  starting: "starting",
  running: "running",
  exited: "exited",
  error: "error",
};

export function PaneTerminal({
  pane,
  workspaceId,
  runtime,
  active,
  selected,
  appearance,
  onStart,
  onClose,
  onSplit,
  onPtyStateChange,
  onActivityChange,
}: PaneTerminalProps) {
  const [info, setInfo] = useState<TerminalInfo | null>(null);
  const live =
    runtime.ptyState === "starting" || runtime.ptyState === "running";
  const shellLabel = info?.shell.label ?? pane.shellId;

  // Focus follows keyboard selection: when this pane becomes the selected one
  // (via shortcuts or sidebar), move xterm focus to it. The initial mount of a
  // selected pane is skipped — the terminal surface focuses itself on start.
  const prevSelected = useRef(selected);
  const [focusRequest, setFocusRequest] = useState(0);
  useEffect(() => {
    if (selected && !prevSelected.current) {
      setFocusRequest((current) => current + 1);
    }
    prevSelected.current = selected;
  }, [selected]);

  return (
    <>
      <div className="ws-pane-toolbar">
        <strong>{pane.title}</strong>
        <span className="ws-pane-shell" title={shellLabel}>
          {shellLabel}
        </span>
        <div className="ws-pane-split-buttons">
          <button
            className="ws-mini-button"
            type="button"
            title="Split right"
            aria-label={`Split ${pane.title} to the right`}
            onClick={() => onSplit(pane.id, "horizontal")}
          >
            ◫
          </button>
          <button
            className="ws-mini-button"
            type="button"
            title="Split below"
            aria-label={`Split ${pane.title} below`}
            onClick={() => onSplit(pane.id, "vertical")}
          >
            ⬒
          </button>
          <button
            className="ws-mini-button"
            type="button"
            title="Close pane"
            aria-label={`Close ${pane.title}`}
            onClick={() => onClose(pane.id)}
          >
            ✕
          </button>
        </div>
      </div>

      <div className="ws-pane-terminal">
        {live && (
          <TerminalSurface
            key={runtime.terminalId}
            active={active}
            appearance={appearance}
            panelOpen
            workingDirectory={null}
            restartToken={runtime.generation}
            focusRequest={focusRequest}
            paneLaunch={{
              terminalId: runtime.terminalId,
              paneId: pane.id,
              generation: runtime.generation,
              workspaceId,
              launch: pane.launch,
              attachToExisting: runtime.ptyState === "running",
            }}
            onInfoChange={setInfo}
            onStatusChange={(status, message) =>
              onPtyStateChange(pane.id, STATUS_TO_PTY[status], message ?? null)
            }
            onScreenState={(activity) => onActivityChange(pane.id, activity)}
          />
        )}
      </div>

      {!live && (
        <div className="ws-pane-status" role="status">
          <strong>
            {runtime.ptyState === "error"
              ? "Terminal unavailable"
              : runtime.ptyState === "exited"
                ? "Terminal finished"
                : "Terminal stopped"}
          </strong>
          {runtime.detail && <p>{runtime.detail}</p>}
          <button type="button" onClick={() => onStart(pane.id)}>
            {runtime.ptyState === "stopped" || runtime.generation < 0
              ? "Start terminal"
              : "Start a new terminal"}
          </button>
        </div>
      )}
    </>
  );
}
