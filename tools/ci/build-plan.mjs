#!/usr/bin/env node
// Computes the "full build" plan of a flavor from the catalog: the core source
// and every catalog module compatible with it (ADR 0003). Writes the plan as
// JSON and, on GitHub Actions, exposes key values as step outputs.
//
// Usage: node tools/ci/build-plan.mjs <emulator> <flavor> <out.json>

import { readFileSync, readdirSync, writeFileSync, appendFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const [emulator, flavorId, out] = process.argv.slice(2);
if (!emulator || !flavorId || !out) {
  console.error("usage: build-plan.mjs <emulator> <flavor> <out.json>");
  process.exit(2);
}

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "..");
const emulatorDir = join(root, "catalog", "emulators", emulator);
const read = (path) => JSON.parse(readFileSync(path, "utf8"));
const text = (path) => readFileSync(path, "utf8").replace(/\r\n/g, "\n");
const sha256 = (value) => createHash("sha256").update(value).digest("hex");

const flavor = read(join(emulatorDir, "flavors", `${flavorId}.json`));
const modules = readdirSync(join(emulatorDir, "modules"))
  .filter((f) => f.endsWith(".json"))
  .sort()
  .map((f) => read(join(emulatorDir, "modules", f)))
  .filter((m) => m.compatibleFlavors.includes(flavorId))
  .map((m) => ({
    id: m.id,
    repo: m.source.repo,
    commit: m.source.commit,
    dbimportId: m.database.dbimportId,
    mandatory: flavor.requiredModules.includes(m.id) || m.removable === false,
  }));

for (const required of flavor.requiredModules) {
  if (!modules.some((m) => m.id === required)) {
    console.error(`required module ${required} is missing from the catalog`);
    process.exit(1);
  }
}

// Runtime dependencies, as declared by the emulator.
const emulatorManifest = read(join(emulatorDir, "emulator.json"));
const components = emulatorManifest.components.map((c) => `catalog/components/${c.id}.json`);
const database = components.find((path) => read(join(root, path)).kind === "database");
if (!database) {
  console.error(`${emulator} declares no database component`);
  process.exit(1);
}
const client = `catalog/clients/${emulatorManifest.client}.json`;

// The build key covers every input that changes the artifact: identical inputs
// => identical key => the existing artifact is reused instead of compiling.
// Recipe = the workflow's global env (toolchain versions) + its `build` job
// (comment lines ignored) + every script the build job runs. Smoke-test-only
// edits do not change the key.
const workflow = text(join(root, ".github", "workflows", "build-server.yml"));
const section = (from, to) => {
  const start = workflow.indexOf(from);
  const end = start < 0 ? -1 : workflow.indexOf(to, start + from.length);
  if (end < 0) {
    console.error(`build-server.yml: section '${from.trim()}' .. '${to.trim()}' not found`);
    process.exit(1);
  }
  return workflow.slice(start, end);
};
const withoutComments = (yaml) =>
  yaml
    .split("\n")
    .filter((line) => !/^\s*#/.test(line))
    .join("\n");
const recipe = [
  withoutComments(section("\nenv:\n", "\njobs:\n")),
  withoutComments(section("\n  build:\n", "\n  smoke-test:\n")),
  ...["build-plan.mjs", "package-server.mjs", "fetch-mysql.ps1"].map((f) => text(join(here, f))),
].join("\n");
const keyInput = JSON.stringify({
  core: [flavor.source.repo, flavor.source.commit],
  // Whole plan entries: package-server.mjs writes them into build-info.json.
  modules,
  // MySQL client libraries linked into the Windows servers.
  database: read(join(root, database)).platforms["windows-x86_64"] ?? null,
  recipe: sha256(recipe),
});
const buildKey = sha256(keyInput).slice(0, 16);

const plan = {
  emulator,
  flavor: flavorId,
  buildKey,
  core: { repo: flavor.source.repo, commit: flavor.source.commit },
  modules,
  database,
  client,
};
writeFileSync(out, JSON.stringify(plan, null, 2) + "\n");
console.log(JSON.stringify(plan, null, 2));

if (process.env.GITHUB_OUTPUT) {
  appendFileSync(
    process.env.GITHUB_OUTPUT,
    [
      `build-key=${buildKey}`,
      `core-repo=${flavor.source.repo}`,
      `core-commit=${flavor.source.commit}`,
      `artifact=server-${emulator}-${flavorId}-${buildKey}`,
      `mandatory=${modules.filter((m) => m.mandatory).map((m) => m.id).join(",")}`,
      `optional=${modules.filter((m) => !m.mandatory).map((m) => m.id).join(",")}`,
      `database-manifest=${database}`,
      `client-manifest=${client}`,
      "",
    ].join("\n"),
  );
}
