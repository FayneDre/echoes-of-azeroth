# 0002. MySQL 8.4 LTS, portable

- Status: Accepted
- Date: 2026-11-13

## Context

AzerothCore requires MySQL 8.4 LTS. MariaDB and MySQL 5.7/8.1 were dropped on
2024-09-19 (azerothcore-wotlk PR #19451); MySQL "innovation" releases (9.x/26.x)
are not supported. A native MySQL service would need admin rights, conflict
with existing installs and run permanently.

## Decision

Ship MySQL 8.4 LTS as a portable component (official no-install ZIP, trimmed and
repackaged by CI), one instance per realm, bound to `127.0.0.1` on a free port,
started/stopped by the launcher. Version pinned and tested in CI.

## Consequences

- Clean shutdown is mandatory (`mysqladmin shutdown`); Job Objects prevent orphans.
- Backups before every migration; datadir refused inside cloud-synced folders.
- Security patches are our responsibility (CI republishes the component).
- An "external database" mode remains available for advanced users.
