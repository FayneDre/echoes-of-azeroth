# 0003. Full build with all catalog modules

- Status: Accepted
- Date: 2026-11-13

## Context

AzerothCore modules are compiled into `worldserver`. Users must be able to add
or remove optional modules at any time from the launcher; Playerbots is
mandatory and requires the `mod-playerbots/azerothcore-wotlk` fork.
Per-combination builds mean 2ⁿ binaries and 30–60 min waits.

## Decision

CI builds one binary per flavor and catalog release with every catalog module
compiled in. The launcher enables/disables modules via their `activation`
config, controls module SQL through dbimport `Updates.AllowedModules`, and
regenerates the `world` database on removal. Player-data databases are never
regenerated.

## Consequences

- Installing a module takes seconds, no binary download.
- Each module must be proven inert when disabled (CI test) — otherwise it cannot
  be in the catalog as removable.
- Adding a module to the catalog means a new full build.
- Custom modules outside the catalog will need a local build (later).
