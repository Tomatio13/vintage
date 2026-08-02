import { useCallback, useLayoutEffect, useState } from "react";

export const FONT_SCALE_KEY = "vintage.ui.font-scale";
export const DEFAULT_FONT_SCALE = 1;
export const MIN_FONT_SCALE = 0.85;
export const MAX_FONT_SCALE = 2;
export const FONT_SCALE_STEP = 0.05;

export function clampFontScale(scale: number) {
  if (!Number.isFinite(scale)) return DEFAULT_FONT_SCALE;
  const stepped = Math.round(scale / FONT_SCALE_STEP) * FONT_SCALE_STEP;
  return Math.min(MAX_FONT_SCALE, Math.max(MIN_FONT_SCALE, Number(stepped.toFixed(2))));
}

export function formatFontScale(scale: number) {
  return `${Math.round(clampFontScale(scale) * 100)}%`;
}

function applyFontScale(scale: number) {
  const next = clampFontScale(scale);
  document.documentElement.style.setProperty("--app-font-scale", String(next));
  return next;
}

function storedFontScale() {
  try {
    const scale = Number(window.localStorage.getItem(FONT_SCALE_KEY));
    return Number.isFinite(scale) && scale > 0 ? clampFontScale(scale) : DEFAULT_FONT_SCALE;
  } catch {
    return DEFAULT_FONT_SCALE;
  }
}

/** Apply the stored UI scale before the first paint to avoid a size flash. */
export function initializeFontScale() {
  applyFontScale(storedFontScale());
}

export function useFontScale() {
  const [fontScale, setFontScaleState] = useState(storedFontScale);

  useLayoutEffect(() => {
    const next = applyFontScale(fontScale);
    if (next !== fontScale) {
      setFontScaleState(next);
      return;
    }
    try {
      window.localStorage.setItem(FONT_SCALE_KEY, String(next));
    } catch {
      // Persistence is optional when storage is unavailable.
    }
  }, [fontScale]);

  const setFontScale = useCallback((scale: number) => {
    setFontScaleState(clampFontScale(scale));
  }, []);

  return { fontScale, setFontScale };
}
