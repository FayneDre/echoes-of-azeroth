---
name: add-emulator
description: Add support for a new emulator family (e.g. TrinityCore, CMaNGOS, vMaNGOS) or a new client version to Echoes of Azeroth. Use when the user wants another emulator, core flavor or game client.
---

# Add an emulator, flavor or client

The engine is generic; a new emulator is **manifests + one driver**. If you find
yourself changing the engine for emulator-specific behaviour, stop and move that
behaviour into the driver (see `docs/decisions/0004-manifests-and-drivers.md`).

## Checklist

1. **Research** (gh api / official docs, never memory): supported database server
   and version, build system, required data (dbc/maps/vmaps/mmaps), config file
   names and DB connection keys, processes and ports, account creation method
   (console, SOAP, SQL), client version and build number.
2. **Client**: reuse an existing `catalog/clients/*.json` if the client version
   matches, otherwise add one (detection build, locales, realmlist format, patch dir).
3. **Component**: reuse `mysql-8-4` if compatible; otherwise add a component manifest.
   Different emulators may need different database versions; instances are isolated.
4. **Emulator**: `catalog/emulators/<id>/emulator.json` (driver name, client,
   components, databases with every connection key, ports, processes + readiness).
5. **Flavors**: one per source tree (official / fork) with pinned commits and
   `requiredModules`.
6. **Modules**: use the `add-module` skill.
7. **Driver**: implement the `EmulatorDriver` trait in `crates/drivers/<id>`
   (once the trait exists, phase 3). Keep it small.
8. **Validate**: `npm run validate:catalog`, `cargo test --workspace`.
9. **Docs**: update `CLAUDE.md` (verified facts section), `docs/architecture.md`
   if the model had to grow, and add an ADR for any new decision.
