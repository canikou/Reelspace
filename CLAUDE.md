# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Reelspace is a local-first Tauri 2 desktop app (Windows x64, Linux x86_64) for indexing gameplay recordings, organizing them (tags, notes, games, collections), and cutting clips with FFmpeg. Frontend: React 18 + TypeScript + Vite (`src/`). Backend: Rust (`src-tauri/`). Package manager: pnpm. Early alpha.

## Commands

One-time setup (FFmpeg/FFprobe are not in git; they get staged into `vendor/ffmpeg/bin/`):

```powershell
pnpm install --frozen-lockfile
powershell -File scripts/prepare-ffmpeg.ps1   # Linux: ./scripts/prepare-ffmpeg.sh
```

Dev and build:

```powershell
pnpm tauri:dev                 # app + Vite dev server on 127.0.0.1:1420 (strictPort)
pnpm build                     # tsc + vite build (frontend only)
pnpm tauri:build --config src-tauri/tauri.unsigned.conf.json   # local package without updater signing key
```

`build-run.bat` / `build-setup.bat` are the maintainer's Windows wrappers around `scripts/runner.ps1`. They log to `logs/runners/`. `build-setup.bat` expects the maintainer's DPAPI-protected updater signing key.

Validation (CI in `.github/workflows/validate.yml` runs exactly these, on Windows and Linux):

```powershell
pnpm test                                                      # vitest run --dir src
pnpm vitest run src/lib/api.test.ts                            # single frontend test file
pnpm vitest run -t "name substring"                            # single test by name
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo test --manifest-path src-tauri/Cargo.toml <test_name>    # single Rust test
cargo test --manifest-path src-tauri/Cargo.toml --test library_faults   # one integration test file
./scripts/check-version.ps1                                    # version sync check
./scripts/test-release-media.ps1                               # end-to-end media pipeline on sanitized fixtures (Windows)
```

Clippy runs with `-D warnings`, so any warning fails CI. `#[cfg(target_os = "linux")]` code (recorder, media server, tray) is only compiled and linted on Linux. Keep it clean without running it locally on Windows.

GNOME extension tests: `gjs -m packaging/gnome-extension/test/osd.test.js`.

## Versioning

The version must match in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` (the `reelspace` package entry), and `src-tauri/tauri.conf.json`. `scripts/check-version.ps1` enforces this. Release notes go in `docs/releases/v<version>.md`.

## Architecture

### Frontend ↔ backend boundary

- All backend calls go through `src/lib/api.ts`, which wraps `invoke()`. Shared shapes live in `src/lib/types.ts` and mirror `src-tauri/src/models.rs` (serde `camelCase`).
- Backend errors are `AppError { code, message, ... }` from `src-tauri/src/error.rs`. The `ErrorCode` enum is duplicated as the `ERROR_CODES` set in `api.ts` and the `AppErrorCode` type. Adding a code means updating all three.
- Adding a Tauri command means: write `#[tauri::command]` in `commands.rs` (core library/media), `features.rs` (games, collections, storage, archive, recorder-app detection), or `youtube.rs`. Then register it in `generate_handler!` in `src-tauri/src/lib.rs` and add a wrapper in `api.ts`.
- `App.tsx` holds app-level state and does page switching with a `PageKey` from `Sidebar`. There is no router. Pages in `src/pages/` get data and action callbacks as props.

### Backend state and workers (`src-tauri/src`)

- `state.rs` `AppState` is cloned into every command and worker. It holds the SQLite `Database` (`db.rs`), the `Ffmpeg` handle, telemetry, the recovery journal, and three single-slot `MediaJobQueue`s from `jobs.rs` (`background_jobs`, `media_jobs`, `export_jobs`). These throttle FFmpeg/FFprobe work by priority.
- `lib.rs::run()` starts long-lived background threads: the thumbnail worker, the YouTube upload worker, and the filesystem watcher (`watcher.rs`, `notify`). On Linux it also starts the loopback media server and the tray.
- Asset access is allow-listed. `tauri.conf.json` ships an empty `assetProtocol.scope`. `AppState::authorize_settings_assets` grants the source, working, and clip folders plus app-managed dirs at startup and whenever settings are saved. A new folder setting that the UI must read needs to be added there.
- Linux playback: WebKitGTK/GStreamer cannot stream `asset://`. `media_server.rs` serves allowed files over `http://127.0.0.1:<port>/<token>/...` with Range support. The frontend gets the base from the `media_server_base` command. On Windows the result is `None` and the frontend uses `convertFileSrc`.
- `ffmpeg.rs` resolves bundled binaries from `vendor/ffmpeg/bin` (Tauri resources). Debug builds fall back to `<repo>/vendor/ffmpeg/bin` through `CARGO_MANIFEST_DIR`.
- `db.rs`: rusqlite (bundled). Migrations are keyed on `PRAGMA user_version` against `DATABASE_SCHEMA_VERSION`. Schema changes bump that constant and add a migration step. Existing tests check that migrations keep organization data. `Database::open_resilient` backs up and rebuilds a corrupt DB. `recovery.rs` detects unclean shutdowns and resumes persistent jobs.
- `portability.rs` handles metadata-only library export and import (JSON). Matching and conflict rules are documented in `docs/library-data.md`.
- `youtube.rs` handles optional private YouTube archival: OAuth desktop flow, tokens in the OS keyring, and a resumable persistent upload queue. The client ID can be overridden at compile time with `REELSPACE_YOUTUBE_CLIENT_ID`. See `docs/youtube-archive.md`.
- `telemetry.rs` writes local rotated diagnostics only. Routine logs must not contain full media paths.

### Linux recorder (`src-tauri/src/recorder/`, binary `reelspace-recorder`)

This is a separate daemon binary (`src/bin/reelspace-recorder.rs`), run as a systemd user service (`packaging/linux/reelspace-recorder.service`). It supervises `gpu-screen-recorder` (replay buffer plus rolling/session recordings) and exposes D-Bus `org.reelspace.Recorder1`. The config file is `~/.config/reelspace/recorder.json`, defined by `Config` in `recorder/config.rs`. The main app talks to the daemon through `recorder_app.rs`. The optional GNOME Shell extension in `packaging/gnome-extension/` shows an OSD and sends focused-window changes. The full D-Bus contract and storage-budget rules are in `docs/recorder-dbus.md`. Keep that doc in sync with interface changes.

### Tests

- Rust unit tests sit inline (`#[cfg(test)]`) in most modules. `src-tauri/tests/` has integration tests (`library_faults.rs`, `recorder_supervisor.rs`). `src-tauri/examples/` holds a library benchmark and the release media smoke test used by `test-release-media.ps1`.
- Frontend tests are colocated `*.test.ts(x)` files under `src/`, run with vitest.

### Packaging and release

- Tauri config layering: `tauri.conf.json` (base) + `tauri.prod.conf.json` (used by `pnpm tauri:build`) + `tauri.unsigned.conf.json` (no updater signing) + per-OS `tauri.windows.conf.json` / `tauri.linux.conf.json`.
- `.github/workflows/release.yml` builds releases. The updater manifest is published to the `updater-alpha` GitHub release. Signing needs `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- `packaging/arch/PKGBUILD` handles Arch packaging.
