# Reelspace GNOME Shell extension (Linux, optional)

GNOME doesn't show normal notification banners while a fullscreen game is focused, and apps on Wayland can't draw over other windows. This extension fills both gaps for `reelspace-recorder`.

- **OSD:** shows Shell OSD toasts (the volume-popup style, drawn above fullscreen games) when `org.reelspace.Recorder1` emits:
  - Saving clip…
  - Clip saved m:ss · game
  - Recording game
  - Session saved h:mm:ss · game
  - Recorder error: detail (shown once per state change)
  - Recorder stopped (also shown if the daemon disappears from the bus)

  Persistent alerts stay with the daemon's critical notifications, which GNOME does show over fullscreen.
- **Focused window:** owns `org.reelspace.Shell` at `/org/reelspace/Shell` and provides:
  - `FocusedWindow() -> (s title, s app_id)`
  - signal `FocusedWindowChanged(s title, s app_id)`, debounced 250 ms and emitted only on change.

  `app_id` is the Flatpak/Snap id, otherwise the `.desktop` id without its suffix, otherwise WM_CLASS. Proton games give `steam_app_<AppId>`.

Supports GNOME Shell 49–51.

## Install (per user)
```sh
cp -r packaging/gnome-extension/reelspace@reelspace.app ~/.local/share/gnome-shell/extensions/
# Wayland: log out and back in so the shell sees the new directory, then:
gnome-extensions enable reelspace@reelspace.app
```

## Test
`gjs -m packaging/gnome-extension/test/osd.test.js` runs the pure signal-to-OSD mapping tests.
