# Contributing to Echoes of Azeroth

Thanks for helping! This guide covers setup, workflow and the rules every
contribution must follow. Architecture lives in [`docs/architecture.md`](docs/architecture.md).

## 1. Setup (Windows)

| Tool | Version | Install |
|---|---|---|
| Rust | stable (pinned by `rust-toolchain.toml`) | `winget install Rustlang.Rustup` |
| Visual Studio Build Tools | 2022, workload "Desktop development with C++" | `winget install Microsoft.VisualStudio.2022.BuildTools` |
| Node.js | 24 | `winget install OpenJS.NodeJS` |
| WebView2 | any | preinstalled on Windows 11 |

```sh
git clone https://github.com/FayneDre/echoes-of-azeroth.git
cd echoes-of-azeroth
npm install            # also activates the git hooks (core.hooksPath = .githooks)
npm run dev            # launcher in development mode
```

## 2. Checks

Run before every push (CI runs the same):

```sh
npm run check                                        # svelte-check + catalog schema + i18n parity
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p eoa-cli -- catalog validate             # cross-manifest references
```

## 3. Git hooks

`npm install` sets `core.hooksPath` to `.githooks/`:

- **pre-commit**: blocks private words (see below) in staged changes, then runs
  catalog validation / i18n parity / `cargo fmt --check` when relevant files are staged.
- **commit-msg**: enforces [Conventional Commits](https://www.conventionalcommits.org)
  and rejects AI co-author or attribution trailers.

### Private words (local only)

To make sure personal information never reaches the public repository, list
words that must never be committed (your real name, personal e-mail, local
user-folder name...) in **`.git/info/private-words`**, one per line (`#` for
comments). The file lives inside `.git/`, so it is never pushed. Hooks reject
commits containing any of them.

Configure a commit identity that does not expose your e-mail, e.g. the GitHub
no-reply address: `git config user.email "<id>+<login>@users.noreply.github.com"`.

## 4. Commit messages

```
<type>(<scope>): <imperative summary>

<optional body explaining why>
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`,
`chore`, `revert`. Scopes: `core`, `catalog`, `cli`, `app`, `ui`, `ci`, `docs`,
`tools`, `claude`. Commits are authored by humans: no AI co-author lines.

## 5. Project rules

- **Catalog**: follow [`catalog/README.md`](catalog/README.md). Pin full commits,
  verify facts upstream, change the JSON Schema and `crates/eoa-core/src/manifest.rs` together.
- **UI text**: every string goes through `t()` and exists in both
  `app/src/lib/i18n/locales/en.json` and `fr.json`.
- **Architecture changes** need an ADR in [`docs/decisions/`](docs/decisions/).
- **Never** commit game client files, Blizzard assets, built servers or secrets.
- **Never** reduce in-game server settings to save resources: we optimise the
  installation, not the game.

## 6. Working with Claude Code

The repository ships shared Claude Code configuration in `.claude/`:

| Path | Purpose |
|---|---|
| `CLAUDE.md` | Project context and rules loaded by Claude Code |
| `.claude/settings.json` | Shared permissions, hooks, no AI co-author attribution |
| `.claude/hooks/guard-shell.mjs` | Blocks AI attribution in commits, `--no-verify`, force pushes, private words |
| `.claude/hooks/after-edit.mjs` | After each edit: private-word scan, catalog validation, i18n parity, rustfmt |
| `.claude/skills/*` | Procedures: `add-module`, `add-emulator`, `bump-pins`, `commit`, `new-adr` |

Personal Claude Code settings go in `.claude/settings.local.json` (git-ignored).
