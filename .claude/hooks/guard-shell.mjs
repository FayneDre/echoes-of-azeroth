#!/usr/bin/env node
// Claude Code PreToolUse hook for Bash/PowerShell.
// Blocks (exit 2, reason on stderr -> shown to Claude):
// - commits carrying AI co-author / attribution lines;
// - bypassing git hooks (--no-verify on commit/push, -n in any short-flag
//   cluster of git commit, core.hooksPath set to anything but .githooks);
// - force pushes (--force*, -f in a flag cluster, +refspec);
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

// Inspect each simple command on its own, with quoted text removed so that
// commit messages or grep patterns are not mistaken for flags.
const segments = command
  .split(/\|\||&&|[|;&\n]/)
  .map((s) => s.replace(/"(?:[^"\\]|\\.)*"|'[^']*'/g, '""').trim())
  .filter((s) => /\bgit\b/.test(s));
const hasShortFlag = (s, letter) => new RegExp(`(?:^|\\s)-(?!-)[A-Za-z]*${letter}[A-Za-z]*(?=\\s|$)`).test(s);

if (/\bgit\b[^|;&]*\bcommit\b/.test(command) && /co-authored-by:.*(claude|anthropic)|noreply@anthropic\.com|generated with \[?claude/i.test(command)) {
  block("commits in this repository must not contain AI co-author or attribution lines. Remove the trailer and retry.");
}
for (const s of segments) {
  const commit = /\bgit\b.*\bcommit\b/.test(s);
  const push = /\bgit\b.*\bpush\b/.test(s);
  if ((commit || push) && /\s--no-verify\b/.test(s)) block("git hooks must not be bypassed (--no-verify).");
  if (commit && hasShortFlag(s.replace(/^.*?\bcommit\b/, ""), "n")) {
    block("git hooks must not be bypassed (-n on git commit).");
  }
  if (push) {
    const args = s.replace(/^.*?\bpush\b/, "");
    if (/\s--force(-with-lease|-if-includes)?\b/.test(args) || hasShortFlag(args, "f") || /(?:^|\s)\+\S/.test(args)) {
      block("force pushes (--force, -f, +refspec) are not allowed. Ask the maintainer.");
    }
  }
  if (/core\.hookspath/i.test(s) && !/^git\s+config\s+(--local\s+)?core\.hookspath\s+\.githooks$/i.test(s)) {
    block("changing core.hooksPath away from .githooks is not allowed.");
  }
}

try {
  if (findPrivateWords(command).length > 0) {
    block("the command contains a private word listed in .git/info/private-words. Never put personal information in commands, commits or files.");
  }
} catch {
  // Not inside a git repository yet: nothing to check.
}

process.exit(0);
