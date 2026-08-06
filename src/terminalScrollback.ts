/**
 * Persisted terminal scrollback capacity.
 *
 * Keeping fewer lines lowers memory use for every live terminal, which is
 * especially useful on lower-spec computers running multiple panes.
 */

import { useCallback, useState } from "react";

export const TERMINAL_SCROLLBACK_STORAGE_KEY = "vintage.terminal.scrollback";
export const TERMINAL_SCROLLBACK_OPTIONS = [1000, 2500, 5000, 10000] as const;
export const DEFAULT_TERMINAL_SCROLLBACK = TERMINAL_SCROLLBACK_OPTIONS[0];

export function parseTerminalScrollback(value: string | null): number {
  if (value === null) return DEFAULT_TERMINAL_SCROLLBACK;
  const parsed = Number(value);
  return TERMINAL_SCROLLBACK_OPTIONS.includes(
    parsed as 1000 | 2500 | 5000 | 10000,
  )
    ? parsed
    : DEFAULT_TERMINAL_SCROLLBACK;
}

function storedTerminalScrollback(): number {
  try {
    return parseTerminalScrollback(
      window.localStorage.getItem(TERMINAL_SCROLLBACK_STORAGE_KEY),
    );
  } catch {
    return DEFAULT_TERMINAL_SCROLLBACK;
  }
}

export function useTerminalScrollback() {
  const [scrollback, setScrollbackState] = useState(storedTerminalScrollback);

  const setScrollback = useCallback((value: number) => {
    const next = parseTerminalScrollback(String(value));
    setScrollbackState(next);
    try {
      window.localStorage.setItem(
        TERMINAL_SCROLLBACK_STORAGE_KEY,
        String(next),
      );
    } catch {
      // The active setting still applies when persistent storage is unavailable.
    }
  }, []);

  return { scrollback, setScrollback };
}
