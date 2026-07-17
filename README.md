# Reelspace

Reelspace is a Rust-based Windows desktop app for managing large raw capture folders, especially Nvidia Shadowplay output.

It provides a responsive gallery, local tags and tag aliases, in-window playback, trim definitions that reference source files, clip generation through FFmpeg, storage limit warnings, raw cleanup assistance, collections, notes, and Windows Explorer integration.

## Prerequisites

- Windows 10/11 with WebView2 runtime
- Rust stable toolchain
- Node.js 20+ and pnpm
- FFmpeg and FFprobe copied to `vendor/ffmpeg/bin/ffmpeg.exe` and `vendor/ffmpeg/bin/ffprobe.exe`

## Build And Run

```bat
build-run.bat
```

This is the only user-facing runner. It prints native Vite, Cargo, and Tauri output directly in the terminal while writing the same live output to a timestamped file under `logs\runners`.

By default it uses Tauri dev mode, which starts the frontend server and Rust desktop app together. Keep the runner open while testing: frontend edits hot-reload and Rust edits rebuild incrementally. Closing the runner window terminates its app, server, and compiler processes. The next startup also removes stale Reelspace processes and frees the dev-server port.

Useful modes:

```bat
build-run.bat --check
build-run.bat --build-only
build-run.bat --dist
```

`--check` runs fast validation without launching. `--build-only` compiles the debug executable without launching it. `--dist` runs the slower optimized installer build once and copies setup artifacts to `dist`. Add `--no-pause` to automation or agent checks so failures return immediately.
