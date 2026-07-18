# Architecture

Reelspace is split into small, agent-friendly compartments.

## Frontend

- `src/App.tsx` owns screen routing and shared modal state.
- `src/lib` owns typed API calls and formatting helpers.
- `src/components` owns reusable UI pieces such as cards, rails, stats, player, and trim modal.
- `src/pages` owns top-level page composition for Home, Gallery, Clips, Collections, and Settings.

## Backend

- `commands.rs` exposes Tauri commands and keeps business logic out of command handlers.
- `db.rs` owns transactional SQLite migrations, organization data, probe-cache records, and persisted thumbnail jobs.
- `media.rs` owns fingerprinted incremental scanning, tag inference, atomic thumbnails, and clip item construction.
- `portability.rs` owns the versioned metadata-only library archive, strict import validation, conservative path remapping, and conflict-aware metadata merging.
- `ffmpeg.rs` owns FFmpeg/FFprobe discovery and process execution.
- `storage.rs` owns storage accounting and raw cleanup candidate ordering.
- `explorer.rs` owns Windows Explorer reveal/select behavior.
- `models.rs` owns shared serializable types.
- `state.rs` owns app-level paths and shared services.

## Media Model

Raw files are source-of-truth records. Clips are generated files that reference the raw source path plus trim start/end metadata. Cleanup actions are intentionally restricted to raw files.

## Reliability Boundaries

- Tauri commands return structured error codes, safe messages, and retryability instead of frontend-parsed strings.
- Probe metadata is disposable and versioned separately from tags, notes, collections, and clip records.
- Routine scans coalesce, do not follow links, exclude generated-output roots, and only remove missing records after an error-free traversal of an available source.
- Thumbnail jobs are deduplicated in SQLite, run with one FFmpeg worker, use bounded retries, and recover queued or interrupted work after restart.
- FFmpeg outputs are written beside their destination under unique partial names, verified, and atomically finalized. A changed source invalidates the operation.
- Production media tools resolve only from bundled application resources. Development builds may use explicit local development fallbacks.
