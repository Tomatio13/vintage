import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import type { ResolvedAppearance } from "../appearance";
import { host } from "../host";
import type { AgentActivity, TerminalInfo } from "../host/types";
import { isDesktopHost } from "../shared/platform";
import {
  buildLogicalLines,
  detectScreenActivity,
} from "../workspace/screenDetection.ts";
import type { PaneLaunchSpec } from "../workspace/types.ts";

export type TerminalStatus = "starting" | "running" | "exited" | "error";

/** Workspace-pane launch contract; when null the legacy path is used. */
export interface PaneLaunchRequest {
  terminalId: string;
  paneId: string;
  generation: number;
  workspaceId: string;
  launch: PaneLaunchSpec;
  /** Reconnect after a split-tree remount without restarting the host PTY. */
  attachToExisting: boolean;
}

const LIGHT_TERMINAL_THEME = {
  background: "#f4f8ff",
  foreground: "#1d2b42",
  cursor: "#006edc",
  cursorAccent: "#f4f8ff",
  selectionBackground: "#75b9ff66",
  black: "#142033",
  red: "#b94736",
  green: "#0b8f66",
  yellow: "#8a6515",
  blue: "#246fbe",
  magenta: "#a13bb4",
  cyan: "#087f9e",
  white: "#e5edf8",
  brightBlack: "#7186a2",
  brightRed: "#d15d49",
  brightGreen: "#00a77a",
  brightYellow: "#a77b1c",
  brightBlue: "#328ce8",
  brightMagenta: "#c457da",
  brightCyan: "#00a8c8",
  brightWhite: "#ffffff",
};

const DARK_TERMINAL_THEME = {
  background: "#050914",
  foreground: "#d9e8ff",
  cursor: "#42c8ff",
  cursorAccent: "#050914",
  selectionBackground: "#165b8f99",
  black: "#08101d",
  red: "#ff7e68",
  green: "#5ee6a8",
  yellow: "#e6c56f",
  blue: "#56a8ff",
  magenta: "#f778ff",
  cyan: "#42d9ff",
  white: "#d9e8ff",
  brightBlack: "#60789a",
  brightRed: "#ff9b89",
  brightGreen: "#8ff0c4",
  brightYellow: "#f2d98f",
  brightBlue: "#8fc7ff",
  brightMagenta: "#ffadff",
  brightCyan: "#8be9ff",
  brightWhite: "#f4f8ff",
};

function terminalTheme(appearance: ResolvedAppearance) {
  return appearance === "dark" ? DARK_TERMINAL_THEME : LIGHT_TERMINAL_THEME;
}

export function TerminalSurface({
  active,
  appearance,
  panelOpen,
  workingDirectory,
  paneLaunch,
  restartToken,
  focusRequest,
  fontFamily,
  fontSize,
  onInfoChange,
  onStatusChange,
  onScreenState,
}: {
  active: boolean;
  appearance: ResolvedAppearance;
  panelOpen: boolean;
  workingDirectory: string | null;
  paneLaunch?: PaneLaunchRequest | null;
  restartToken: number;
  /** Bumped by the owner when keyboard selection should move focus here. */
  focusRequest?: number;
  /** CSS font-family stack for the terminal surface. */
  fontFamily: string;
  /** Terminal font size in pixels. */
  fontSize: number;
  onInfoChange: (info: TerminalInfo | null) => void;
  onStatusChange: (status: TerminalStatus, message?: string) => void;
  /** Live bottom-buffer screen state, debounced ~120ms after output. */
  onScreenState?: (activity: Exclude<AgentActivity, "done">) => void;
}) {
  const container = useRef<HTMLDivElement | null>(null);
  const terminalInstance = useRef<Terminal | null>(null);
  const panelOpenRef = useRef(panelOpen);
  const paneLaunchRef = useRef<PaneLaunchRequest | null>(paneLaunch ?? null);
  paneLaunchRef.current = paneLaunch ?? null;
  const onScreenStateRef = useRef(onScreenState);
  onScreenStateRef.current = onScreenState;
  const requestFit = useRef<(() => void) | null>(null);
  const [renderReady, setRenderReady] = useState(false);

  useLayoutEffect(() => {
    panelOpenRef.current = panelOpen;
    if (!panelOpen) {
      setRenderReady(false);
      return;
    }

    const animationFrame = window.requestAnimationFrame(() =>
      requestFit.current?.(),
    );
    return () => window.cancelAnimationFrame(animationFrame);
  }, [panelOpen]);

  // Re-fit when the pane becomes visible again (tab switch, unhide). The
  // hidden tab has zero height, so fit runs when it is shown.
  useLayoutEffect(() => {
    if (!active || !panelOpen) return;
    const animationFrame = window.requestAnimationFrame(() =>
      requestFit.current?.(),
    );
    return () => window.cancelAnimationFrame(animationFrame);
  }, [active, panelOpen]);

  useEffect(() => {
    if (!active || !panelOpen || !renderReady) return;
    const animationFrame = window.requestAnimationFrame(() =>
      terminalInstance.current?.focus(),
    );
    return () => window.cancelAnimationFrame(animationFrame);
  }, [active, panelOpen, renderReady]);

  // A bump in focusRequest means keyboard selection moved onto this pane;
  // reclaim xterm focus once the surface is ready so subsequent keystrokes
  // reach this PTY instead of the one that was focused before.
  useEffect(() => {
    if (focusRequest === undefined || focusRequest === 0) return;
    if (!active || !panelOpen || !renderReady) return;
    const animationFrame = window.requestAnimationFrame(() =>
      terminalInstance.current?.focus(),
    );
    return () => window.cancelAnimationFrame(animationFrame);
  }, [focusRequest, active, panelOpen, renderReady]);

  useEffect(() => {
    if (terminalInstance.current)
      terminalInstance.current.options.theme = terminalTheme(appearance);
  }, [appearance]);

  // Apply font family/size changes to a live terminal. Changing the family
  // alters glyph widths, so the character grid must be re-measured: re-run fit
  // after the option takes effect, otherwise xterm keeps the old cell size and
  // the new font only shows once the pane is resized or remounted.
  useEffect(() => {
    const terminal = terminalInstance.current;
    if (!terminal) return;
    let changed = false;
    if (terminal.options.fontFamily !== fontFamily) {
      terminal.options.fontFamily = fontFamily;
      changed = true;
    }
    if (terminal.options.fontSize !== fontSize) {
      terminal.options.fontSize = fontSize;
      changed = true;
    }
    if (changed) requestFit.current?.();
  }, [fontFamily, fontSize]);

  useEffect(() => {
    const target = container.current;
    if (!target) return;

    let active = true;
    let started = false;
    let startRequested = false;
    const launchAtMount = paneLaunchRef.current;
    const attachToExisting = launchAtMount?.attachToExisting ?? false;
    let exited = false;
    let resizeFrame = 0;
    let resizeTimer = 0;
    let screenTimer = 0;
    const terminalId =
      launchAtMount?.terminalId ?? `terminal-${crypto.randomUUID()}`;
    const running = { current: false };
    const unlisteners: Array<() => void> = [];
    const encoder = new TextEncoder();
    let writeQueue = Promise.resolve();
    const terminal = new Terminal({
      allowProposedApi: false,
      cursorBlink: true,
      cursorStyle: "bar",
      cursorWidth: 1,
      drawBoldTextInBrightColors: false,
      fontFamily,
      fontSize,
      // Standard weights (400/700): static fonts like Cica and HackGen only
      // ship Regular and Bold, so non-standard values (e.g. 430) make the
      // browser fall back to the default font on Windows.
      fontWeight: 400,
      fontWeightBold: 700,
      letterSpacing: 0.15,
      lineHeight: 1.22,
      minimumContrastRatio: 4.5,
      scrollback: 5000,
      theme: terminalTheme(appearance),
    });
    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(target);
    terminalInstance.current = terminal;

    const fit = () => {
      window.cancelAnimationFrame(resizeFrame);
      resizeFrame = window.requestAnimationFrame(() => {
        if (!active || target.clientWidth === 0 || target.clientHeight === 0)
          return;
        try {
          const dimensions = fitAddon.proposeDimensions();
          if (!dimensions || dimensions.cols < 20 || dimensions.rows < 2)
            return;
          if (
            dimensions.cols !== terminal.cols ||
            dimensions.rows !== terminal.rows
          ) {
            terminal.resize(dimensions.cols, dimensions.rows);
          }
          if (panelOpenRef.current) {
            setRenderReady(true);
            if (!startRequested) void start();
          }
        } catch {
          // xterm can be between layout and disposal while the pane is closing.
        }
      });
    };
    const scheduleFit = () => {
      window.clearTimeout(resizeTimer);
      // The panel animates its width. Fitting on every animation frame makes
      // interactive shells redraw their prompt into scrollback repeatedly.
      resizeTimer = window.setTimeout(fit, 90);
    };
    requestFit.current = scheduleFit;

    const enqueueInput = (data: number[]) => {
      if (!running.current || data.length === 0) return;
      writeQueue = writeQueue
        .then(() => host.terminal.write(terminalId, data))
        .catch(() => undefined);
    };
    const dataSubscription = terminal.onData((data) =>
      enqueueInput(Array.from(encoder.encode(data))),
    );
    const binarySubscription = terminal.onBinary((data) => {
      enqueueInput(
        Array.from(data, (character) => character.charCodeAt(0) & 0xff),
      );
    });
    const resizeSubscription = terminal.onResize(({ cols, rows }) => {
      if (!running.current) return;
      void host.terminal.resize(terminalId, cols, rows).catch(() => undefined);
    });

    onInfoChange(null);
    if (!attachToExisting) onStatusChange("starting");

    const start = async () => {
      if (startRequested) return;
      startRequested = true;
      if (!isDesktopHost()) {
        onStatusChange(
          "error",
          "The terminal is available in the VINTAGE desktop app.",
        );
        return;
      }

      const reportScreen = () => {
        const launch = paneLaunchRef.current;
        if (!launch || launch.launch.type !== "agent") return;
        const agent = launch.launch.preset;
        const buffer = terminal.buffer.active;
        const start = Math.max(0, buffer.length - 80);
        const rows: string[] = [];
        for (let index = start; index < buffer.length; index += 1) {
          const line = buffer.getLine(index);
          if (line) rows.push(line.translateToString(true));
        }
        const snapshot = buildLogicalLines(
          rows.map((text) => ({ text, newline: true })),
        );
        const opts = terminal.options as Record<string, unknown>;
        const title =
          typeof opts.windowsPty === "object" &&
          opts.windowsPty !== null &&
          typeof (opts.windowsPty as { title?: unknown }).title === "string"
            ? (opts.windowsPty as { title: string }).title
            : null;
        const match = detectScreenActivity(agent, {
          lines: snapshot,
          title,
          progress: null,
        });
        if (match && match.state !== "done") {
          onScreenStateRef.current?.(match.state);
        }
      };
      const scheduleScreenReport = () => {
        window.clearTimeout(screenTimer);
        screenTimer = window.setTimeout(reportScreen, 120);
      };

      try {
        const listeners = await Promise.all([
          host.terminal.onOutput((payload) => {
            if (payload.terminalId === terminalId) {
              terminal.write(Uint8Array.from(payload.data));
              scheduleScreenReport();
            }
          }),
          host.terminal.onExit((payload) => {
            if (payload.terminalId !== terminalId || !active) return;
            exited = true;
            running.current = false;
            const detail = payload.signal
              ? `Shell stopped (${payload.signal}).`
              : payload.exitCode === null || payload.exitCode === 0
                ? "Shell exited."
                : `Shell exited with code ${payload.exitCode}.`;
            onStatusChange("exited", detail);
          }),
        ]);
        if (!active) {
          listeners.forEach((unlisten) => unlisten());
          return;
        }
        unlisteners.push(...listeners);

        if (attachToExisting) {
          try {
            await host.terminal.resize(
              terminalId,
              Math.max(2, terminal.cols),
              Math.max(2, terminal.rows),
            );
            if (!active) return;
            running.current = true;
            onStatusChange("running");
            terminal.focus();
          } catch (error) {
            if (active) onStatusChange("error", String(error));
          }
          return;
        }

        const launchRequest = launchAtMount;
        const info = await host.terminal.start(
          launchRequest
            ? {
                terminalId,
                paneId: launchRequest.paneId,
                generation: launchRequest.generation,
                workspaceId: launchRequest.workspaceId,
                launch: launchRequest.launch,
                cols: Math.max(2, terminal.cols),
                rows: Math.max(2, terminal.rows),
              }
            : {
                terminalId,
                workingDirectory,
                cols: Math.max(2, terminal.cols),
                rows: Math.max(2, terminal.rows),
              },
        );
        started = true;
        if (!active) {
          void host.terminal.stop(terminalId).catch(() => undefined);
          return;
        }
        onInfoChange(info);
        if (exited) return;
        running.current = true;
        onStatusChange("running");
        terminal.focus();
      } catch (error) {
        if (active) onStatusChange("error", String(error));
      }
    };
    const resizeObserver = new ResizeObserver(scheduleFit);
    resizeObserver.observe(target);
    scheduleFit();

    return () => {
      active = false;
      running.current = false;
      requestFit.current = null;
      window.clearTimeout(resizeTimer);
      window.clearTimeout(screenTimer);
      window.cancelAnimationFrame(resizeFrame);
      resizeObserver.disconnect();
      dataSubscription.dispose();
      binarySubscription.dispose();
      resizeSubscription.dispose();
      unlisteners.forEach((unlisten) => unlisten());
      // Workspace PTYs outlive transient split-tree remounts. Their owner stops
      // them on explicit close or when the workspace root unmounts.
      if (started && !launchAtMount)
        void host.terminal.stop(terminalId).catch(() => undefined);
      if (terminalInstance.current === terminal)
        terminalInstance.current = null;
      terminal.dispose();
    };
  }, [restartToken]);

  return (
    <div
      className="terminal-surface"
      ref={container}
      data-render-ready={renderReady}
      aria-label="Interactive terminal"
    />
  );
}
