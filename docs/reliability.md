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

Use the Settings repair action, or run a normal library refresh, to enqueue missing or corrupt thumbnails. Partial files use `.reelspace-part.jpg` names inside the thumbnail cache and are never treated as completed thumbnails.

## Diagnostics

Installed-app diagnostics are JSON lines in the application data `logs` directory. Logs rotate at 5 MB with three backups. Events include internal media IDs, counts, durations, error codes, cache hits, source availability, and worker outcomes; routine events do not include full personal media paths.

Runner transcripts are under `logs\runners`. The runner reports its revision, normalized mode, Node, pnpm, Rust, Cargo, Tauri, MSVC target, bundled FFmpeg versions, command failures, exit code, and log path.

## Safe Recovery

- Preserve the database before manual recovery.
- Do not delete the `clips` directory when clearing caches.
- Thumbnail and audio-preview directories may be removed while Reelspace is closed; they are regenerated.
- If a source drive is disconnected, reconnect it and refresh. Do not remove and re-add the source solely to recover its records.
- If a migration fails, keep the original database and application log. Reelspace does not commit a partial migration.
- To roll back a code change, prefer `git revert <commit>` so history remains recoverable.
