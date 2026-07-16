# Architecture

Reelspace is split into small, agent-friendly compartments.

## Frontend

- `src/App.tsx` owns screen routing and shared modal state.
- `src/lib` owns typed API calls and formatting helpers.
- `src/components` owns reusable UI pieces such as cards, rails, stats, player, and trim modal.
- `src/pages` owns top-level page composition for Home, Gallery, Clips, Collections, and Settings.

## Backend

- `commands.rs` exposes Tauri commands and keeps business logic out of command handlers.
- `db.rs` owns SQLite schema, migrations, and persistence.
- `media.rs` owns scanning, tag inference, thumbnails, and clip item construction.
- `ffmpeg.rs` owns FFmpeg/FFprobe discovery and process execution.
- `storage.rs` owns storage accounting and raw cleanup candidate ordering.
- `explorer.rs` owns Windows Explorer reveal/select behavior.
- `models.rs` owns shared serializable types.
- `state.rs` owns app-level paths and shared services.

## Media Model

Raw files are source-of-truth records. Clips are generated files that reference the raw source path plus trim start/end metadata. Cleanup actions are intentionally restricted to raw files.
