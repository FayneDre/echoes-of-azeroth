# 0010. Signed artifact index

- Status: Accepted
- Date: 2026-11-14

## Context

The launcher installs a server from two CI artifacts: the server build and
its database snapshot (ADR 0009). Today they are passed by hand
(`rk install --server --db-snapshot`). ADR 0005 calls for an ed25519-signed
index that lists every artifact with mirrors and hashes, so the launcher can
find them by itself and trust nothing it has not verified.

Constraints:

- GitHub Releases hosts the files (2 GB per asset). Replacing release assets
  is not atomic: if the index and its signature were two files, a launcher
  could read the new one with the old signature.
- Artifact names and download URLs are predictable. Integrity must come from
  the signature, never from where a file was downloaded.
- An attacker who controls a mirror can serve an *older*, validly signed
  index (rollback) to bring back a build with a known flaw.
- The build key (ADR 0003, `tools/ci/build-plan.mjs`) hashes CI internals
  (recipe, scripts). The launcher cannot recompute it. What it knows are the
  catalog pins: the core and module repositories and commits.

## Decision

- **Envelope.** One file, `index.json`:
  `{"format": 1, "keyId": "<16 hex>", "signature": "<128 hex>", "payload": "<JSON text>"}`.
  The signature is Ed25519 (RFC 8032, verified strictly) over
  `"realmkeeper/index/v1\n"` followed by the UTF-8 bytes of `payload`.
  These are the exact bytes that were signed, so no canonical JSON is
  needed. Because a single file carries both, replacing it is atomic.
- **Keys.** `keyId` is the first 16 hex digits of the SHA-256 of the raw
  32-byte public key. The trusted public keys are compiled into the launcher
  (`rk-catalog`). Several can be trusted at once (rotation), and a launcher
  release revokes a key by removing it. The private key exists only as a
  GitHub Actions secret (`RK_INDEX_SIGNING_KEY`, PKCS#8 PEM). It is never
  written to the repository or the runner disk.
- **Payload.** `{"format": 1, "serial": <u64>, "published": "<RFC 3339>", "builds": [...]}`.
  Each build entry carries `emulator`, `flavor`, `platform` and `buildKey`,
  plus its pins: `core` {repo, commit} and `modules` {id: commit} for every
  module compiled in. It lists its artifacts: `server` and `dbSnapshot`, each
  with `urls` (mirrors, tried in order), `size` and `sha256`. `dbSnapshot`
  also records `mysql` {id, version} and its base `modules`.
- **Anti-rollback.** `serial` increases with every publication. The launcher
  keeps the last verified envelope in `state/index.json`. It refuses a lower
  serial, and also refuses the same serial with a different payload.
- **Selection.** The launcher picks a build whose emulator, flavor and
  platform match, and whose pins equal those of its catalog: the flavor's
  source and every compatible module's commit. Among several, the last
  listed wins (newest recipe). No match means the catalog is ahead of
  published builds, which produces an explicit error.
- **Trust chain.** Envelope signature → payload → each artifact's size and
  sha256, verified by `rk-store` before any file is used.
  `snapshot.json` and `build-info.json` are still checked by the install
  (ADR 0009).
- **Publication.** CI creates one GitHub release per build, with the tag
  `build-<emulator>-<flavor>-<buildKey>` holding the server and snapshot
  ZIPs. The index is the `index.json` asset of the fixed release `index`.
  Only `main` publishes, after a green smoke test. The previous index is
  read and the new build is appended (or replaces the same build key) with
  `serial + 1`. Signing is done by `tools/ci/index.mjs` (Node `crypto`).

## Consequences

- The launcher can install with no file argument, and any mirror can be
  added later without trusting it.
- Losing the signing key means a launcher release with a new trusted key.
  The maintainer keeps an offline backup of it.
- There is no expiry date yet, so a launcher that has never seen a newer
  index can be held on an old one (freeze). That old index is still signed
  and hash-checked. An `expires` field can be added with a scheduled
  re-signing.
- The catalog itself is still bundled with the launcher. Shipping it
  through the index is later work.
- Compression (zstd) changes only the artifact files and their hashes, not
  this format.
