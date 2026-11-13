#!/usr/bin/env node
// Git commit-msg hook (installed via `core.hooksPath = .githooks`).
// - Enforces Conventional Commits on the subject line.
// - Rejects AI co-author / attribution trailers: commits are authored by humans only.
// - Rejects private words (see private-words.mjs).

import { readFileSync } from "node:fs";
import { findPrivateWords } from "./private-words.mjs";

const file = process.argv[2];
const message = readFileSync(file, "utf8")
  .split(/\r?\n/)
  .filter((line) => !line.startsWith("#"))
  .join("\n")
  .trim();
const subject = message.split("\n")[0] ?? "";
const errors = [];

const TYPES = "feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert";
const isMergeOrRevert = /^(Merge|Revert) /.test(subject);
if (!isMergeOrRevert && !new RegExp(`^(${TYPES})(\\([a-z0-9-]+\\))?!?: \\S`).test(subject)) {
  errors.push(`subject must follow Conventional Commits: "<type>(<scope>)?: <summary>" with type in ${TYPES.split("|").join(", ")}`);
}
if (subject.length > 100) errors.push("subject is longer than 100 characters");

const forbiddenTrailers = [
  /^co-authored-by:.*(claude|anthropic|copilot|chatgpt|openai|gemini|\bai\b)/im,
  /generated with \[?claude/i,
  /noreply@anthropic\.com/i,
];
if (forbiddenTrailers.some((re) => re.test(message))) {
  errors.push("AI co-author or attribution lines are not allowed in this repository");
}

if (findPrivateWords(message).length > 0) {
  errors.push("message contains a private word listed in .git/info/private-words");
}

if (errors.length > 0) {
  console.error("✗ Commit message rejected:");
  for (const error of errors) console.error(`  - ${error}`);
  process.exit(1);
}
