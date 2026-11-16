# 0014. Launcher services shared by the app and the CLI

- Status: Accepted
- Date: 2026-11-15

## Context

Phases 2 to 4 grew the player actions in the `rk` CLI:
- install, update, modules, options;
- client preparation, play, accounts.

The desktop app needed the same actions with:
- progress and log events for the UI;
- translated errors (principle 8);
- background sessions it can stop.

Copying the CLI code into the Tauri layer would have broken the "thin
adapter" rule (ADR 0001) and doubled every fix. Two facts shaped the design:

- **A completed engine operation does not run again.** It returns
  `AlreadyCompleted` when its plan fingerprint is unchanged. An unfinished
  one refuses a different plan (`PlanChanged`). Resuming an interrupted
  update therefore needs the exact inputs it started with: the artifact path
  and the backup name.
- **A world start writes a lot of console output.** It prints thousands of
  lines in a few seconds, which is too much to send to the UI one event per
  line.

## Decision

1. **New crate `rk-launcher`.** It is the only layer above the engine,
   runtime, client and drivers. `Launcher` holds the catalog, the data root
   (`%LOCALAPPDATA%\RealmKeeperData`, `RK_ROOT` overrides it) and the store.
   It offers:
   - `install`, `update`, `repair` and `resume`;
   - `set_modules`, `rebuilds_for`, `set_options` and `options`;
   - `register_client` and `prepare_client`;
   - `create_account` and the instance listing, which reports any
     unfinished operation.

   The CLI and the Tauri commands only map arguments and output.
2. **Errors carry a stable code.** `LauncherError::to_ui()` gives a
   `UiError{code, params, detail}`.
   - The UI shows `error.<code>` from the locales, filled with `params`.
   - `detail` is the full cause chain, folded, for bug reports.
   - An unmapped error becomes `unknown`.
3. **Progress events.** `progress::EventObserver` turns engine events into
   serializable `OperationEvent`s, with at most one byte-progress event every
   150 ms.
4. **Sessions.** `session::Session` runs one instance, and optionally the
   game, in a tokio task.
   - It emits `SessionEvent`s: process states, readiness with the address,
     log lines, game started and exited, failure, end.
   - It takes console lines and a stop request through a cloneable
     `SessionControl`.
   - The Tauri adapter batches the events every 100 ms.
   - A stop requested during the start stops what was already started
     (processes, then the database) cleanly.
   - Closing the window stops every session cleanly before the app exits.
5. **Pending changes.** `instances/<id>/pending.json` records what the
   `instance-<id>` operation was started with:
   - `install`: the artifact and the snapshot;
   - `update`: the artifact and the backup label.

   `resume` rebuilds the very same plan from it, and so does `install` when
   the files still exist; a store file cleaned up meanwhile is fetched again
   (same content, same path). An install includes its first module
   operation: the build is recorded and the file deleted only after both
   complete. An interrupted `modules-<id>` without a pending change resumes
   from the record saved before it.
6. **Repair.** It is an update to the build of the current release. Its
   backup label `repair-<build>-<time>` makes it a new plan even when the
   build is unchanged. It reinstalls `bin/`, `sql/` and the configs, rebuilds
   the overlay, reruns the post-install steps and dbimport, and dumps every
   database first. Player data is never regenerated.
7. **One change at a time in the app.** Each operation has a cancel button
   that stops at the next step boundary. A change is refused while the
   instance runs, and a start is refused while it changes. The instance
   lock enforces both rules anyway.

## Consequences

- New player actions go into `rk-launcher` first. The UI and the CLI get
  them by adding a command, never by adding logic.
- `update_operation` takes the backup label. Two updates or repairs to the
  same build are distinct operations, but a resumed one is the same.
- Error codes are an API between Rust and the locales. Adding one means
  adding the `error.<code>` keys in both locales, which `check:i18n`
  enforces.
- The UI shows only the last 1,500 lines of each process. The full output
  stays in `logs/<process>.console.log`.
