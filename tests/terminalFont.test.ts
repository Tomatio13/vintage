import assert from "node:assert/strict";
import test from "node:test";
import {
  clampTerminalFontSize,
  DEFAULT_TERMINAL_FONT_FAMILY,
  DEFAULT_TERMINAL_FONT_PRESET_ID,
  DEFAULT_TERMINAL_FONT_SIZE,
  MAX_TERMINAL_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
  parseTerminalFontSettings,
  resolveTerminalFontFamily,
  TERMINAL_FONT_PRESETS,
  type TerminalFontSettings,
} from "../src/terminalFont.ts";

function settings(
  overrides: Partial<TerminalFontSettings> = {},
): TerminalFontSettings {
  return {
    preset: "default",
    family: "",
    size: 12,
    ...overrides,
  };
}

test("clampTerminalFontSize rounds and clamps to the supported range", () => {
  assert.equal(clampTerminalFontSize(12), 12);
  assert.equal(clampTerminalFontSize(13.6), 14);
  assert.equal(clampTerminalFontSize(2), MIN_TERMINAL_FONT_SIZE);
  assert.equal(clampTerminalFontSize(999), MAX_TERMINAL_FONT_SIZE);
  assert.equal(clampTerminalFontSize(-5), MIN_TERMINAL_FONT_SIZE);
});

test("clampTerminalFontSize falls back for non-finite values", () => {
  assert.equal(clampTerminalFontSize(Number.NaN), DEFAULT_TERMINAL_FONT_SIZE);
  assert.equal(
    clampTerminalFontSize(Number.POSITIVE_INFINITY),
    DEFAULT_TERMINAL_FONT_SIZE,
  );
});

test("presets cover default plus the user-requested fonts and custom", () => {
  const ids = TERMINAL_FONT_PRESETS.map((preset) => preset.id);
  assert.ok(ids.includes("default"));
  assert.ok(ids.includes("cica"));
  assert.ok(ids.includes("hackgen"));
  assert.ok(ids.includes("hackgen35"));
  assert.ok(ids.includes("nerd"));
  assert.ok(ids.includes("custom"));
  // Non-custom presets always end with a mono fallback.
  for (const preset of TERMINAL_FONT_PRESETS) {
    if (preset.id === "custom") continue;
    assert.match(preset.family, /monospace$/);
  }
});

test("resolveTerminalFontFamily maps presets to families", () => {
  const cica = TERMINAL_FONT_PRESETS.find((preset) => preset.id === "cica");
  assert.ok(cica);
  assert.equal(
    resolveTerminalFontFamily(settings({ preset: "cica", family: "" })),
    cica.family,
  );
});

test("resolveTerminalFontFamily uses custom family with mono fallback", () => {
  assert.equal(
    resolveTerminalFontFamily(
      settings({ preset: "custom", family: "JetBrainsMono Nerd Font" }),
    ),
    '"JetBrainsMono Nerd Font", monospace',
  );
});

test("resolveTerminalFontFamily falls back to default for empty or unknown", () => {
  assert.equal(
    resolveTerminalFontFamily(settings({ preset: "custom", family: "  " })),
    DEFAULT_TERMINAL_FONT_FAMILY,
  );
  assert.equal(
    resolveTerminalFontFamily(settings({ preset: "does-not-exist" })),
    DEFAULT_TERMINAL_FONT_FAMILY,
  );
  // A custom family that already includes monospace is used as-is.
  assert.equal(
    resolveTerminalFontFamily(
      settings({ preset: "custom", family: "Foo, monospace" }),
    ),
    "Foo, monospace",
  );
});

test("parseTerminalFontSettings falls back for invalid input", () => {
  assert.deepEqual(parseTerminalFontSettings("not json"), settings());
  assert.deepEqual(parseTerminalFontSettings("{"), settings());
  assert.deepEqual(parseTerminalFontSettings("null"), settings());
  assert.deepEqual(parseTerminalFontSettings('"string"'), settings());
});

test("parseTerminalFontSettings restores a valid custom settings object", () => {
  const parsed = parseTerminalFontSettings(
    JSON.stringify({ preset: "hackgen", family: "", size: 14 }),
  );
  assert.deepEqual(parsed, settings({ preset: "hackgen", size: 14 }));
});

test("parseTerminalFontSettings clamps size and ignores unknown presets", () => {
  const parsed = parseTerminalFontSettings(
    JSON.stringify({ preset: "bogus", family: "X", size: 999 }),
  );
  assert.equal(parsed.preset, DEFAULT_TERMINAL_FONT_PRESET_ID);
  assert.equal(parsed.size, MAX_TERMINAL_FONT_SIZE);
  assert.equal(parsed.family, "");
});

test("parseTerminalFontSettings normalizes a custom preset with no family", () => {
  const parsed = parseTerminalFontSettings(
    JSON.stringify({ preset: "custom", family: "   ", size: 13 }),
  );
  // Falls back to the default preset so nothing renders the system font.
  assert.equal(parsed.preset, DEFAULT_TERMINAL_FONT_PRESET_ID);
  assert.equal(parsed.size, 13);
});

test("parseTerminalFontSettings keeps custom family trimmed and bounded", () => {
  const parsed = parseTerminalFontSettings(
    JSON.stringify({ preset: "custom", family: "  HackGen Nerd  ", size: 12 }),
  );
  assert.equal(parsed.preset, "custom");
  assert.equal(parsed.family, "HackGen Nerd");
});
