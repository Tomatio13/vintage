import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_TERMINAL_SCROLLBACK,
  parseTerminalScrollback,
} from "../src/terminalScrollback.ts";

test("parseTerminalScrollback accepts supported capacities", () => {
  assert.equal(parseTerminalScrollback("1000"), 1000);
  assert.equal(parseTerminalScrollback("2500"), 2500);
  assert.equal(parseTerminalScrollback("5000"), 5000);
  assert.equal(parseTerminalScrollback("10000"), 10000);
});

test("parseTerminalScrollback rejects missing and unsupported capacities", () => {
  assert.equal(parseTerminalScrollback(null), DEFAULT_TERMINAL_SCROLLBACK);
  assert.equal(parseTerminalScrollback("999"), DEFAULT_TERMINAL_SCROLLBACK);
  assert.equal(parseTerminalScrollback("not-a-number"), DEFAULT_TERMINAL_SCROLLBACK);
});
