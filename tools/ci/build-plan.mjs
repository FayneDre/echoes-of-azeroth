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

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const emulatorDir = join(root, "catalog", "emulators", emulator);
const read = (path) => JSON.parse(readFileSync(path, "utf8"));

const flavor = read(join(emulatorDir, "flavors", `${flavorId}.json`));
const modules = readdirSync(join(emulatorDir, "modules"))
  .filter((f) => f.endsWith(".json"))
  .sort()
  .map((f) => read(join(emulatorDir, "modules", f)))
  .filter((m) => m.compatibleFlavors.includes(flavorId))
  .map((m) => ({ id: m.id, repo: m.source.repo, commit: m.source.commit, dbimportId: m.database.dbimportId }));

for (const required of flavor.requiredModules) {
  if (!modules.some((m) => m.id === required)) {
    console.error(`required module ${required} is missing from the catalog`);
    process.exit(1);
  }
}

// The build key identifies the exact sources; identical inputs => identical key.
const keyInput = JSON.stringify({ core: flavor.source.commit, modules: modules.map((m) => [m.id, m.commit]) });
const buildKey = createHash("sha256").update(keyInput).digest("hex").slice(0, 16);

const plan = {
  emulator,
  flavor: flavorId,
  buildKey,
  core: { repo: flavor.source.repo, commit: flavor.source.commit },
  modules,
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
      "",
    ].join("\n"),
  );
}
