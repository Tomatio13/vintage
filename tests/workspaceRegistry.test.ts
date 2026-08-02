import assert from "node:assert/strict";
import test from "node:test";
import {
  migrateRegistryText,
  titleFromPath,
  validateRegistryRoots,
  type WorkspaceRootRecord,
} from "../src/workspace/persistence.ts";

const NUL = String.fromCharCode(0);

function fixedGenerator(): () => string {
  let counter = 0;
  return () => {
    counter += 1;
    return `id-${counter.toString(16).padStart(8, "0")}`;
  };
}

test("empty or blank registry text yields no roots without migration", () => {
  for (const text of ["", "   ", "\n"]) {
    const result = migrateRegistryText(text, fixedGenerator());
    assert.deepEqual(result, { status: "ok", roots: [], migrated: false });
  }
});

test("plain path arrays migrate with generated ids and derived titles", () => {
  const result = migrateRegistryText(
    JSON.stringify(["/home/user/alpha", "/home/user/beta", "/home/user/alpha"]),
    fixedGenerator(),
  );
  assert.equal(result.status, "ok");
  if (result.status !== "ok") return;
  assert.equal(result.migrated, true);
  assert.equal(result.roots.length, 2, "duplicate paths collapse");
  assert.deepEqual(
    result.roots.map((root) => [
      root.id,
      root.path,
      root.title,
      root.createdAt,
    ]),
    [
      ["id-00000001", "/home/user/alpha", "alpha", 0],
      ["id-00000002", "/home/user/beta", "beta", 0],
    ],
  );
});

test("legacy object records keep their creation time", () => {
  const result = migrateRegistryText(
    JSON.stringify([
      { path: "/home/user/alpha", createdAt: 111 },
      { path: "/home/user/beta", createdAt: 222 },
    ]),
    fixedGenerator(),
  );
  assert.equal(result.status, "ok");
  if (result.status !== "ok") return;
  assert.equal(result.migrated, true);
  assert.deepEqual(
    result.roots.map((root) => root.createdAt),
    [111, 222],
  );
});

test("versioned registries are accepted without migration", () => {
  const roots: WorkspaceRootRecord[] = [
    { id: "root-1", path: "/home/user/alpha", title: "alpha", createdAt: 5 },
  ];
  const result = migrateRegistryText(
    JSON.stringify({ version: 1, roots }),
    fixedGenerator(),
  );
  assert.equal(result.status, "ok");
  if (result.status !== "ok") return;
  assert.equal(result.migrated, false);
  assert.deepEqual(result.roots, roots);
});

test("damaged or unknown-version registries are rejected", () => {
  const damaged = migrateRegistryText(
    '{"version":1,"roots":[',
    fixedGenerator(),
  );
  assert.equal(damaged.status, "invalid");
  if (damaged.status === "invalid") {
    assert.match(damaged.reason, /damaged/i);
  }

  const unknown = migrateRegistryText(
    JSON.stringify({ version: 2, roots: [] }),
    fixedGenerator(),
  );
  assert.equal(unknown.status, "invalid");
  if (unknown.status === "invalid") {
    assert.match(unknown.reason, /unsupported version/i);
  }

  const notJsonShape = migrateRegistryText(
    JSON.stringify(42),
    fixedGenerator(),
  );
  assert.equal(notJsonShape.status, "invalid");
});

test("legacy entries with unusable paths are rejected", () => {
  const nulPath = migrateRegistryText(
    JSON.stringify([`/home/user${NUL}/alpha`]),
    fixedGenerator(),
  );
  assert.equal(nulPath.status, "invalid");
  if (nulPath.status === "invalid") {
    assert.match(nulPath.reason, /unusable path/i);
  }

  const mixedShapes = migrateRegistryText(
    JSON.stringify(["/ok", { notPath: true }]),
    fixedGenerator(),
  );
  assert.equal(mixedShapes.status, "invalid");
});

test("versioned records are validated for ids, paths, and titles", () => {
  function registryWith(roots: WorkspaceRootRecord[]) {
    return migrateRegistryText(
      JSON.stringify({ version: 1, roots }),
      fixedGenerator(),
    );
  }

  const duplicateId = registryWith([
    { id: "same", path: "/a", title: "a", createdAt: 0 },
    { id: "same", path: "/b", title: "b", createdAt: 0 },
  ]);
  assert.equal(duplicateId.status, "invalid");
  if (duplicateId.status === "invalid") {
    assert.match(duplicateId.reason, /duplicate id/i);
  }

  const emptyPath = registryWith([
    { id: "r1", path: "", title: "x", createdAt: 0 },
  ]);
  assert.equal(emptyPath.status, "invalid");

  const badId = registryWith([
    { id: "has space", path: "/a", title: "a", createdAt: 0 },
  ]);
  assert.equal(badId.status, "invalid");

  const longTitle = registryWith([
    { id: "r1", path: "/a", title: "t".repeat(129), createdAt: 0 },
  ]);
  assert.equal(longTitle.status, "invalid");
});

test("registry root count is bounded", () => {
  const roots = Array.from({ length: 65 }, (_, index) => ({
    id: `root-${index}`,
    path: `/ws/${index}`,
    title: `${index}`,
    createdAt: 0,
  }));
  assert.ok(validateRegistryRoots(roots).length > 0);
  assert.deepEqual(validateRegistryRoots(roots.slice(0, 64)), []);
});

test("titleFromPath handles both separators and trailing slashes", () => {
  assert.equal(titleFromPath("/home/user/project"), "project");
  assert.equal(titleFromPath("C:\\Users\\me\\repo"), "repo");
  assert.equal(titleFromPath("/home/user/project/"), "project");
  assert.equal(titleFromPath("solo"), "solo");
  assert.equal(titleFromPath("/"), "/");
});
