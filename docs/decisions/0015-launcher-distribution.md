# 0015. Launcher distribution: installer, updates, signing, crash reports, zstd artifacts

- Status: Accepted
- Date: 2026-11-15

## Context

The launcher must reach a public audience on Windows (ADR 0005):
- no admin rights (principle 6);
- signed binaries;
- updates without a manual download;
- downloads that stay reasonable on slow connections.

Facts:

- **Installer modes.** Tauri's NSIS bundle supports a per-user install
  (`installMode: currentUser`, no elevation). Its MSI bundle installs per
  machine.
- **Updater.** `tauri-plugin-updater` 2.14 checks a static JSON feed and
  verifies each download with a minisign public key compiled into the app.
  On Windows it runs the NSIS installer and the app restarts.
- **Build-time key.** `bundle.createUpdaterArtifacts` makes the build fail
  without the private key, so a release config cannot enable it
  unconditionally.
- **zstd, measured on 2026-11-15** on build `b8a8e89ebf07aeb9`:
  - deflate: server 256 MB, snapshot 144 MB;
  - zstd level 19: server 173 MB (12: 199 MB, 3: 238 MB), snapshot 94 MB;
  - packing at level 19 takes 8 minutes on one core, once per build.

  An install from the zstd archives took 30 s and the servers started.
  Node 24 has `zlib.zstdCompressSync` and `crc32`. The `zip` crate reads
  zstd entries (method 93) with its `zstd` feature.

## Decision

1. **Installer.**
   - NSIS only, per user, with English and French. The MSI target is
     dropped because it needs admin rights.
   - Releases come only from `.github/workflows/release-app.yml`, run by
     hand. It produces `app-v<version>`, a draft by default for the closed
     beta.
2. **Code signing (SignPath).** The workflow uploads the unsigned installer
   and, when `SIGNPATH_API_TOKEN` and the SignPath variables are set,
   submits it to SignPath and publishes the signed result. Without them it
   publishes the installer unsigned, with a warning.
3. **Auto-update.**
   - **When it is on.** It is enabled only when the variable
     `EOA_UPDATER_PUBKEY` and the secret `TAURI_SIGNING_PRIVATE_KEY` exist.
     `tools/ci/app-release.mjs config` then adds `plugins.updater` (public
     key, feed URL, passive install mode) and `createUpdaterArtifacts` to the
     release build.
   - **How it is signed.** The update signature is made after SignPath, over
     the final installer bytes.
   - **Where the feed lives.** `latest.json` is in the rolling
     `app-latest` release. It is updated only by non-draft releases, so a
     beta never reaches players.
   - **Without it.** Builds without the feed (local, debug) do not register
     the plugin, and the UI shows nothing.
   - **In the app.** It checks once at start. A failed check is silent,
     since offline is normal. Installing is refused while servers run or a
     change is in progress, because the installer closes the launcher.
4. **Opt-in crash reports.**
   - A panic hook writes `crash-reports/crash-<ms>.txt` under the data
     root: version, OS, panic message and backtrace.
   - Personal folders are replaced by `%USERPROFILE%`-style names, and the
     user name by `<user>`.
   - At the next start the UI offers to view the report, to open a
     pre-filled GitHub issue (the player reads it and submits it, or not),
     or to ignore it.
   - Nothing is ever sent by the launcher, and there is no telemetry
     service. At most 10 reports are kept.
5. **zstd artifacts.** The publish job of `build-server.yml` packs the
   server build and the database snapshot with `tools/ci/zip-zstd.mjs`. It
   writes a ZIP with zstd entries at level 19, deterministically: sorted
   entries, fixed timestamps, directory entries. The container stays a ZIP,
   so the extraction code (parallel, prefix strip, subdirectories) is
   unchanged. A server build already published with deflate stays valid.

## Consequences

- **What the maintainer must provide:**
  - a SignPath Foundation project (token, organization, project and policy
    slugs);
  - an updater key pair made with `npx tauri signer generate`: the public
    key goes in a repository variable, the private key and its password in
    secrets.

  Until then, releases are unsigned and do not update themselves.
- **What gets signed.** SignPath signs the installer. The launcher
  executable inside it is signed only if the SignPath artifact
  configuration signs the embedded files; otherwise, signing it during
  the Tauri build (`bundle.windows.signCommand`) is follow-up work.
- **Archive limits.** The packer writes no ZIP64. Archives and entries over
  4 GiB, or more than 65,535 entries, are refused at packing time; the
  current artifacts are far below these limits.
- **Verified locally on 2026-11-15:**
  - a release build with the `app-release.mjs config` override (throwaway
    key) produces the 5 MB per-user installer and its updater signature;
  - `tauri signer sign` re-signs an installer, and `app-release.mjs feed`
    writes the feed;
  - the installer needs no elevation.
- **Not verified yet:** the in-app install of an update from the feed, which
  needs an installed test build.
- **Assumed, not verified in CI yet:** the SignPath action inputs
  (`signpath/github-action-submit-signing-request@v1`) and the first public
  run of `release-app.yml`. Both run only once the maintainer sets the
  secrets.
