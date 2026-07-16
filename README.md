# Reelspace

Reelspace is a Rust-based Windows desktop app for managing large raw capture folders, especially Nvidia Shadowplay output.

It provides a responsive gallery, local tags and tag aliases, in-window playback, trim definitions that reference source files, clip generation through FFmpeg, storage limit warnings, raw cleanup assistance, collections, notes, and Windows Explorer integration.

## Prerequisites

- Windows 10/11 with WebView2 runtime
- Rust stable toolchain
- Node.js 20+ and pnpm
- FFmpeg and FFprobe copied to `vendor/ffmpeg/bin/ffmpeg.exe` and `vendor/ffmpeg/bin/ffprobe.exe`

## Development

```bat
run.bat
```

## Test

```bat
test.bat
```

This runs TypeScript, the frontend production build, and `cargo check`.

## Build And Run Debug

```bat
build-run.bat
```

This builds the frontend and debug Rust binary, then launches the app. It avoids release optimization and NSIS packaging.

The first executable build can still take several minutes because Rust has to codegen and link the Tauri/WebView2 dependency graph. Use `test.bat` for the fast inner loop.

## Installer

```bat
dist-setup.bat
```

The installer artifacts are copied to `dist`.

This is the slow path. Use it only when you need an optimized distributable installer.
