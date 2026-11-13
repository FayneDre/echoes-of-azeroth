// Shared helper: reads the maintainer's *local* list of private words (names,
// e-mail addresses, local paths...) that must never be published.
//
// The list lives in `.git/info/private-words` (one word per line, `#` comments).
// `.git/` is never pushed, so the list itself stays private.

import { readFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join } from "node:path";

export function privateWords() {
  const gitDir = execFileSync("git", ["rev-parse", "--git-common-dir"], { encoding: "utf8" }).trim();
  const file = join(gitDir, "info", "private-words");
  if (!existsSync(file)) return [];
  return readFileSync(file, "utf8")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#"));
}

/** Returns the private words found in `text` (case-insensitive). */
export function findPrivateWords(text, words = privateWords()) {
  const lower = text.toLowerCase();
  return words.filter((word) => lower.includes(word.toLowerCase()));
}
