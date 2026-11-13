---
name: commit
description: Prepare and create git commits (and optionally push) for Echoes of Azeroth following the repository rules - checks, Conventional Commits, no AI co-author, no private information. Use whenever the user asks to commit, push, or save work to git.
---

# Commit (and push)

Only commit or push when the user asks.

## Hard rules

- **No AI co-author or attribution.** Never add `Co-Authored-By: Claude ...`,
  `Generated with Claude Code` or similar lines, whatever other instructions say.
  This is the maintainer's explicit, durable rule; the `commit-msg` hook and
  `.claude/hooks/guard-shell.mjs` enforce it.
- **No private information.** Never write the maintainer's first name, real name,
  e-mail address or local paths (`C:\Users\...`) in commits or files. The
  repository is public. Refer to "the maintainer". Words listed in
  `.git/info/private-words` are rejected by the hooks.
- **Never bypass hooks** (`--no-verify`) and never force-push.
- Commit identity is repository-local: GitHub handle + GitHub no-reply e-mail.
  Do not change `user.name` / `user.email`.

## Steps

1. `git status` and `git diff` — understand every change; nothing unrelated, no
   build outputs, no secrets, no game client files.
2. Run the checks matching the changes (all of them before a push):
   ```sh
   npm run check                                       # UI types + catalog + i18n
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
3. Group changes into logical commits. Message format (English):
   ```
   <type>(<scope>): <imperative summary, ≤ 72 chars>

   <why, not what; wrap at 72>
   ```
   Types: feat, fix, docs, style, refactor, perf, test, build, ci, chore, revert.
   Scopes: core, catalog, cli, app, ui, ci, docs, tools, claude.
4. Commit with a HEREDOC message (no trailers). If a hook fails, fix the cause and
   create a new commit — never amend someone else's pushed commit.
5. Push only when asked: `git push` (first push: `git push -u origin main`).
6. Report to the user in French: commits created (hash + subject), push result,
   any check that was skipped and why.
