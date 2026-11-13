# 0006. Server data downloaded from upstream, not re-hosted

- Status: Accepted
- Date: 2026-11-13

## Context

AzerothCore needs pre-extracted server data (dbc, maps, vmaps, mmaps, Cameras).
The AzerothCore tooling (`acore.sh`) downloads it from the
`wowgaming/client-data` GitHub releases (v20.0: `Data.zip`, 1 196 168 257 bytes,
sha256 published by GitHub). The data is derived from Blizzard's client and the
repository declares no license. ADR 0005 planned to re-host every artifact.

## Decision

- The launcher downloads server data **directly from the pinned upstream release**
  (`catalog/clients/<id>.json` → `serverData.url`, `version`, `size`, `sha256`).
- The project never re-hosts Blizzard-derived data. Our own artifacts (binaries,
  SQL, MySQL repackaging) keep following ADR 0005.
- Integrity is enforced by the pinned sha256; a mismatch aborts the install.
- `bump-pins` covers data releases too (new tag → new url/size/sha256).

## Consequences

- Availability depends on upstream; the mirror list mechanism may later point to
  other community mirrors of the *same* file (same sha256), never to a copy we host.
- The download (~1.1 GB) is the largest install step: parallel ranged download and
  resume are mandatory in `eoa-store`.
- Extracting map data from the user's own client stays possible as a future fallback.
