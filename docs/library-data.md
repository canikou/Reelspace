# Portable Library Data

Reelspace Settings can export and import a versioned `.json` archive for user-created library organization. The archive contains media references, tags, notes, game classifications and aliases, collection names and membership, storage preferences, verified private archive references, and clip provenance/trim metadata. It never contains video, audio, thumbnails, previews, FFmpeg binaries, OAuth credentials, application settings, or diagnostics.

## Matching order

1. Exact original path, with kind, size, and duration verification.
2. The exported path relative to any currently configured source, working, clip, or managed clip root.
3. A unique kind, filename, size, and duration match in the current index.
4. Restore a database record when exactly one original/remapped path exists locally, has the exported size, and remains inside a currently configured source/output root.
5. If no local copy exists, restore a remote-only record only when the archive contains a validated, verified, private YouTube reference. The placeholder path stays inside Reelspace application data and no video is downloaded.

Ambiguous and changed local files remain unmatched. A missing file without a verified remote reference is never restored.

## Merge behavior

- Tags are merged case-insensitively. Existing tags take priority if the 100-tag or 4 KB application limit is reached.
- An imported note fills an empty local note. A different non-empty local note is preserved and reported as a conflict.
- Collections are matched case-insensitively by name, created when absent, and memberships are added without removing local memberships.
- Game classifications, aliases, Keep Local choices, collection defaults, and verified private archive references are restored for safely matched records.
- Media, organization, preference, and archive-reference changes commit in one SQLite transaction. Any database failure rolls back the entire import.

The current archive format identifier is `reelspace-library-data`, version `2`. Reelspace also accepts version 1 archives. Files are limited to 64 MB, 250,000 media references, 50,000 collections, and 2,000,000 collection memberships. Unknown fields and unsupported versions are rejected so a typo or future incompatible archive cannot be partially interpreted.
