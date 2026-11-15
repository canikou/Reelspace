# Handoff: audit fixes (branch `handoff/polaris`)

Written 2026-11-15 when Reelspace work moved from Terra (the Windows PC) to Polaris.

## What was done and where it stopped

The work is a full code audit (20 findings) and the fixes for it. Everything is on this branch, on top of `master` (316730b):

| Commit | What |
|---|---|
| `4f9a200` | Audit fixes for findings 2 (partial), 3, 4, 8, 9, 11, 12, 13, 14, 15, 16, 17 (partial), 20 |
| `fc8a110` | Linux recorder fixes for findings 1, 5, 7, 18, 19 |
| `d2d0cd9` | Imported names reject path parts; upload attempts reset on progress |
| `7cfd52d` | Regressions from the fixes above: recorder cap in budget mode, upload attempt cap, OAuth deadline, backslashes on Linux |
| `972aff4` | UI follow-ups: storage-limit round trip, update error + Retry, pending update kept, error text in Trim/Collection dialogs, Settings pickers, Repair busy guard |
| `44ef2b7` | Recorder test race fix (`good_reload_clears_rejected_error_state` was flaky) |
| this commit | `CLAUDE.md`, `scripts/smoke/` (live smoke test), this file |

Verified:
- Windows at `972aff4`:
  - fmt and clippy are clean.
  - Rust tests pass: 72 lib and 4 library_faults.
  - `pnpm test` passes 31/31, and `pnpm build` passes.
- Linux (WSL):
  - clippy is clean.
  - 105 lib, 4 library_faults, and 11 recorder_supervisor tests pass (the last at `44ef2b7`).
  - The gjs OSD test passes.
- Live smoke test, `scripts/smoke`, on Windows: PASS on all 5 steps with 0 console errors.

Where it stopped: all fixes are verified, and the work waits on the maintainer's decisions below. Nothing is merged, released or deployed.

## Next steps

1. Run `scripts/test-release-media.ps1` on this branch's HEAD (Windows). It last passed at `d2d0cd9`.
2. Get the maintainer's decisions below. Then open a PR from this branch to `master`.
3. Remaining open items:
   - Finding 17 (partial): orphaned ffmpeg processes are not killed when the app exits. Orphaned `.reelspace-part` files are already swept at startup.
   - `daemon.rs:225`: a good config reload clears `paused`, even if the user paused during the startup config hold.
   - Test harness: when a recorder test panics, its fake `gpu-screen-recorder` process keeps running. `Rig` has no `Drop`.
   - The library card for the smoke fixture showed "No preview". The thumbnail was probably still being generated; this is not confirmed.

## Decisions waiting on the maintainer

1. **Merging this branch.** Recommended: review the commits, then open a PR to `master`.
2. **Finding 6.** A stable release writes `latest.json` only to `updater-stable`, but the app polls `updater-alpha` (`release.yml:316-331`).
   - Recommended: also publish to `updater-alpha` while in alpha.
   - Alternative: point stable builds at `updater-stable`.
3. **Finding 10.** YouTube delete needs the `youtube.force-ssl` scope. Adding it needs a Google Cloud console change, and every user must reconnect.
   - Recommended: add the scope.
   - Alternative: drop delete-from-YouTube.
4. **Finding 2 on Linux.** Two files whose names differ only in case share one row. Fixing it needs a DB migration.
   - Recommended: leave it for now.
5. **Behaviour changes in these commits.** Revert a hunk to undo one.
   - An empty source folder keeps its rows, because it looks like an unplugged drive.
   - Uploads stop after 8 attempts or on HTTP 400.
   - A bad recorder config at startup starts paused, instead of recording on default settings.
   - A manual scan during a watcher scan returns the current list.
   - `cap_gb`/`max_storage_gb` must be larger than one chunk, and the newest file is never pruned.
6. **CI and packaging.** All recommended:
   - Pin the `release.yml` actions to SHAs, and scope `contents: write` to the publish job.
   - PKGBUILD: add `libayatana-appindicator`, and add `gpu-screen-recorder` as an optdepend.
   - Add a `pnpm audit --audit-level high` gate.
   - Add focus traps to dialogs.

## Gotchas

- **Env var names** (no values here):
  - `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: release signing.
  - `REELSPACE_YOUTUBE_CLIENT_ID`: compile-time override.
  - `CARGO_BUILD_JOBS`: use 2 on shared machines.
  - `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`: does NOT work for this app, because Tauri passes its own browser args.
- **CDP smoke test** (`scripts/smoke/smoke.ps1`, Windows plus a desktop session):
  - It starts `pnpm tauri:dev --config scripts/smoke/cdp.conf.json`. The overlay sets the window's `additionalBrowserArgs` to the Tauri defaults plus `--remote-debugging-port=9333`.
  - `smoke.mjs` then drives the app over CDP, calling `window.__TAURI_INTERNALS__.invoke`.
  - It creates a 5 s fixture in `%TEMP%` and points `sourceFolders` at it. At the end it restores the original settings, kills only its own process tree, and deletes the fixture.
  - It refuses to start if port 9333 is busy.
  - It leaves `%APPDATA%\com.reelspace.app` in place.
  - Outputs: `scripts/smoke/out/` (result.json, console-errors.json, screenshots) and `scripts/smoke/tauri-dev.log`, both gitignored.
  - If the `app.windows` entry in `tauri.conf.json` changes, update `cdp.conf.json` to match. JSON merge replaces the whole array.
  - Do not edit files under `src-tauri/` while it runs: `tauri dev` rebuilds and restarts the app.
- **Recorder tests** (`src-tauri/tests/recorder_supervisor.rs`, Linux only):
  - Do not run them as a background job (`cmd &`) from a non-interactive bash. That starts the tests with SIGINT ignored, the fake recorder cannot trap it, and 4 tests time out.
  - Run them in the foreground. Use `setsid` only in the foreground if you need a process group.
  - Do not pipe the output into `grep`. A leaked fake recorder can hold the pipe open. Write to a file instead.
- `#[cfg(target_os = "linux")]` code is only linted on Linux, and `cfg!(windows)` code only on Windows. Run clippy on both before a PR.
- A debug build plus tests at `-j4` can use more than 4 GB of RAM. Use `-j2` on shared hosts.

## Where each step runs

| Step | Plain Linux (Polaris) | Needs Windows (Terra, book a slot) |
|---|---|---|
| `pnpm install --frozen-lockfile`, `pnpm test`, `pnpm build` (tsc + vite build) | yes | |
| `cargo fmt --check`, `cargo check`, `cargo clippy ... -D warnings`, `cargo test --all-targets` | yes (needs the Tauri Linux deps: webkit2gtk, etc.) | also run clippy here for `cfg(windows)` code |
| `recorder_supervisor` tests, `gjs -m packaging/gnome-extension/test/osd.test.js` | yes (Linux only) | |
| `./scripts/check-version.ps1` | yes, with pwsh | yes |
| `./scripts/test-release-media.ps1` | | yes |
| `scripts/smoke/smoke.ps1` (WebView2 CDP smoke test) | | yes (DESKTOP slot) |
| `pnpm tauri:build` (NSIS installer) | AppImage/deb only | yes |

FFmpeg is not in git. Stage it first with `scripts/prepare-ffmpeg.ps1` (Windows) or `./scripts/prepare-ffmpeg.sh` (Linux). The full command list is in `CLAUDE.md`.
