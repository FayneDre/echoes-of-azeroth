# Catalog

Declarative description of everything the launcher can install. The launcher
engine is generic; adding an emulator, client, component or module starts here.

```
catalog/
├─ schema/                         JSON Schemas (draft 2020-12), one per manifest kind
├─ clients/<id>.json               game clients (detection, realmlist, patches, server data)
├─ components/<id>.json            shared runtimes (MySQL 8.4, ...)
└─ emulators/<emulator>/
   ├─ emulator.json                driver, client, databases, ports, processes
   ├─ flavors/<id>.json            core source trees (official repo or fork), required modules
   └─ modules/<id>.json            modules (activation, SQL, config, options, assets, post-install)
```

## Rules

- File name == `id` (kebab-case). Flavors/modules: `emulator` == parent directory.
- Every upstream source is pinned to a full 40-char commit; `ref` only documents the branch.
- User-facing text is `{ "en": "...", "fr": "..." }`; English is mandatory.
- Config values follow upstream `.conf` conventions (booleans are `0`/`1`).
- A module listed in a flavor's `requiredModules` must be `"removable": false`.
- A removable module must use `activation.mode = "config"` and be inert when disabled.
- Schema changes must be mirrored in `crates/eoa-core/src/manifest.rs` (and vice versa).

## Validate

```sh
npm run validate:catalog                      # JSON Schema + layout
cargo run -p eoa-cli -- catalog validate      # cross-manifest references
cargo run -p eoa-cli -- catalog resolve --emulator azerothcore --module mod-ah-bot
```
