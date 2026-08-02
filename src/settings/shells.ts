/**
 * Default shell preference for new terminals.
 *
 * Follows the same localStorage pattern as appearance.ts / fontScale.ts /
 * terminalFont.ts: a pure parser for stored values plus a small hook that owns
 * the storage read/write and React state. The stored value is a shell id from
 * the host's detected shell list (e.g. "windows-git-bash"); it is resolved
 * against live detection at use time so an uninstalled shell falls back to the
 * automatic default instead of launching nothing.
 */

import { useCallback, useState } from "react";

export const DEFAULT_SHELL_STORAGE_KEY = "vintage.default-shell";

/** Valid shell ids are short, printable, bounded identifiers. */
const SHELL_ID_PATTERN = /^[A-Za-z0-9_.-]{1,64}$/;

/**
 * Parses a stored shell id. Returns null for invalid or empty input, meaning
 * "use the host's automatic default".
 */
export function parsePreferredShellId(value: string | null): string | null {
  if (value === null) return null;
  const trimmed = value.trim();
  if (!SHELL_ID_PATTERN.test(trimmed)) return null;
  return trimmed;
}

/**
 * Picks the shell to use for new terminals: the user's preferred id when it
 * is available on this machine, then the platform default, then the first
 * available shell, then null.
 */
export function resolvePreferredShellId(
  preferredId: string | null,
  shells: readonly { id: string; available: boolean }[],
): string | null {
  if (preferredId !== null) {
    const match = shells.find(
      (shell) => shell.id === preferredId && shell.available,
    );
    if (match) return match.id;
  }
  const platformDefault = shells.find(
    (shell) =>
      shell.available &&
      (shell.id === "windows-default" || shell.id === "unix-default"),
  );
  if (platformDefault) return platformDefault.id;
  const first = shells.find((shell) => shell.available);
  return first ? first.id : null;
}

function storedPreferredShellId(): string | null {
  try {
    return parsePreferredShellId(
      window.localStorage.getItem(DEFAULT_SHELL_STORAGE_KEY),
    );
  } catch {
    return null;
  }
}

export function useDefaultShell() {
  const [preferredShellId, setPreferredShellIdState] = useState<string | null>(
    storedPreferredShellId,
  );

  const setPreferredShellId = useCallback((id: string) => {
    setPreferredShellIdState(id);
    try {
      window.localStorage.setItem(DEFAULT_SHELL_STORAGE_KEY, id);
    } catch {
      // The active preference still applies when storage is unavailable.
    }
  }, []);

  return { preferredShellId, setPreferredShellId };
}
