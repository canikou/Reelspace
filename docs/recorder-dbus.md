# Reelspace recorder D-Bus interface

Session bus · name `org.reelspace.Recorder` · path `/org/reelspace/Recorder` · interface `org.reelspace.Recorder1`.

## Methods
| Method | Signature | Notes |
|---|---|---|
| `SaveClip` | `(u seconds) -> s` | Returns at once with a job id; `""` if a save is already running (double-press ignored). `0` = configured `clip_sec`. |
| `Status` | `() -> a{sv}` | Keys: `state` (`recording\|paused\|waiting_mount\|waiting_audio\|error\|stopped`), `detail`, `mode`, `replay_seconds` (u), `archive_bytes` (t), `cap_bytes` (t), `free_bytes` (t), `min_free_bytes` (t), `archive_hours_estimate` (d), `current_chunk_started` (x, unix s, 0 = none), `session_game`, `last_error`. |
| `Pause` / `Resume` | `()` | Pause stops gpu-screen-recorder entirely (no replay buffer either). |
| `ReloadConfig` | `()` | Re-reads `~/.config/reelspace/recorder.json`. Invalid config is rejected (last good kept, `last_error` set). gsr restarts only if capture/fps/codec/bitrate/audio/clip_sec/rec_dir changed. |
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
