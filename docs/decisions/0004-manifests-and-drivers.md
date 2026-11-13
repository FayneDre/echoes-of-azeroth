# 0004. Declarative manifests + per-emulator drivers

- Status: Accepted
- Date: 2026-11-13

## Context

AzerothCore is first, but other emulators (TrinityCore, CMaNGOS, vMaNGOS...),
their clients and modules must be addable without rewriting the launcher.

## Decision

- Everything describable is data: JSON manifests under `catalog/`, validated by
  JSON Schema (`catalog/schema/`) and by `eoa-catalog` for cross-references.
- Flavor and module ids are unique within an emulator; files are named after ids.
- What cannot be declared (account creation, console, config dialect) is a Rust
  `EmulatorDriver` implementation, selected by the emulator's `driver` field.
- The engine exposes a closed, tested step vocabulary.
- Manifest types in `eoa-core` mirror the schemas with `deny_unknown_fields`.

## Consequences

- Schema and Rust types must change together (enforced by tests on the real catalog).
- `schemaVersion` gates compatibility; breaking changes bump it.
- No dynamic plugins (WASM/scripts) for now: simpler and safer for a public app.
