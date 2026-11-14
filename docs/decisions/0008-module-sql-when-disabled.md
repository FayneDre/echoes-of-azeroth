# 0008. Module SQL applied while disabled, per manifest flag

- Status: Accepted
- Date: 2026-11-14

## Context

ADR 0003 compiles every catalog module into one build and assumes a module
that is compiled in but disabled by config is inert, so its SQL is applied
only when the player chooses it (`Updates.AllowedModules`).

A local end-to-end install (2026-11-14, fork `32fd347`, mod-ah-bot `c11d831`)
disproves this for AHBot: with `AuctionHouseBot.EnableSeller = 0` and
`EnableBuyer = 0`, worldserver still queries `acore_world.mod_auctionhousebot`
at startup and aborts with "Your database structure is not up to date" when
the module's SQL was not applied.

AHBot's SQL (`data/sql/db-world/*.sql`) only creates and fills its own tables
(`mod_auctionhousebot`, `mod_auctionhousebot_disabled_items`,
`auctionhousebot_professionItems`). Individual Progression's SQL, by contrast,
changes game content in core tables: applying it while disabled would change
the game for players who did not choose it.

## Decision

- Module manifests get `database.applyWhenDisabled` (default `false`). When
  `true`, the module's SQL is allowed in dbimport whether or not the module is
  chosen; its config keeps it disabled.
- Only for SQL limited to the module's own tables, checked against the pinned
  commit's `data/sql` when the flag is set or the pin is bumped (`add-module`
  and `bump-pins` skills).
- mod-ah-bot: `applyWhenDisabled: true`.
- The CI smoke test reads the flag from the catalog: round A ("modules off")
  allows mandatory modules plus flagged ones and checks that exactly the
  allowed modules' SQL is applied; both rounds must start the servers.

## Consequences

- Inertness now means "disabled by config, with its own tables present" for
  flagged modules; removing such a module leaves its (inert) tables in
  `world`, which ADR 0003 regenerates anyway.
- A module whose SQL is not limited to its own tables and that also reads its
  tables while disabled cannot be supported by the full build without a
  change upstream; the smoke test reveals such cases.
