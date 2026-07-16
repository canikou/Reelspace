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

## Build And Run

```bat
build-run.bat
```

## Installer

```bat
dist-setup.bat
```

The installer artifacts are copied to `dist`.
