#!/usr/bin/env node
// Ensures every UI locale has exactly the same keys and placeholders as en.json
// (the source of truth). Run by `npm run check`, the pre-commit hook and CI.

import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const dir = join(dirname(fileURLToPath(import.meta.url)), "..", "app", "src", "lib", "i18n", "locales");
const load = (file) => JSON.parse(readFileSync(join(dir, file), "utf8"));
const placeholders = (text) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");

const reference = load("en.json");
const referenceKeys = Object.keys(reference);
let problems = 0;
const report = (message) => {
  problems += 1;
  console.error(`✗ ${message}`);
};

for (const file of readdirSync(dir).filter((f) => f.endsWith(".json") && f !== "en.json")) {
  const locale = load(file);
  for (const key of referenceKeys) {
    if (!(key in locale)) report(`${file}: missing key "${key}"`);
    else if (typeof locale[key] !== "string" || locale[key].trim() === "") report(`${file}: empty value for "${key}"`);
    else if (placeholders(locale[key]) !== placeholders(reference[key])) {
      report(`${file}: placeholders of "${key}" differ from en.json`);
    }
  }
  for (const key of Object.keys(locale)) {
    if (!(key in reference)) report(`${file}: unknown key "${key}" (add it to en.json first)`);
  }
}

if (problems > 0) {
  console.error(`\n${problems} i18n problem(s).`);
  process.exit(1);
}
console.log(`i18n OK (${referenceKeys.length} keys).`);
