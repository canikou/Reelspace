# Reelspace

Your gameplay recordings, organized locally.

Reelspace is a Windows desktop library for browsing large capture folders, playing recordings, adding your own organization, and turning useful moments into clips—without uploading your media to a service.

> [!WARNING]
> Reelspace is early alpha software. Back up important recordings and exported library data. The installer is not code-signed, so Windows SmartScreen may show an unknown-publisher warning.

## What works today

- Incremental scanning for large recording folders, including persistent probe caching and background thumbnail recovery
- A responsive gallery with in-window playback and Windows Explorer integration
- Custom tags, notes, collections, and collection membership
- Trim definitions and FFmpeg-powered clip generation that retain a reference to the source recording
- Storage usage warnings and a confirmed raw-recording cleanup flow
- Metadata-only export and import for tags, notes, collections, memberships, and clip references
- Local diagnostics, unclean-shutdown detection, bounded job retries, and conservative database recovery

## Install on Windows

Reelspace currently supports Windows 10/11 x64.

1. Open the [latest Reelspace release](https://github.com/canikou/Reelspace/releases/latest).
2. Download `Reelspace_0.1.1-alpha.1_windows_x64_setup.exe` and its `.sha256` file.
3. Optionally verify the download in PowerShell:

   ```powershell
   (Get-FileHash .\Reelspace_0.1.1-alpha.1_windows_x64_setup.exe -Algorithm SHA256).Hash
   Get-Content .\Reelspace_0.1.1-alpha.1_windows_x64_setup.exe.sha256
   ```

   The two hashes should match.

4. Run the installer. If SmartScreen appears, inspect the publisher warning and choose **More info** only if the filename and checksum match this GitHub release.

## Getting started

Open **Settings**, add one or more folders containing recordings, and save. Reelspace indexes supported local media and fills thumbnails in the background. Configure a working folder and clip folder before exporting clips.

Use **Export Library Data** in Settings to create a portable JSON archive of your organization metadata. The archive contains file references, not video files. After moving to another computer or drive, index the local recordings first and then import the archive; Reelspace restores data only for safely matched local files. See [portable library data](docs/library-data.md) for the exact matching and conflict rules.

## Privacy and local data

Reelspace is local-first. It has no account, cloud sync, advertising, analytics service, or hosted crash-report collector. Your database, thumbnails, previews, recovery reports, and rotated diagnostics remain in your Windows application-data directory. Routine logs avoid full media paths, but review any diagnostic file before sharing it publicly.

Exported library-data archives may contain filenames, folder references, tags, notes, and collection names. Treat them as personal data even though they contain no video or audio.

## Known limits

- Windows x64 only; the installer is currently unsigned.
- Recordings must remain available on a local or Windows-accessible filesystem for playback, scanning, and clip creation.
- There is no cloud backup, automatic updater, or online video archival.
- Import carries organization metadata and file references, not media or application settings.
- This alpha may still contain rough edges. Keep backups and verify generated clips before deleting any source recording.

## Roadmap

Near-term work focuses on large-library performance, resilient recovery, richer local organization, and safer data portability. Plans may change as the alpha is tested.

Found a problem? [Open an issue](https://github.com/canikou/Reelspace/issues/new/choose) with reproduction steps and sanitized diagnostics. For security-sensitive reports, use [GitHub private vulnerability reporting](https://github.com/canikou/Reelspace/security/advisories/new). Do not attach personal recordings or unreviewed library-data exports.

## Building from source

Source builds require Windows 10/11 x64, Node.js 20+, pnpm, the stable Rust toolchain with `rustfmt` and `clippy`, and the Windows C++ build tools used by Tauri.

```powershell
pnpm install --frozen-lockfile
powershell -File scripts/prepare-ffmpeg.ps1
pnpm build
pnpm test
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
pnpm tauri:build
```

The FFmpeg preparation script downloads one pinned BtbN Windows build, verifies its archive and executable hashes, and stages only FFmpeg, FFprobe, and their license for packaging. See [third-party notices](THIRD_PARTY_NOTICES.md).

Release candidates also run `scripts/test-release-media.ps1`. With no arguments it creates and removes its own sanitized fixtures; a custom source folder may contain one valid recording and at least one malformed `.mp4`. The harness exercises scanning, failure isolation, thumbnailing, accurate clipping, persistence, and metadata export/import without writing into the source folder.
