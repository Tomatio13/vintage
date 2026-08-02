/**
 * React binding for user-customized keyboard shortcuts.
 *
 * Follows the same localStorage pattern as appearance.ts and fontScale.ts:
 * the pure parse/serialize/match logic lives in shortcuts.ts (testable without
 * a browser), while this hook owns the storage read/write and React state.
 */

import { useCallback, useState } from "react";
import {
  DEFAULT_SHORTCUT_BINDINGS,
  parseShortcutBindings,
  rebindShortcut,
  serializeShortcutBindings,
  type ShortcutAction,
  type ShortcutBinding,
  type ShortcutKey,
} from "../workspace/shortcuts.ts";

export const KEYBINDINGS_STORAGE_KEY = "vintage.keybindings";

function storedBindings(): ShortcutBinding[] {
  try {
    const text = window.localStorage.getItem(KEYBINDINGS_STORAGE_KEY);
    return text === null
      ? [...DEFAULT_SHORTCUT_BINDINGS]
      : parseShortcutBindings(text);
  } catch {
    return [...DEFAULT_SHORTCUT_BINDINGS];
  }
}

function persistBindings(bindings: readonly ShortcutBinding[]) {
  try {
    window.localStorage.setItem(
      KEYBINDINGS_STORAGE_KEY,
      serializeShortcutBindings(bindings),
    );
  } catch {
    // The active bindings still apply when persistent storage is unavailable.
  }
}

export function useKeybindings() {
  const [bindings, setBindings] = useState<ShortcutBinding[]>(storedBindings);

  /**
   * Assigns `key` to `action`. Returns false when the chord is already used by
   * another action, leaving the current bindings untouched.
   */
  const bind = useCallback(
    (action: ShortcutAction, key: ShortcutKey): boolean => {
      const next = rebindShortcut(bindings, action, key);
      if (next === null) return false;
      persistBindings(next);
      setBindings(next);
      return true;
    },
    [bindings],
  );

  const resetAll = useCallback(() => {
    const next = [...DEFAULT_SHORTCUT_BINDINGS];
    persistBindings(next);
    setBindings(next);
  }, []);

  return { bindings, bind, resetAll };
}
