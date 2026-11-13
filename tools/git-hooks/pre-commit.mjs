#!/usr/bin/env node
// Git pre-commit hook (installed via `core.hooksPath = .githooks`).
// 1. Blocks private words (see private-words.mjs) in staged content and paths.
// 2. Runs fast checks relevant to the staged files.

import { execFileSync, spawnSync } from "node:child_process";
import { findPrivateWords, privateWords } from "./private-words.mjs";

const git = (...args) => execFileSync("git", args, { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
const staged = git("diff", "--cached", "--name-only", "--diff-filter=ACMR").split("\n").filter(Boolean);
let failed = false;

// 1. Private words.
const words = privateWords();
if (words.length > 0) {
  const added = git("diff", "--cached", "--unified=0", "--no-color", "--diff-filter=ACMR")
    .split("\n")
    .filter((line) => line.startsWith("+") && !line.startsWith("+++"))
    .join("\n");
  const hits = new Set([...findPrivateWords(added, words), ...findPrivateWords(staged.join("\n"), words)]);
  if (hits.size > 0) {
    console.error(`✗ Staged changes contain ${hits.size} private word(s) listed in .git/info/private-words.`);
    console.error("  Remove them before committing (run `git diff --cached` to find them).");
    failed = true;
  }
}

// 2. Fast checks, only when relevant files are staged.
const run = (label, command, args) => {
  const result = spawnSync(command, args, { stdio: "inherit", shell: process.platform === "win32" });
  if (result.status !== 0) {
    console.error(`✗ ${label} failed`);
    failed = true;
  }
};
if (staged.some((f) => f.startsWith("catalog/"))) run("catalog validation", "node", ["tools/validate-catalog.mjs"]);
if (staged.some((f) => f.includes("i18n/locales/"))) run("i18n check", "node", ["tools/check-i18n.mjs"]);
if (staged.some((f) => f.endsWith(".rs"))) run("cargo fmt --check", "cargo", ["fmt", "--all", "--", "--check"]);

process.exit(failed ? 1 : 0);
