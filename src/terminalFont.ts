/**
 * Terminal font family and font size (px) settings.
 *
 * Follows the same localStorage pattern as appearance.ts / fontScale.ts: the
 * pure parse/clamp/resolve logic lives here so it is testable without a
 * browser, and `useTerminalFont` owns the storage read/write and React state.
 *
 * Font size is specified in pixels and applies to the terminal only. The UI
 * scale (--app-font-scale) is independent and keeps controlling the rest of
 * the app.
 */

import { useCallback, useState } from "react";

export const TERMINAL_FONT_STORAGE_KEY = "vintage.terminal.font";

export const DEFAULT_TERMINAL_FONT_SIZE = 12;
export const MIN_TERMINAL_FONT_SIZE = 8;
export const MAX_TERMINAL_FONT_SIZE = 48;
export const TERMINAL_FONT_SIZE_STEP = 1;

/** System default mono stack, mirrors App.css `--mono`. */
export const DEFAULT_TERMINAL_FONT_FAMILY =
  '"SFMono-Regular", "SF Mono", Menlo, Consolas, monospace';

/** Monospace fallback appended to user/preset families. */
const MONO_FALLBACK = "monospace";

export interface TerminalFontPreset {
  /** Stable id persisted to storage; "custom" means a free-form family. */
  id: string;
  label: string;
  family: string;
}

export const TERMINAL_FONT_PRESETS: readonly TerminalFontPreset[] = [
  { id: "default", label: "Default", family: DEFAULT_TERMINAL_FONT_FAMILY },
  { id: "cica", label: "Cica", family: "Cica, monospace" },
  { id: "hackgen", label: "HackGen", family: "HackGen, monospace" },
  { id: "hackgen35", label: "HackGen35", family: "HackGen35, monospace" },
  {
    id: "nerd",
    label: "JetBrains Mono Nerd Font",
    family: '"JetBrainsMono Nerd Font", monospace',
  },
  { id: "custom", label: "Custom", family: "" },
];

export const DEFAULT_TERMINAL_FONT_PRESET_ID = "default";

export interface TerminalFontSettings {
  /** Selected preset id; "custom" stores the free-form family. */
  preset: string;
  /** Free-form font family; only meaningful when preset === "custom". */
  family: string;
  size: number;
}

export function defaultTerminalFontSettings(): TerminalFontSettings {
  return {
    preset: DEFAULT_TERMINAL_FONT_PRESET_ID,
    family: "",
    size: DEFAULT_TERMINAL_FONT_SIZE,
  };
}

/** Clamps a px font size to the supported range; non-finite → default. */
export function clampTerminalFontSize(size: number): number {
  if (!Number.isFinite(size)) return DEFAULT_TERMINAL_FONT_SIZE;
  return Math.min(
    MAX_TERMINAL_FONT_SIZE,
    Math.max(MIN_TERMINAL_FONT_SIZE, Math.round(size)),
  );
}

/**
 * Quotes a custom font name for CSS unless it is already a quoted string or a
 * single CSS identifier. Multi-word names ("JetBrainsMono Nerd Font") must be
 * quoted to be parsed as one font family.
 */
function quoteFamilyName(family: string): string {
  const trimmed = family.trim();
  if (!trimmed) return trimmed;
  if (trimmed.startsWith('"') || trimmed.startsWith("'")) return trimmed;
  const singleToken = /^[A-Za-z0-9_-]+$/.test(trimmed);
  return singleToken ? trimmed : `"${trimmed}"`;
}

/** The font-family string passed to xterm, with a mono fallback. */
export function resolveTerminalFontFamily(
  settings: TerminalFontSettings,
): string {
  const preset = TERMINAL_FONT_PRESETS.find(
    (candidate) => candidate.id === settings.preset,
  );
  if (settings.preset === "custom") {
    const raw = settings.family.trim();
    if (!raw) return DEFAULT_TERMINAL_FONT_FAMILY;
    // A complete font-family list (already containing a fallback) is used
    // verbatim; a bare family name is quoted when needed and gets a fallback.
    if (raw.includes("monospace")) return raw;
    const family = quoteFamilyName(raw);
    return `${family}, ${MONO_FALLBACK}`;
  }
  if (!preset || !preset.family) return DEFAULT_TERMINAL_FONT_FAMILY;
  return preset.family;
}

/**
 * Parses untrusted persisted text back into a settings object. Any invalid or
 * out-of-range field falls back to its default; a custom preset without a
 * usable family also falls back to the default preset.
 */
export function parseTerminalFontSettings(
  text: string,
  defaults: TerminalFontSettings = defaultTerminalFontSettings(),
): TerminalFontSettings {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return { ...defaults };
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return { ...defaults };
  }
  const value = parsed as Record<string, unknown>;

  const presetRaw = value.preset;
  const presetIsKnown =
    typeof presetRaw === "string" &&
    TERMINAL_FONT_PRESETS.some((candidate) => candidate.id === presetRaw);
  const preset = presetIsKnown ? (presetRaw as string) : defaults.preset;

  let family = defaults.family;
  if (presetIsKnown && typeof value.family === "string") {
    const trimmed = value.family.slice(0, 256).trim();
    if (trimmed) family = trimmed;
  }

  let size = defaults.size;
  if (typeof value.size === "number" && Number.isFinite(value.size)) {
    size = clampTerminalFontSize(value.size);
  }

  const settings: TerminalFontSettings = { preset, family, size };
  // A custom preset with no usable family would render the system default
  // anyway; normalize it to the default preset for a cleaner persisted state.
  if (settings.preset === "custom" && !settings.family) {
    settings.preset = DEFAULT_TERMINAL_FONT_PRESET_ID;
  }
  return settings;
}

function storedSettings(): TerminalFontSettings {
  try {
    const text = window.localStorage.getItem(TERMINAL_FONT_STORAGE_KEY);
    return text === null
      ? defaultTerminalFontSettings()
      : parseTerminalFontSettings(text);
  } catch {
    return defaultTerminalFontSettings();
  }
}

function persistSettings(settings: TerminalFontSettings) {
  try {
    window.localStorage.setItem(
      TERMINAL_FONT_STORAGE_KEY,
      JSON.stringify(settings),
    );
  } catch {
    // The active settings still apply when persistent storage is unavailable.
  }
}

export function useTerminalFont() {
  const [settings, setSettings] =
    useState<TerminalFontSettings>(storedSettings);

  const setFontSize = useCallback((size: number) => {
    setSettings((current) => {
      const next = { ...current, size: clampTerminalFontSize(size) };
      persistSettings(next);
      return next;
    });
  }, []);

  /** Selects a preset; when preset is "custom", `family` is the font name. */
  const setFontFamily = useCallback((preset: string, family = "") => {
    setSettings((current) => {
      const next: TerminalFontSettings = {
        preset,
        family: preset === "custom" ? family.trim() : "",
        size: current.size,
      };
      persistSettings(next);
      return next;
    });
  }, []);

  return {
    settings,
    setFontSize,
    setFontFamily,
  };
}
