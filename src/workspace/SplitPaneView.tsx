/**
 * Recursive renderer for the pane split tree.
 *
 * Direction convention: "horizontal" lays children out in a row (divider is
 * a vertical bar), "vertical" stacks them in a column. Divider dragging
 * reports the structural path of its split node so same-direction nesting
 * stays unambiguous; keyboard resizing steps the same divider.
 */

import {
  useRef,
  type KeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import type { PaneLayout, SplitDirection } from "./types.ts";
import type { SplitPath } from "./paneLayout.ts";

const KEYBOARD_RATIO_STEP = 0.05;
const KEYBOARD_RATIO_FINE_STEP = 0.02;

export interface SplitPaneViewProps {
  layout: PaneLayout;
  selectedPaneId: string;
  renderPane: (paneId: string) => ReactNode;
  onSelectPane: (paneId: string) => void;
  onResize: (path: SplitPath, ratio: number) => void;
}

export function SplitPaneView({
  layout,
  selectedPaneId,
  renderPane,
  onSelectPane,
  onResize,
}: SplitPaneViewProps) {
  return (
    <SplitNode
      layout={layout}
      path={[]}
      selectedPaneId={selectedPaneId}
      renderPane={renderPane}
      onSelectPane={onSelectPane}
      onResize={onResize}
    />
  );
}

interface SplitNodeProps extends SplitPaneViewProps {
  path: SplitPath;
}

function SplitNode({
  layout,
  path,
  selectedPaneId,
  renderPane,
  onSelectPane,
  onResize,
}: SplitNodeProps) {
  if (layout.type === "leaf") {
    const selected = layout.paneId === selectedPaneId;
    return (
      <div
        className="ws-pane"
        data-selected={selected}
        onPointerDownCapture={() => {
          if (!selected) onSelectPane(layout.paneId);
        }}
      >
        {renderPane(layout.paneId)}
      </div>
    );
  }

  return (
    <div className={`ws-split ws-split-${layout.direction}`}>
      <div className="ws-split-child" style={{ flexGrow: layout.ratio }}>
        <SplitNode
          layout={layout.first}
          path={[...path, "first"]}
          selectedPaneId={selectedPaneId}
          renderPane={renderPane}
          onSelectPane={onSelectPane}
          onResize={onResize}
        />
      </div>
      <Divider
        direction={layout.direction}
        ratio={layout.ratio}
        onResize={(ratio) => onResize(path, ratio)}
      />
      <div className="ws-split-child" style={{ flexGrow: 1 - layout.ratio }}>
        <SplitNode
          layout={layout.second}
          path={[...path, "second"]}
          selectedPaneId={selectedPaneId}
          renderPane={renderPane}
          onSelectPane={onSelectPane}
          onResize={onResize}
        />
      </div>
    </div>
  );
}

function Divider({
  direction,
  ratio,
  onResize,
}: {
  direction: SplitDirection;
  ratio: number;
  onResize: (ratio: number) => void;
}) {
  const dividerRef = useRef<HTMLDivElement | null>(null);

  function ratioFromPointer(clientX: number, clientY: number): number | null {
    const element = dividerRef.current?.parentElement;
    if (!element) return null;
    const rect = element.getBoundingClientRect();
    if (direction === "horizontal") {
      if (rect.width <= 0) return null;
      return (clientX - rect.left) / rect.width;
    }
    if (rect.height <= 0) return null;
    return (clientY - rect.top) / rect.height;
  }

  function handlePointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    document.body.classList.add("is-resizing-ws-divider");
  }

  function handlePointerMove(event: ReactPointerEvent<HTMLDivElement>) {
    if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
    const next = ratioFromPointer(event.clientX, event.clientY);
    if (next !== null) onResize(next);
  }

  function finishPointer(event: ReactPointerEvent<HTMLDivElement>) {
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    document.body.classList.remove("is-resizing-ws-divider");
  }

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const step = event.shiftKey
      ? KEYBOARD_RATIO_FINE_STEP
      : KEYBOARD_RATIO_STEP;
    const positiveKey = direction === "horizontal" ? "ArrowRight" : "ArrowDown";
    const negativeKey = direction === "horizontal" ? "ArrowLeft" : "ArrowUp";
    let next: number | null = null;
    if (event.key === positiveKey) next = ratio + step;
    else if (event.key === negativeKey) next = ratio - step;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = 1;
    if (next === null) return;
    event.preventDefault();
    onResize(next);
  }

  return (
    <div
      ref={dividerRef}
      className={`ws-divider ws-divider-${direction}`}
      role="separator"
      aria-orientation={direction === "horizontal" ? "vertical" : "horizontal"}
      aria-valuenow={Math.round(ratio * 100)}
      aria-valuemin={20}
      aria-valuemax={80}
      tabIndex={0}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={finishPointer}
      onPointerCancel={finishPointer}
      onKeyDown={handleKeyDown}
    />
  );
}
