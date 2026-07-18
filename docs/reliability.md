# Reliability And Recovery

## Local State

Reelspace stores its SQLite database and disposable media caches in the Windows application data directory selected by Tauri. The database uses WAL mode, a busy timeout, foreign keys, transactional schema migrations, and a startup integrity check.

User organization data lives in the `media`, `collections`, and `collection_items` tables. FFprobe responses and thumbnail job state are separate and disposable. Clearing a thumbnail or audio-preview cache must never remove raw recordings, generated clips, tags, notes, or collections.

## Scanning

Each recording fingerprint contains a normalized canonical path, file size, and nanosecond modification time. A matching probe-schema version is also required before FFprobe data is reused. A routine scan:

1. coalesces if another logical scan is already active;
2. walks configured sources without following links or entering configured output roots;
3. defers empty or recently modified recordings;
4. re-probes only a missing or changed fingerprint;
5. preserves every record under a disconnected source or an incompletely traversed source;
6. queues missing thumbnails and returns without waiting for FFmpeg image work.

Watcher events are not the durable source of truth. The frontend performs debounced focus and periodic reconciliation so missed filesystem changes are corrected by SQLite-backed scans.

## Thumbnail Recovery

Thumbnail work is persisted in `thumbnail_jobs`. Only one worker is active, duplicate media IDs are collapsed, failed work uses bounded attempts and backoff, and a new source fingerprint resets an exhausted job. A process interrupted while a job is marked `running` returns that job to `queued` during the next database open.

Retry deadlines use an interruptible timed wake-up, so delayed work does not depend on another scan or restart. The Settings repair action explicitly resets the bounded attempt budget for permanently failed thumbnails. Missing-media jobs are removed instead of remaining stuck in `running`.

Use the Settings repair action, or run a normal library refresh, to enqueue missing or corrupt thumbnails. Partial files use `.reelspace-part.jpg` names inside the thumbnail cache and are never treated as completed thumbnails.

## Diagnostics

Installed-app diagnostics are JSON lines in the application data `logs` directory. Logs rotate at 5 MB with three backups. Events include internal media IDs, counts, durations, error codes, cache hits, source availability, and worker outcomes; routine events do not include full personal media paths.

Runner transcripts are under `logs\runners`. The runner reports its revision, normalized mode, Node, pnpm, Rust, Cargo, Tauri, MSVC target, bundled FFmpeg versions, command failures, exit code, and log path.

Rust panics are written as structured reports under the application `recovery` directory. An active-session marker is removed on graceful exit; if it remains after an interruption, the next launch records an unclean-shutdown recovery event and resumes persistent thumbnail work.

Reelspace enforces a single desktop instance and focuses the existing window when launched again. Recovery reports are capped at 20 files. Asset-protocol access begins empty and is expanded only to configured source/output folders and Reelspace-managed media cache directories.

If SQLite cannot be opened or fails its integrity check, Reelspace renames the database and its WAL/SHM sidecars with a timestamped `.corrupt-*` suffix before creating a clean rebuildable index. The quarantined database is never overwritten. A database created by a newer Reelspace schema is preserved and rejected instead of being reset.

Only confirmed database corruption triggers quarantine. Locking, temporary I/O errors, permission failures, disk-full conditions, and migration failures leave the original database in place for retry or manual recovery. Changed scan records are committed in one atomic batch transaction.

## Large-Library Benchmark

Run the real scanner without launching the UI:

```powershell
$env:REELSPACE_BENCHMARK_REPORT='D:\path\to\report.json'
cargo run --release --example library_benchmark -- 'D:\path\to\recordings'
```

The harness treats media roots as read-only. Its SQLite database and thumbnail directory are created under the system temporary directory, and it refuses to put scratch state or the report inside a source root. It performs one cold FFprobe pass and five cached passes, reporting failures by stable error code.

## Safe Recovery

- Preserve the database before manual recovery.
- Do not delete the `clips` directory when clearing caches.
- Thumbnail and audio-preview directories may be removed while Reelspace is closed; they are regenerated.
- If a source drive is disconnected, reconnect it and refresh. Do not remove and re-add the source solely to recover its records.
- If a migration fails, keep the original database and application log. Reelspace does not commit a partial migration.
- To roll back a code change, prefer `git revert <commit>` so history remains recoverable.
