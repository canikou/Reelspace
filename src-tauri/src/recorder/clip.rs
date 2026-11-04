//! Save-clip pipeline: gsr save-replay -> ffmpeg remux (.part) -> xattrs -> rename -> delete raw.
use super::control::{Control, Event};
use super::files::now_ts;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

pub fn gsr_cli(bin: &str, sock: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(bin)
        .arg("-ipc")
        .arg(sock)
        .args(args)
        .output()
        .map_err(|e| format!("{bin}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!(
            "{bin} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn probe_duration(ffmpeg: &str, file: &Path) -> Option<f64> {
    let probe = ffmpeg
        .strip_suffix("ffmpeg")
        .map_or("ffprobe".to_string(), |dir| format!("{dir}ffprobe"));
    let out = Command::new(probe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(file)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

pub fn remux(ffmpeg: &str, raw: &Path, part: &Path) -> Result<(), String> {
    let out = Command::new(ffmpeg)
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(raw)
        .args([
            "-map",
            "0",
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
            "-y",
        ])
        .arg(part)
        .output()
        .map_err(|e| format!("{ffmpeg}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Runs the whole pipeline synchronously; emits ClipSaved / ClipFailed. Caller owns the in-flight flag.
pub fn run_save(ctl: &Control, seconds: u32) {
    let cfg = ctl.cfg();
    let fail = |reason: String, raw: Option<&Path>| {
        ctl.sink.emit(Event::ClipFailed {
            reason,
            raw_path: raw.map(|p| p.display().to_string()).unwrap_or_default(),
        });
    };
    let raw = match gsr_cli(
        &cfg.gsr_cli_path,
        &ctl.sock,
        &["save-replay", &seconds.to_string()],
    ) {
        Ok(p) if !p.is_empty() => PathBuf::from(p.lines().last().unwrap_or("")),
        Ok(_) => return fail("recorder returned no file".into(), None),
        Err(e) => {
            eprintln!("reelspace-recorder: save-replay failed: {e}");
            ctl.status.lock().unwrap().last_error = e;
            return fail("Recorder is not running, so no clip was saved".into(), None);
        }
    };
    let clip_dir = cfg.clip_dir();
    let ts = now_ts();
    let part = clip_dir.join(format!(".clip_{ts}.mp4.part"));
    let dest = clip_dir.join(format!("clip_{ts}.mp4"));
    let game = ctl.current_game.lock().unwrap().clone();
    let result = (|| {
        std::fs::create_dir_all(&clip_dir).map_err(|e| e.to_string())?;
        remux(&cfg.ffmpeg_path, &raw, &part)?;
        if let Some((name, appid)) = &game {
            let _ = xattr::set(&part, "user.reelspace.game", name.as_bytes());
            if let Some(id) = appid {
                let _ = xattr::set(
                    &part,
                    "user.reelspace.steam_appid",
                    id.to_string().as_bytes(),
                );
            }
        }
        if dest.exists() {
            return Err(format!("{} already exists", dest.display()));
        }
        std::fs::rename(&part, &dest)
            .map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
        Ok::<(), String>(())
    })();
    match result {
        Ok(()) => {
            let _ = std::fs::remove_file(&raw);
            let duration = probe_duration(&cfg.ffmpeg_path, &dest).unwrap_or(seconds as f64);
            ctl.sink.emit(Event::ClipSaved {
                path: dest.display().to_string(),
                duration,
                game: game.map(|g| g.0).unwrap_or_default(),
            });
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            fail(e, Some(&raw));
        }
    }
}

/// D-Bus `SaveClip`: immediate return; "" when a save is already running.
pub fn save_clip(ctl: &Arc<Control>, seconds: u32) -> String {
    if ctl.paused.load(std::sync::atomic::Ordering::SeqCst) {
        ctl.sink.emit(Event::ClipFailed {
            reason: "Recorder is paused — resume to save clips".into(),
            raw_path: String::new(),
        });
        return String::new();
    }
    if !ctl.begin_save() {
        return String::new();
    }
    let seconds = if seconds == 0 {
        ctl.cfg().clip_sec
    } else {
        seconds
    };
    let id = now_ts();
    ctl.sink.emit(Event::ClipSaving(seconds));
    let c = ctl.clone();
    std::thread::spawn(move || {
        run_save(&c, seconds);
        c.end_save();
    });
    id
}
