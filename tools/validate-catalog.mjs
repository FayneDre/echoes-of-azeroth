#!/usr/bin/env node
// Validates every catalog manifest against its JSON Schema, plus the
// file-layout rules (id == file name, emulator == parent directory).
// Cross-manifest semantic checks (references, compatibility) live in the
// Rust `eoa-catalog` crate and run with `cargo run -p eoa-cli -- catalog validate`.

import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join, basename, dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const catalog = join(root, "catalog");
const schemaDir = join(catalog, "schema");

const ajv = new Ajv2020({ allErrors: true, strict: true, strictRequired: false, allowUnionTypes: true });
addFormats(ajv);
for (const file of readdirSync(schemaDir).filter((f) => f.endsWith(".schema.json"))) {
  ajv.addSchema(JSON.parse(readFileSync(join(schemaDir, file), "utf8")));
}
const schemaUrl = (kind) => `https://echoes-of-azeroth.github.io/schema/v1/${kind}.schema.json`;

const jsonFiles = (dir) =>
  existsSync(dir) ? readdirSync(dir).filter((f) => f.endsWith(".json")).map((f) => join(dir, f)) : [];

/** @type {{ kind: string, path: string, emulator?: string }[]} */
const targets = [];
for (const path of jsonFiles(join(catalog, "clients"))) targets.push({ kind: "client", path });
for (const path of jsonFiles(join(catalog, "components"))) targets.push({ kind: "component", path });
const emulatorsDir = join(catalog, "emulators");
for (const emulator of existsSync(emulatorsDir) ? readdirSync(emulatorsDir) : []) {
  const dir = join(emulatorsDir, emulator);
  targets.push({ kind: "emulator", path: join(dir, "emulator.json"), emulator });
  for (const path of jsonFiles(join(dir, "flavors"))) targets.push({ kind: "flavor", path, emulator });
  for (const path of jsonFiles(join(dir, "modules"))) targets.push({ kind: "module", path, emulator });
}

let failures = 0;
const fail = (path, message) => {
  failures += 1;
  console.error(`✗ ${relative(root, path)}: ${message}`);
};

for (const { kind, path, emulator } of targets) {
  const before = failures;
  if (!existsSync(path)) {
    fail(path, "missing file");
    continue;
  }
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(path, `invalid JSON: ${error.message}`);
    continue;
  }
  const validate = ajv.getSchema(schemaUrl(kind));
  if (!validate(manifest)) {
    for (const error of validate.errors ?? []) {
      fail(path, `${error.instancePath || "/"} ${error.message}`);
    }
    continue;
  }
  const expectedId = kind === "emulator" ? emulator : basename(path, ".json");
  if (manifest.id !== expectedId) fail(path, `id "${manifest.id}" must be "${expectedId}"`);
  if ((kind === "flavor" || kind === "module") && manifest.emulator !== emulator) {
    fail(path, `emulator "${manifest.emulator}" must be "${emulator}" (parent directory)`);
  }
  if (failures === before) console.log(`✓ ${relative(root, path)}`);
}

if (failures > 0) {
  console.error(`\n${failures} problem(s) found.`);
  process.exit(1);
}
console.log(`\nCatalog OK (${targets.length} manifests).`);
