import assert from "node:assert/strict";
import test from "node:test";
import {
  emptyLayoutFile,
  invalidBackupName,
  parseWorkspaceLayoutFile,
  validateLayoutFileValue,
} from "../src/workspace/persistence.ts";
import {
  normalizePaneTitle,
  normalizeTabTitle,
  type PaneDefinition,
  type PaneLayout,
  type WorkspaceState,
} from "../src/workspace/types.ts";

const NUL = String.fromCharCode(0);

function samplePane(id: string): PaneDefinition {
  return {
    id,
    title: "Pane",
    shellId: "ubuntu-default",
    agentKind: null,
    launch: { type: "shell", shellId: "ubuntu-default" },
    workingDirectory: null,
    resumeSessionId: null,
  };
}

function sampleTab() {
  return {
    id: "tab-1",
    title: "agents",
    layout: { type: "leaf", paneId: "pane-1" } as PaneLayout,
    selectedPaneId: "pane-1",
    panes: [samplePane("pane-1")],
  };
}

function sampleWorkspace(id = "ws-1"): WorkspaceState {
  return {
    id,
    path: "/home/user/project",
    title: "project",
    tabs: [sampleTab()],
    selectedTabId: "tab-1",
  };
}

function layoutFileWith(workspaces: WorkspaceState[]) {
  return { version: 1, workspaces };
}

test("normalizeTabTitle trims valid names and rejects unusable names", () => {
  assert.equal(normalizeTabTitle("  Build agent  "), "Build agent");
  assert.equal(normalizeTabTitle("   "), null);
  assert.equal(normalizeTabTitle("🚀".repeat(128)), "🚀".repeat(128));
  assert.equal(normalizeTabTitle("🚀".repeat(129)), null);
});

test("normalizePaneTitle applies the same rules as tab titles", () => {
  assert.equal(normalizePaneTitle("  Codex  "), "Codex");
  assert.equal(normalizePaneTitle("   "), null);
  assert.equal(normalizePaneTitle("🚀".repeat(128)), "🚀".repeat(128));
  assert.equal(normalizePaneTitle("🚀".repeat(129)), null);
});

test("emptyLayoutFile is version 1 with no workspaces", () => {
  assert.deepEqual(emptyLayoutFile(), { version: 1, workspaces: [] });
});

test("a valid layout file parses and round-trips", () => {
  const file = layoutFileWith([sampleWorkspace()]);
  const text = JSON.stringify(file);
  const result = parseWorkspaceLayoutFile(text);
  assert.equal(result.status, "ok");
  if (result.status === "ok") {
    assert.deepEqual(result.file, file);
  }
});

test("damaged JSON is rejected as damaged", () => {
  const result = parseWorkspaceLayoutFile('{"version":1,"workspaces":');
  assert.equal(result.status, "invalid");
  if (result.status === "invalid") {
    assert.match(result.reason, /damaged/i);
  }
});

test("missing or unknown versions are rejected", () => {
  for (const value of [
    { workspaces: [] },
    { version: 2, workspaces: [] },
    { version: "1", workspaces: [] },
    null,
    [],
  ]) {
    const issues = validateLayoutFileValue(value);
    assert.ok(
      issues.length > 0,
      `value ${JSON.stringify(value)} must be rejected`,
    );
  }
});

test("workspace and tab count limits are enforced", () => {
  const manyWorkspaces = Array.from({ length: 65 }, (_, index) =>
    sampleWorkspace(`ws-${index}`),
  );
  assert.ok(
    validateLayoutFileValue(layoutFileWith(manyWorkspaces)).some((issue) =>
      issue.includes("number of workspaces"),
    ),
  );

  const workspace = sampleWorkspace();
  workspace.tabs = Array.from({ length: 65 }, (_, index) => ({
    ...sampleTab(),
    id: `tab-${index}`,
  }));
  workspace.selectedTabId = "tab-0";
  assert.ok(
    validateLayoutFileValue(layoutFileWith([workspace])).some((issue) =>
      issue.includes("number of tabs"),
    ),
  );
});

test("selected tab and pane references must exist", () => {
  const badTabRef = sampleWorkspace();
  badTabRef.selectedTabId = "missing-tab";
  assert.ok(
    validateLayoutFileValue(layoutFileWith([badTabRef])).some((issue) =>
      issue.includes("missing tab"),
    ),
  );

  const badPaneRef = sampleWorkspace();
  badPaneRef.tabs[0].selectedPaneId = "missing-pane";
  assert.ok(
    validateLayoutFileValue(layoutFileWith([badPaneRef])).some((issue) =>
      issue.includes("missing pane"),
    ),
  );
});

test("pane definitions must match the split tree leaves exactly", () => {
  const extraDefinition = sampleWorkspace();
  extraDefinition.tabs[0].panes.push(samplePane("orphan"));
  assert.ok(
    validateLayoutFileValue(layoutFileWith([extraDefinition])).some((issue) =>
      issue.includes("disagree"),
    ),
  );

  const missingDefinition = sampleWorkspace();
  missingDefinition.tabs[0].panes = [];
  assert.ok(
    validateLayoutFileValue(layoutFileWith([missingDefinition])).some((issue) =>
      issue.includes("disagree"),
    ),
  );
});

test("duplicate ids are rejected at every level", () => {
  const duplicateWorkspace = layoutFileWith([
    sampleWorkspace("same"),
    sampleWorkspace("same"),
  ]);
  assert.ok(
    validateLayoutFileValue(duplicateWorkspace).some((issue) =>
      issue.includes("duplicate workspace id"),
    ),
  );

  const duplicateTab = sampleWorkspace();
  duplicateTab.tabs.push({ ...sampleTab() });
  assert.ok(
    validateLayoutFileValue(layoutFileWith([duplicateTab])).some((issue) =>
      issue.includes("duplicate tab id"),
    ),
  );

  const duplicatePane = sampleWorkspace();
  duplicatePane.tabs[0].layout = {
    type: "split",
    direction: "horizontal",
    ratio: 0.5,
    first: { type: "leaf", paneId: "pane-1" },
    second: { type: "leaf", paneId: "pane-1" },
  };
  duplicatePane.tabs[0].panes = [samplePane("pane-1")];
  assert.ok(
    validateLayoutFileValue(layoutFileWith([duplicatePane])).some((issue) =>
      issue.includes("appears more than once"),
    ),
  );
});

test("split depth beyond 16 levels is rejected", () => {
  let layout: PaneLayout = { type: "leaf", paneId: "deepest" };
  const definitions = [samplePane("deepest")];
  for (let index = 0; index < 16; index += 1) {
    const sideId = `side-${index}`;
    layout = {
      type: "split",
      direction: "horizontal",
      ratio: 0.5,
      first: { type: "leaf", paneId: sideId },
      second: layout,
    };
    definitions.push(samplePane(sideId));
  }
  const workspace = sampleWorkspace();
  workspace.tabs[0].layout = layout;
  workspace.tabs[0].selectedPaneId = "deepest";
  workspace.tabs[0].panes = definitions;
  assert.ok(
    validateLayoutFileValue(layoutFileWith([workspace])).some((issue) =>
      issue.includes("depth"),
    ),
  );
});

test("split ratios outside 0.2 to 0.8 are rejected", () => {
  const workspace = sampleWorkspace();
  workspace.tabs[0].layout = {
    type: "split",
    direction: "horizontal",
    ratio: 0.95,
    first: { type: "leaf", paneId: "pane-1" },
    second: { type: "leaf", paneId: "pane-2" },
  };
  workspace.tabs[0].panes.push(samplePane("pane-2"));
  assert.ok(
    validateLayoutFileValue(layoutFileWith([workspace])).some((issue) =>
      issue.includes("ratio"),
    ),
  );
});

test("launch spec argument limits are enforced", () => {
  function workspaceWithLaunch(
    launch: PaneDefinition["launch"],
  ): WorkspaceState {
    const workspace = sampleWorkspace();
    workspace.tabs[0].panes = [{ ...samplePane("pane-1"), launch }];
    return workspace;
  }

  const nulArgument = workspaceWithLaunch({
    type: "custom",
    program: "bash",
    args: [`bad${NUL}arg`],
  });
  assert.ok(
    validateLayoutFileValue(layoutFileWith([nulArgument])).some((issue) =>
      issue.includes("unusable argument"),
    ),
  );

  const tooManyArgs = workspaceWithLaunch({
    type: "custom",
    program: "bash",
    args: Array.from({ length: 257 }, () => "x"),
  });
  assert.ok(
    validateLayoutFileValue(layoutFileWith([tooManyArgs])).some((issue) =>
      issue.includes("too many arguments"),
    ),
  );

  const oversizedArgv = workspaceWithLaunch({
    type: "agent",
    preset: "codex",
    shellId: "ubuntu-default",
    args: Array.from({ length: 3 }, () => "y".repeat(24 * 1024)),
  });
  assert.ok(
    validateLayoutFileValue(layoutFileWith([oversizedArgv])).some((issue) =>
      issue.includes("total size"),
    ),
  );

  const nulProgram = workspaceWithLaunch({
    type: "custom",
    program: `ba${NUL}sh`,
    args: [],
  });
  assert.ok(
    validateLayoutFileValue(layoutFileWith([nulProgram])).some((issue) =>
      issue.includes("invalid program"),
    ),
  );
});

test("values with NUL characters are rejected", () => {
  const badPath = sampleWorkspace();
  badPath.path = `/home/user${NUL}/project`;
  assert.ok(
    validateLayoutFileValue(layoutFileWith([badPath])).some((issue) =>
      issue.includes("unusable workspace path"),
    ),
  );

  const badShell = sampleWorkspace();
  badShell.tabs[0].panes[0].shellId = `shell${NUL}`;
  assert.ok(
    validateLayoutFileValue(layoutFileWith([badShell])).some((issue) =>
      issue.includes("invalid shell reference"),
    ),
  );
});

test("titles beyond 128 code points are rejected", () => {
  const badTitle = sampleWorkspace();
  badTitle.title = "t".repeat(129);
  assert.ok(
    validateLayoutFileValue(layoutFileWith([badTitle])).some((issue) =>
      issue.includes("title"),
    ),
  );
});

test("invalidBackupName follows the documented shape", () => {
  assert.equal(
    invalidBackupName("20260801T000000Z"),
    "workspace-layouts.invalid-20260801T000000Z.json",
  );
});
