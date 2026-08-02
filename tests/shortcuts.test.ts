import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_SHORTCUT_BINDINGS,
  findDuplicateBinding,
  formatShortcutKey,
  matchShortcut,
  parseShortcutBindings,
  rebindShortcut,
  serializeShortcutBindings,
  type Modifiers,
} from "../src/workspace/shortcuts.ts";

function mods(overrides: Partial<Modifiers> = {}): Modifiers {
  return {
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    metaKey: false,
    ...overrides,
  };
}

test("every default binding matches its own code and modifiers", () => {
  for (const binding of DEFAULT_SHORTCUT_BINDINGS) {
    const action = matchShortcut(
      DEFAULT_SHORTCUT_BINDINGS,
      mods({
        ctrlKey: binding.ctrl,
        altKey: binding.alt,
        shiftKey: binding.shift,
      }),
      binding.code,
    );
    assert.equal(action, binding.action, `binding for ${binding.code}`);
  }
});

test("every default binding is a distinct chord", () => {
  const chords = DEFAULT_SHORTCUT_BINDINGS.map(
    (binding) =>
      `${binding.code}|${binding.ctrl}|${binding.alt}|${binding.shift}`,
  );
  assert.equal(new Set(chords).size, chords.length);
});

test("tab switching uses horizontal arrow chords", () => {
  assert.equal(
    matchShortcut(
      DEFAULT_SHORTCUT_BINDINGS,
      mods({ ctrlKey: true, shiftKey: true }),
      "ArrowRight",
    ),
    "tabNext",
  );
  assert.equal(
    matchShortcut(
      DEFAULT_SHORTCUT_BINDINGS,
      mods({ ctrlKey: true, shiftKey: true }),
      "ArrowLeft",
    ),
    "tabPrevious",
  );
});

test("pane uses vertical and workspace uses Alt+Shift chords", () => {
  assert.equal(
    matchShortcut(
      DEFAULT_SHORTCUT_BINDINGS,
      mods({ ctrlKey: true, shiftKey: true }),
      "ArrowDown",
    ),
    "paneNext",
  );
  assert.equal(
    matchShortcut(
      DEFAULT_SHORTCUT_BINDINGS,
      mods({ ctrlKey: true, shiftKey: true }),
      "ArrowUp",
    ),
    "panePrevious",
  );
  assert.equal(
    matchShortcut(
      DEFAULT_SHORTCUT_BINDINGS,
      mods({ altKey: true, shiftKey: true }),
      "ArrowRight",
    ),
    "workspaceNext",
  );
  assert.equal(
    matchShortcut(
      DEFAULT_SHORTCUT_BINDINGS,
      mods({ altKey: true, shiftKey: true }),
      "ArrowLeft",
    ),
    "workspacePrevious",
  );
});

test("custom bindings override defaults at match time", () => {
  const custom = rebindShortcut(DEFAULT_SHORTCUT_BINDINGS, "tabNext", {
    code: "KeyT",
    ctrl: true,
    alt: false,
    shift: false,
  });
  assert.ok(custom);
  assert.equal(
    matchShortcut(custom, mods({ ctrlKey: true }), "KeyT"),
    "tabNext",
  );
  // The old default chord no longer triggers tabNext.
  assert.notEqual(
    matchShortcut(
      custom,
      mods({ ctrlKey: true, shiftKey: true }),
      "ArrowRight",
    ),
    "tabNext",
  );
});

test("meta key disables every default shortcut", () => {
  for (const binding of DEFAULT_SHORTCUT_BINDINGS) {
    const action = matchShortcut(
      DEFAULT_SHORTCUT_BINDINGS,
      mods({
        ctrlKey: binding.ctrl,
        altKey: binding.alt,
        shiftKey: binding.shift,
        metaKey: true,
      }),
      binding.code,
    );
    assert.equal(action, null);
  }
});

test("missing modifiers are never a shortcut", () => {
  assert.equal(
    matchShortcut(DEFAULT_SHORTCUT_BINDINGS, mods(), "ArrowLeft"),
    null,
  );
  assert.equal(
    matchShortcut(
      DEFAULT_SHORTCUT_BINDINGS,
      mods({ shiftKey: true }),
      "ArrowRight",
    ),
    null,
  );
  assert.equal(
    matchShortcut(
      DEFAULT_SHORTCUT_BINDINGS,
      mods({ altKey: true }),
      "ArrowRight",
    ),
    null,
  );
});

test("unrelated keys and codes return null", () => {
  assert.equal(
    matchShortcut(DEFAULT_SHORTCUT_BINDINGS, mods({ ctrlKey: true }), "KeyA"),
    null,
  );
  assert.equal(
    matchShortcut(DEFAULT_SHORTCUT_BINDINGS, mods({ ctrlKey: true }), "PageUp"),
    null,
  );
  assert.equal(
    matchShortcut(DEFAULT_SHORTCUT_BINDINGS, mods({ ctrlKey: true }), ""),
    null,
  );
});

test("rebindShortcut replaces the target action and rejects duplicates", () => {
  const next = rebindShortcut(DEFAULT_SHORTCUT_BINDINGS, "paneNext", {
    code: "KeyJ",
    ctrl: true,
    alt: false,
    shift: false,
  });
  assert.ok(next);
  assert.equal(
    next.find((binding) => binding.action === "paneNext")?.code,
    "KeyJ",
  );
  // tabNext's chord is already taken by the default table.
  const duplicate = rebindShortcut(DEFAULT_SHORTCUT_BINDINGS, "paneNext", {
    code: "ArrowRight",
    ctrl: true,
    alt: false,
    shift: true,
  });
  assert.equal(duplicate, null);
});

test("findDuplicateBinding reports another action using the same chord", () => {
  const tabNext = DEFAULT_SHORTCUT_BINDINGS.find(
    (binding) => binding.action === "tabNext",
  );
  assert.ok(tabNext);
  const duplicate = findDuplicateBinding(
    DEFAULT_SHORTCUT_BINDINGS,
    "paneNext",
    {
      code: tabNext.code,
      ctrl: tabNext.ctrl,
      alt: tabNext.alt,
      shift: tabNext.shift,
    },
  );
  assert.equal(duplicate?.action, "tabNext");
  // Rebinding an action to its own chord is never a conflict.
  const self = findDuplicateBinding(DEFAULT_SHORTCUT_BINDINGS, "tabNext", {
    code: tabNext.code,
    ctrl: tabNext.ctrl,
    alt: tabNext.alt,
    shift: tabNext.shift,
  });
  assert.equal(self, null);
});

test("serialize and parse round-trip custom bindings", () => {
  const custom = rebindShortcut(DEFAULT_SHORTCUT_BINDINGS, "tabNext", {
    code: "KeyT",
    ctrl: true,
    alt: false,
    shift: false,
  });
  assert.ok(custom);
  const text = serializeShortcutBindings(custom);
  const parsed = parseShortcutBindings(text);
  assert.deepEqual(parsed, custom);
});

test("parse falls back to defaults for invalid or empty input", () => {
  assert.deepEqual(parseShortcutBindings("not json"), [
    ...DEFAULT_SHORTCUT_BINDINGS,
  ]);
  assert.deepEqual(parseShortcutBindings("{"), [...DEFAULT_SHORTCUT_BINDINGS]);
  assert.deepEqual(parseShortcutBindings("null"), [
    ...DEFAULT_SHORTCUT_BINDINGS,
  ]);
  assert.deepEqual(parseShortcutBindings('"string"'), [
    ...DEFAULT_SHORTCUT_BINDINGS,
  ]);
});

test("parse drops invalid entries and keeps defaults for untouched actions", () => {
  const parsed = parseShortcutBindings(
    JSON.stringify([
      { action: "bogus", code: "KeyA", ctrl: true, alt: false, shift: false },
      { action: "tabNext", code: "", ctrl: true, alt: false, shift: false },
      { action: "tabNext", code: "KeyT", ctrl: true, alt: false, shift: false },
      { action: "tabNext", code: "KeyY", ctrl: true, alt: false, shift: false },
    ]),
  );
  // First valid tabNext entry wins; the later KeyY entry is dropped as a
  // repeated action.
  const tabNext = parsed.find((binding) => binding.action === "tabNext");
  assert.equal(tabNext?.code, "KeyT");
  assert.equal(
    parsed.filter((binding) => binding.action === "tabNext").length,
    1,
  );
  // Untouched actions keep their defaults.
  assert.equal(
    parsed.find((binding) => binding.action === "paneNext")?.code,
    "ArrowDown",
  );
});

test("parse normalizes chord flags and rejects non-boolean flags", () => {
  const parsed = parseShortcutBindings(
    JSON.stringify([
      { action: "tabNext", code: "KeyT", ctrl: true, alt: false, shift: false },
      {
        action: "tabPrevious",
        code: "KeyQ",
        ctrl: 1,
        alt: false,
        shift: false,
      },
    ]),
  );
  assert.equal(
    parsed.find((binding) => binding.action === "tabNext")?.code,
    "KeyT",
  );
  // Invalid flags → that entry dropped → tabPrevious falls back to default.
  assert.equal(
    parsed.find((binding) => binding.action === "tabPrevious")?.code,
    "ArrowLeft",
  );
});

test("formatShortcutKey renders chords readably", () => {
  assert.equal(
    formatShortcutKey({
      code: "ArrowLeft",
      ctrl: true,
      alt: false,
      shift: true,
    }),
    "Ctrl+Shift+Left",
  );
  assert.equal(
    formatShortcutKey({ code: "KeyT", ctrl: true, alt: false, shift: false }),
    "Ctrl+T",
  );
  assert.equal(
    formatShortcutKey({ code: "Digit5", ctrl: false, alt: true, shift: false }),
    "Alt+5",
  );
});
