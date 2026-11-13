// Regression cases for .claude/hooks/guard-shell.mjs (run: npm run test:hooks).
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
const hook = fileURLToPath(new URL("../.claude/hooks/guard-shell.mjs", import.meta.url));
const cases = [
  ["allow", 'git commit -m "feat: x"'],
  ["allow", 'git commit -am "fix: handle -n flag"'],
  ["allow", "grep -n foo file && git commit -m x"],
  ["allow", "git push origin main"],
  ["allow", "git config core.hooksPath .githooks"],
  ["allow", "git log -n 5"],
  ["block", 'git commit -nm "feat: x"'],
  ["block", 'git commit -an -m x'],
  ["block", "git commit --no-verify -m x"],
  ["block", "git push --no-verify origin main"],
  ["block", "git push origin +main"],
  ["block", "git push -f origin main"],
  ["block", "git push --force-with-lease"],
  ["block", "git config core.hooksPath .githooks-off"],
  ["block", "git config core.hooksPath .githooks && git config --unset core.hooksPath"],
  ["block", 'git commit -m "x\n\nCo-Authored-By: Claude <noreply@anthropic.com>"'],
];
let bad = 0;
for (const [want, command] of cases) {
  const r = spawnSync(process.execPath, [hook], { input: JSON.stringify({ tool_input: { command } }) });
  const got = r.status === 2 ? "block" : "allow";
  if (got !== want) bad++;
  console.log(`${got === want ? "ok  " : "FAIL"} ${want}: ${command.replace(/\n/g, "\\n")}`);
}
process.exit(bad ? 1 : 0);
