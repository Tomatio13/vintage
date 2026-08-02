import assert from "node:assert/strict";
import test from "node:test";
import {
  parsePreferredShellId,
  resolvePreferredShellId,
} from "../src/settings/shells.ts";

test("parsePreferredShellId accepts valid ids and rejects garbage", () => {
  assert.equal(parsePreferredShellId("windows-git-bash"), "windows-git-bash");
  assert.equal(parsePreferredShellId("  windows-pwsh  "), "windows-pwsh");
  assert.equal(parsePreferredShellId(null), null);
  assert.equal(parsePreferredShellId(""), null);
  assert.equal(parsePreferredShellId("   "), null);
  assert.equal(parsePreferredShellId("has space"), null);
  assert.equal(parsePreferredShellId("a/b"), null);
  assert.equal(parsePreferredShellId("x".repeat(65)), null);
});

test("resolvePreferredShellId prefers the user's available shell", () => {
  const shells = [
    { id: "windows-default", available: true },
    { id: "windows-pwsh", available: true },
    { id: "windows-git-bash", available: true },
  ];
  assert.equal(
    resolvePreferredShellId("windows-git-bash", shells),
    "windows-git-bash",
  );
});

test("resolvePreferredShellId falls back when the preferred shell is gone", () => {
  const shells = [
    { id: "windows-default", available: true },
    { id: "windows-pwsh", available: true },
  ];
  assert.equal(
    resolvePreferredShellId("windows-git-bash", shells),
    "windows-default",
  );
});

test("resolvePreferredShellId skips unavailable preferred shells", () => {
  const shells = [
    { id: "windows-default", available: true },
    { id: "windows-git-bash", available: false },
  ];
  assert.equal(
    resolvePreferredShellId("windows-git-bash", shells),
    "windows-default",
  );
});

test("resolvePreferredShellId with null uses the platform default then first", () => {
  const shells = [
    { id: "windows-git-bash", available: true },
    { id: "windows-default", available: true },
  ];
  assert.equal(resolvePreferredShellId(null, shells), "windows-default");

  const noDefault = [
    { id: "windows-git-bash", available: true },
    { id: "unix-zsh", available: true },
  ];
  assert.equal(resolvePreferredShellId(null, noDefault), "windows-git-bash");
});

test("resolvePreferredShellId handles the unix default and empty lists", () => {
  const unixShells = [
    { id: "unix-default", available: true },
    { id: "unix-zsh", available: true },
  ];
  assert.equal(resolvePreferredShellId("unix-zsh", unixShells), "unix-zsh");
  assert.equal(resolvePreferredShellId(null, unixShells), "unix-default");

  assert.equal(resolvePreferredShellId(null, []), null);
  assert.equal(resolvePreferredShellId("anything", []), null);
});
