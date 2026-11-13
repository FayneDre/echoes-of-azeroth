---
name: add-module
description: Add or update a server module in the Echoes of Azeroth catalog (manifest, pinned commit, config keys, options, assets, post-install). Use when the user asks to support a new module (e.g. mod-transmog) or change an existing module manifest.
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
- Never commit client files: assets reference CI artifacts.

## 3. Validate

```sh
npm run validate:catalog
cargo test -p eoa-catalog
cargo run -p eoa-cli -- catalog resolve --emulator <emulator> --module <id>
```

## 4. Report

Tell the user (in French) what was verified, what is assumed, and which CI checks
phase 1 must add (inertness when disabled, SQL naming, post-install).
