# 0005. Open source distribution and signing

- Status: Accepted
- Date: 2026-11-13

## Context

Public audience on Windows: unsigned executables trigger SmartScreen and
antivirus warnings. Artifacts are large (server data ~1.5–2 GB). Distributed
AzerothCore binaries are GPL-2.0.

## Decision

- Public GitHub repository.
- Artifacts on GitHub Releases first (files chunked under 2 GB); the signed
  index lists multiple mirrors per artifact so Cloudflare R2 can be added later.
- Authenticode signing through the SignPath Foundation open-source program,
  from public CI builds.
- Tauri updater signed with a project key; catalog index signed with ed25519.
- Manifests pin exact upstream commits, which satisfies GPL source availability.

## Consequences

- Release builds must come from CI, never from a developer machine.
- SmartScreen reputation still builds over time; prepare a help page for false positives.
- Launcher licensed GPL-3.0-or-later (OSI-approved, required by SignPath; consistent with the GPL emulator ecosystem).
