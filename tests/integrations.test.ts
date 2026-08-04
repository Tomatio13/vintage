import assert from "node:assert/strict";
import test from "node:test";
import {
  describeHostError,
  integrationAgentLabel,
} from "../src/settings/integrations.ts";

test("integrationAgentLabel maps every agent to a display name", () => {
  assert.equal(integrationAgentLabel("codex"), "Codex");
  assert.equal(integrationAgentLabel("claude"), "Claude Code");
  assert.equal(integrationAgentLabel("opencode"), "OpenCode");
});

test("describeHostError surfaces Tauri string errors verbatim", () => {
  assert.equal(
    describeHostError("The file is not valid TOML.", "fallback"),
    "The file is not valid TOML.",
  );
  assert.equal(
    describeHostError({ message: "A host error." }, "fallback"),
    "A host error.",
  );
  assert.equal(describeHostError(undefined, "fallback"), "fallback");
  assert.equal(describeHostError(null, "fallback"), "fallback");
  assert.equal(describeHostError(42, "fallback"), "fallback");
});
