# Reelspace recorder D-Bus interface

Session bus · name `org.reelspace.Recorder` · path `/org/reelspace/Recorder` · interface `org.reelspace.Recorder1`.

## Methods
| Method | Signature | Notes |
|---|---|---|
| `SaveClip` | `(u seconds) -> s` | Returns at once with a job id; `""` if a save is already running (double-press ignored). `0` = configured `clip_sec`. While paused it returns `""` and emits `ClipFailed("Recorder is paused — resume to save clips", "")`. |
| `Status` | `() -> a{sv}` | Keys: `state` (`recording\|replay\|paused\|waiting_mount\|waiting_audio\|waiting_display\|error\|stopped`; `replay` = only the replay buffer runs, e.g. session mode with no game; `waiting_display` = the monitor/capture target is missing, retried every few seconds; `detail` carries gsr's stderr reason), `detail`, `mode`, `replay_seconds` (u), `archive_bytes` (t), `cap_bytes` (t), `free_bytes` (t), `min_free_bytes` (t), `archive_hours_estimate` (d), `current_chunk_started` (x, unix s, 0 = none), `session_game`, `last_error`. |
| `Pause` / `Resume` | `()` | Pause stops gpu-screen-recorder entirely (no replay buffer either). |
| `ReloadConfig` | `()` | Re-reads `~/.config/reelspace/recorder.json`. Invalid config is rejected (last good kept, `last_error` set). gsr restarts only if capture/fps/codec/bitrate/audio/clip_sec/rec_dir/replay_storage changed. |
| `DeleteRollingArchive` | `() -> t` | Deletes every `rec_YYYY-MM-DD_HH-MM-SS.mkv` in `rec_dir` (never anything else); returns bytes freed. |

## Signals
`StateChanged(s state, s detail)`, `ConfigChanged()`, `ClipSaving(u seconds)`, `ClipSaved(s path, d duration_s, s game)`, `ClipFailed(s reason, s raw_path)`, `SessionStarted(s game, s path)` (path = recording directory; the final file is in `SessionEnded`), `SessionEnded(s game, s path, d duration_s)` (not sent for sessions discarded as too short).

## Consumed
`org.reelspace.Shell.FocusedWindowChanged(s title, s app_id)` (GNOME extension, optional): app_ids listed in `session.focus_app_ids` count as a game.

## Metadata
Clips and session files carry xattrs `user.reelspace.game`, `user.reelspace.steam_appid`; sessions also `user.reelspace.games` (newline separated).

## CLI
`reelspace-recorder [run] | save-clip [--seconds N] | status | pause | resume | reload` (exit 1 if the daemon is absent).

## Running
`systemctl --user enable --now reelspace-recorder` (deb/rpm install the unit; AppImage cannot ship a system unit). Stop any other gpu-screen-recorder replay service first — the daemon refuses to double-capture. Config: `~/.config/reelspace/recorder.json` (see `Config` in `src-tauri/src/recorder/config.rs`).

Storage budget: `max_storage_gb` (optional, >= 1; default unset = no limit other than the `min_free_gb` floor). When set, it is the single budget for all full recordings (`rec_*.mkv` and `session_*.mkv`, found recursively under `rec_dir`, excluding the clips folder and `.replay`); the oldest by filename timestamp are deleted first, both for the budget and for the disk floor, and `cap_gb` / `session.retention` are ignored. The `Status` keys `cap_bytes` and `archive_hours_estimate` then use the budget. When unset, `cap_gb` and `session.retention` behave as before. Clips are never deleted automatically.
