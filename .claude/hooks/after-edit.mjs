#!/usr/bin/env node
// Claude Code PostToolUse hook for Edit/Write.
// Gives immediate feedback (exit 2 -> stderr shown to Claude) after a file change:
// - private words (names, e-mails, local paths) in any repository file;
// - catalog/ changes -> JSON Schema validation;
// - UI locale changes -> FR/EN key parity;
// - .rs changes -> rustfmt on that file.

import { readFileSync, existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { isAbsolute, join, relative, resolve, sep } from "node:path";
import { findPrivateWords } from "../../tools/git-hooks/private-words.mjs";

let input;
try {
  input = JSON.parse(readFileSync(0, "utf8"));
} catch {
  process.exit(0);
}
const root = resolve(process.env.CLAUDE_PROJECT_DIR ?? process.cwd());
const filePath = input?.tool_input?.file_path;
if (!filePath) process.exit(0);
const absolute = resolve(filePath);
const rel = relative(root, absolute).split(sep).join("/");
// Outside the repository, or inside .git/ (local-only files such as the private-words list).
// On Windows, a file on another drive yields an absolute `rel`: also outside.
if (isAbsolute(rel) || rel.startsWith("..") || rel.startsWith(".git/") || !existsSync(absolute)) process.exit(0);

const problems = [];
const run = (label, command, args) => {
  // No shell: paths may contain spaces (e.g. "World of Warcraft") and every tool here is a real executable.
  const result = spawnSync(command, args, { cwd: root, encoding: "utf8" });
  if (result.status !== 0) problems.push(`${label} failed:\n${(result.stdout ?? "") + (result.stderr ?? "")}`.trim());
};

try {
  const hits = findPrivateWords(readFileSync(absolute, "utf8"));
  if (hits.length > 0) {
    problems.push(`${rel} contains ${hits.length} private word(s) listed in .git/info/private-words. Remove them: this repository is public.`);
  }
} catch {
  // Binary file or no git repository yet.
}

if (rel.startsWith("catalog/")) run("Catalog validation (npm run validate:catalog)", "node", ["tools/validate-catalog.mjs"]);
if (rel.includes("i18n/locales/")) run("i18n parity (npm run check:i18n)", "node", ["tools/check-i18n.mjs"]);
if (rel.endsWith(".rs")) {
  // rustup may not be on PATH yet (e.g. installed during the session): try ~/.cargo/bin too.
  const local = join(homedir(), ".cargo", "bin", process.platform === "win32" ? "rustfmt.exe" : "rustfmt");
  const rustfmt = existsSync(local) ? local : "rustfmt";
  if (spawnSync(rustfmt, ["--version"]).status === 0) {
    run("rustfmt", rustfmt, ["--edition", "2021", absolute]);
  }
}

if (problems.length > 0) {
  console.error(problems.join("\n\n"));
  process.exit(2);
}
process.exit(0);
