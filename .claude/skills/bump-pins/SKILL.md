---
name: bump-pins
description: Update pinned upstream commits (core flavors, modules) and component versions in the Echoes of Azeroth catalog. Use when the user asks to update AzerothCore, the Playerbot fork, a module, or MySQL.
---

# Bump pinned versions

1. For each manifest to update, read `source.repo` and `source.ref`, then:
   ```sh
   gh api repos/<owner>/<repo>/commits/<ref> --jq '.sha + " " + .commit.committer.date'
   gh api repos/<owner>/<repo>/compare/<old>...<new> --jq '.commits[].commit.message' | head -50
   ```
2. Scan the compare for breaking signals: renamed/removed config keys, new SQL
   directories, new required core changes, new databases. Diff the `.conf.dist`:
   ```sh
   gh api repos/<owner>/<repo>/contents/<path>.conf.dist?ref=<new> -H "Accept: application/vnd.github.raw"
   ```
3. Update `commit`, and any key/option/override affected. Keep Playerbots on the
   fork commit that its module version expects (they move together).
4. MySQL: stay on 8.4 LTS (AzerothCore rejects 9.x/26.x). Update `version` and `upstream`.
5. Validate: `npm run validate:catalog` and `cargo test -p eoa-catalog`.
6. Summarise for the user (in French): old → new commit, notable upstream changes,
   anything needing a CI smoke test.
