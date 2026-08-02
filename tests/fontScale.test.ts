import assert from "node:assert/strict";
import test from "node:test";
import {
  clampFontScale,
  DEFAULT_FONT_SCALE,
  formatFontScale,
  MAX_FONT_SCALE,
  MIN_FONT_SCALE,
} from "../src/fontScale.ts";

test("clamps and steps font scale values", () => {
  assert.equal(clampFontScale(Number.NaN), DEFAULT_FONT_SCALE);
  assert.equal(clampFontScale(0.5), MIN_FONT_SCALE);
  assert.equal(clampFontScale(3), MAX_FONT_SCALE);
  assert.equal(clampFontScale(1.02), 1);
  assert.equal(clampFontScale(1.03), 1.05);
});

test("formats font scale as a percent label", () => {
  assert.equal(formatFontScale(1), "100%");
  assert.equal(formatFontScale(1.25), "125%");
  assert.equal(formatFontScale(0.85), "85%");
});
