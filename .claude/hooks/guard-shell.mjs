#!/usr/bin/env node
// Claude Code PreToolUse hook for Bash/PowerShell.
// Blocks (exit 2, reason on stderr -> shown to Claude):
// - commits carrying AI co-author / attribution lines;
// - bypassing git hooks (--no-verify, -n on commit, core.hooksPath override);
// - force pushes;
// - commands containing a private word from .git/info/private-words.

import { readFileSync } from "node:fs";
import { findPrivateWords } from "../../tools/git-hooks/private-words.mjs";

let input;
try {
  input = JSON.parse(readFileSync(0, "utf8"));
} catch {
  process.exit(0); // Not our business if the payload is unreadable.
}
const command = String(input?.tool_input?.command ?? "");
if (!command) process.exit(0);

const block = (reason) => {
  console.error(`Blocked by .claude/hooks/guard-shell.mjs: ${reason}`);
  process.exit(2);
};

const isCommit = /\bgit\b[^|;&]*\bcommit\b/.test(command);
if (isCommit) {
  if (/co-authored-by:.*(claude|anthropic)|noreply@anthropic\.com|generated with \[?claude/i.test(command)) {
    block("commits in this repository must not contain AI co-author or attribution lines. Remove the trailer and retry.");
  }
  if (/--no-verify\b|\s-n\b/.test(command)) block("git hooks must not be bypassed (--no-verify).");
}
if (/\bgit\b.*\bpush\b.*(--force\b|--force-with-lease\b|\s-f\b)/.test(command)) {
  block("force pushes are not allowed. Ask the maintainer.");
}
if (/core\.hookspath/i.test(command) && !/core\.hookspath\s+\.githooks/i.test(command)) {
  block("changing core.hooksPath away from .githooks is not allowed.");
}

try {
  if (findPrivateWords(command).length > 0) {
    block("the command contains a private word listed in .git/info/private-words. Never put personal information in commands, commits or files.");
  }
} catch {
  // Not inside a git repository yet: nothing to check.
}

process.exit(0);
