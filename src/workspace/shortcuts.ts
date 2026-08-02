/**
 * Keyboard shortcuts for moving between tabs, panes and workspaces, with
 * support for user-customized bindings persisted to localStorage.
 *
 * This module is DOM- and React-free so its pure functions can be unit tested
 * with the Node test runner. The bindings table is passed explicitly into
 * `matchShortcut` instead of being read from a module constant, so the current
 * set of bindings can be changed at runtime (settings screen) and reused for
 * persistence.
 *
 * Matching is based on KeyboardEvent.code (physical layout) rather than
 * `event.key`, so `Ctrl+Shift+Left` stays `ArrowLeft` even on layouts where
 * shifted characters differ. Any Meta key makes a binding inactive so
 * Cmd-based chords on macOS never collide with these.
 *
 * Linux notes (WebKitGTK + GNOME, verified on Ubuntu via gsettings): the
 * webview swallows browser-style tab chords (`Ctrl+Shift+]`, `Ctrl+PageDown`,
 * `Ctrl+Tab`) before the page sees them, and the desktop shell owns every
 * `Ctrl+Alt+<arrow>` chord (with or without Shift) for workspace switching and
 * window moves. So Ctrl-only bindings stay on the arrows for tabs and panes
 * (`Ctrl+Shift+Left/Right` = tab, `Ctrl+Shift+Up/Down` = pane), while
 * workspaces use `Alt+Shift+Left/Right`, which GNOME leaves free.
 */

/** A shortcut action that can be triggered by a chord. */
export const SHORTCUT_ACTIONS = [
  "tabPrevious",
  "tabNext",
  "panePrevious",
  "paneNext",
  "workspacePrevious",
  "workspaceNext",
] as const;

export type ShortcutAction = (typeof SHORTCUT_ACTIONS)[number];

/** Subset of KeyboardEvent modifier flags used for matching. */
export interface Modifiers {
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

/** The physical key + modifiers a user assigns to an action. */
export interface ShortcutKey {
  code: string;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
}

export interface ShortcutBinding extends ShortcutKey {
  action: ShortcutAction;
}

/**
 * Ordered table of default bindings. Each chord appears once; the first match
 * wins, but no code + modifier combination repeats by design.
 */
export const DEFAULT_SHORTCUT_BINDINGS: readonly ShortcutBinding[] = [
  {
    action: "tabPrevious",
    code: "ArrowLeft",
    ctrl: true,
    alt: false,
    shift: true,
  },
  {
    action: "tabNext",
    code: "ArrowRight",
    ctrl: true,
    alt: false,
    shift: true,
  },
  {
    action: "panePrevious",
    code: "ArrowUp",
    ctrl: true,
    alt: false,
    shift: true,
  },
  {
    action: "paneNext",
    code: "ArrowDown",
    ctrl: true,
    alt: false,
    shift: true,
  },
  {
    action: "workspacePrevious",
    code: "ArrowLeft",
    ctrl: false,
    alt: true,
    shift: true,
  },
  {
    action: "workspaceNext",
    code: "ArrowRight",
    ctrl: false,
    alt: true,
    shift: true,
  },
];

/** Human-facing labels for each action, in display order. */
export const SHORTCUT_ACTION_LABELS: Record<ShortcutAction, string> = {
  tabPrevious: "Previous tab",
  tabNext: "Next tab",
  panePrevious: "Previous pane",
  paneNext: "Next pane",
  workspacePrevious: "Previous workspace",
  workspaceNext: "Next workspace",
};

/**
 * Resolves a physical key + modifiers to a shortcut action against the given
 * bindings, or null. A Meta chord never matches, so Cmd-based shortcuts on
 * macOS cannot collide with these.
 */
export function matchShortcut(
  bindings: readonly ShortcutBinding[],
  mods: Modifiers,
  code: string,
): ShortcutAction | null {
  if (mods.metaKey) return null;
  for (const binding of bindings) {
    if (
      code === binding.code &&
      mods.ctrlKey === binding.ctrl &&
      mods.altKey === binding.alt &&
      mods.shiftKey === binding.shift
    ) {
      return binding.action;
    }
  }
  return null;
}

/**
 * Returns the binding (other than `action`) that already uses the same chord,
 * or null when the chord is free. Used to reject duplicate assignments.
 */
export function findDuplicateBinding(
  bindings: readonly ShortcutBinding[],
  action: ShortcutAction,
  key: ShortcutKey,
): ShortcutBinding | null {
  for (const binding of bindings) {
    if (binding.action === action) continue;
    if (
      binding.code === key.code &&
      binding.ctrl === key.ctrl &&
      binding.alt === key.alt &&
      binding.shift === key.shift
    ) {
      return binding;
    }
  }
  return null;
}

/**
 * Replaces the binding for `action` with `key` and returns a new array, or
 * null when `key` is already used by another action.
 */
export function rebindShortcut(
  bindings: readonly ShortcutBinding[],
  action: ShortcutAction,
  key: ShortcutKey,
): ShortcutBinding[] | null {
  const duplicate = findDuplicateBinding(bindings, action, key);
  if (duplicate) return null;
  return bindings.map((binding) =>
    binding.action === action ? { ...binding, ...key } : binding,
  );
}

/**
 * Serializes bindings to the JSON text persisted in localStorage. Each entry
 * stores the action plus the chord; the compact shape is stable for migration.
 */
export function serializeShortcutBindings(
  bindings: readonly ShortcutBinding[],
): string {
  return JSON.stringify(
    bindings.map(({ action, code, ctrl, alt, shift }) => ({
      action,
      code,
      ctrl,
      alt,
      shift,
    })),
  );
}

/**
 * Parses untrusted persisted text back into a full bindings table. Entries
 * with an unknown action, a repeated action, missing/invalid chord fields, or
 * a duplicate chord are dropped (first occurrence wins); the remaining
 * entries are merged over the defaults so every action always has a binding.
 */
export function parseShortcutBindings(
  text: string,
  defaults: readonly ShortcutBinding[] = DEFAULT_SHORTCUT_BINDINGS,
): ShortcutBinding[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return [...defaults];
  }
  if (!Array.isArray(parsed)) return [...defaults];

  const knownActions = new Set<ShortcutAction>(
    defaults.map((binding) => binding.action),
  );
  const seenChords = new Set<string>();
  const result: ShortcutBinding[] = [];
  const touched = new Set<ShortcutAction>();

  for (const item of parsed) {
    if (typeof item !== "object" || item === null) continue;
    const candidate = item as Record<string, unknown>;
    const action = candidate.action;
    if (
      typeof action !== "string" ||
      !knownActions.has(action as ShortcutAction)
    ) {
      continue;
    }
    if (touched.has(action as ShortcutAction)) continue;
    const binding = normalizeBinding(action as ShortcutAction, candidate);
    if (binding === null) continue;
    const chord = chordKey(binding);
    if (seenChords.has(chord)) continue;
    seenChords.add(chord);
    result.push(binding);
    touched.add(binding.action);
  }

  // Every action keeps a binding, falling back to defaults for anything the
  // persisted data did not provide (or provided invalidly).
  for (const binding of defaults) {
    if (!touched.has(binding.action)) result.push(binding);
  }

  return result;
}

function normalizeBinding(
  action: ShortcutAction,
  value: Record<string, unknown>,
): ShortcutBinding | null {
  const code = value.code;
  if (typeof code !== "string" || code.length === 0 || code.length > 128) {
    return null;
  }
  const ctrl = coerceFlag(value.ctrl);
  const alt = coerceFlag(value.alt);
  const shift = coerceFlag(value.shift);
  if (ctrl === null || alt === null || shift === null) return null;
  return { action, code, ctrl, alt, shift };
}

function coerceFlag(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

function chordKey(binding: ShortcutBinding): string {
  return `${binding.code}|${binding.ctrl}|${binding.alt}|${binding.shift}`;
}

/** Short display names for codes that would otherwise be cryptic. */
const KEY_LABELS: Record<string, string> = {
  ArrowLeft: "Left",
  ArrowRight: "Right",
  ArrowUp: "Up",
  ArrowDown: "Down",
  PageUp: "PageUp",
  PageDown: "PageDown",
  Home: "Home",
  End: "End",
  Enter: "Enter",
  Tab: "Tab",
  Space: "Space",
  Backspace: "Backspace",
  Escape: "Esc",
  BracketLeft: "[",
  BracketRight: "]",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backslash: "\\",
  Minus: "-",
  Equal: "=",
};

function keyLabel(code: string): string {
  if (KEY_LABELS[code]) return KEY_LABELS[code];
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  return code;
}

/** Renders a chord for the settings UI, e.g. "Ctrl+Shift+Left". */
export function formatShortcutKey(key: ShortcutKey): string {
  const parts: string[] = [];
  if (key.ctrl) parts.push("Ctrl");
  if (key.alt) parts.push("Alt");
  if (key.shift) parts.push("Shift");
  parts.push(keyLabel(key.code));
  return parts.join("+");
}
