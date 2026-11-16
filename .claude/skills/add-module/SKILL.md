---
name: add-module
description: Add or update a server module in the RealmKeeper catalog (manifest, pinned commit, config keys, options, assets, post-install). Use when the user asks to support a new module (e.g. mod-transmog) or change an existing module manifest.
---

# Add a module to the catalog

Work from facts in the upstream repository, never from memory. Record unknowns as
open questions instead of guessing.

## 1. Research upstream (gh api)

```sh
gh api repos/<owner>/<repo> --jq '.default_branch, .license.spdx_id, .archived'
gh api repos/<owner>/<repo> --jq .full_name     # canonical name (renames redirect)
gh api repos/<owner>/<repo>/commits/<branch> --jq .sha
gh api "repos/<owner>/<repo>/git/trees/<branch>?recursive=1" --jq '.tree[].path' | grep -E 'conf|sql|optional'
gh api repos/<owner>/<repo>/contents/conf/<file>.conf.dist -H "Accept: application/vnd.github.raw"
gh api repos/<owner>/<repo>/contents/README.md -H "Accept: application/vnd.github.raw"
```

Collect: license, required core/fork, enable/disable keys, SQL folders (which
databases), extra databases, required `worldserver.conf` changes, optional client
files (MPQ) or server DBC overrides, accounts/characters it needs.

## 2. Write `catalog/emulators/<emulator>/modules/<id>.json`

- Start from an existing manifest (`mod-ah-bot.json` = config activation + post-install,
  `mod-individual-progression.json` = core overrides + choice options + assets).
- `id` = file name = module directory = `dbimportId`. It must match the loader
  function the module defines: AzerothCore calls `Add<id with - -> _>Scripts()`.
  Check it: `grep -rn "void Add.*Scripts()" src/*loader*` in the repo. Use the
  **canonical** repo name (`gh api repos/<owner>/<repo> --jq .full_name`): a
  renamed repo (e.g. `mod-ahbot` -> `mod-ah-bot`) still redirects but its loader
  follows the new name. Pin `source.commit` (40 chars).
- `compatibleFlavors`: only flavors you verified.
- `removable: true` requires `activation.mode: "config"` with `enable` and `disable`.
- `database.touches` lists every database receiving SQL; `dbimportId` is the
  name for `Updates.AllowedModules`.
- `config.coreOverrides` only for settings the module *requires*.
- `options`: a few user-friendly settings, labels in `en` and `fr`, defaults = upstream
  defaults unless the user asks otherwise (never throttle in-game behaviour).
- Never commit client files. An asset names an archive of the module repository at
  its pinned commit (`source.path`, `size`, `sha256`; `.zip` or `.7z`) and the members
  it uses (`client` / `server`, each `{from, to}`, ADR 0013). Download the archive
  from the pinned commit to measure size and sha256, and list its members before
  writing them. Re-check both on every pin bump.

## 3. Validate

```sh
npm run validate:catalog
cargo test -p rk-catalog
cargo run -p rk-cli -- catalog resolve --emulator <emulator> --module <id>
```

## 4. Report

Tell the user (in French) what was verified, what is assumed, and which CI checks
phase 1 must add (inertness when disabled, SQL naming, post-install).
