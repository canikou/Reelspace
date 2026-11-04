//! Recorder configuration: serde with defaults, strict validation, pure estimates.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Session,
    Manual,
    Rolling,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Session => "session",
            Mode::Manual => "manual",
            Mode::Rolling => "rolling",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Retention {
    KeepForever,
    Days { days: u32 },
    SizeCapGb { gb: u64 },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct SessionConfig {
    pub retention: Retention,
    pub min_session_sec: u64,
    pub deny_appids: Vec<u32>,
    /// Non-Steam process names (argv0 basename or comm) that count as a game.
    pub game_processes: Vec<String>,
    /// Focused-window app_ids (GNOME extension) that count as a game.
    pub focus_app_ids: Vec<String>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            retention: Retention::KeepForever,
            min_session_sec: 60,
            deny_appids: vec![431960, 228980],
            game_processes: vec![],
            focus_app_ids: vec![],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    pub mode: Mode,
    pub rec_dir: PathBuf,
    pub clip_dir: Option<PathBuf>,
    pub cap_gb: u64,
    pub chunk_sec: u64,
    pub clip_sec: u32,
    pub bitrate_kbps: u32,
    pub fps: u32,
    pub capture: String,
    pub codec: String,
    /// gsr replay buffer: "disk" (segments in rec_dir/.replay, saves RAM) or "ram".
    pub replay_storage: String,
    pub audio_tracks: Vec<String>,
    pub require_mountpoint: bool,
    pub min_free_gb: u64,
    pub session: SessionConfig,
    pub sounds: bool,
    pub notifications: bool,
    /// ffmpeg used for clip remuxing (later: the bundled one).
    pub ffmpeg_path: String,
    /// Engine binaries; overridable for tests / custom installs.
    pub gsr_path: String,
    pub gsr_cli_path: String,
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Rolling,
            rec_dir: home().join("Videos/Recordings"),
            clip_dir: None,
            cap_gb: 256,
            chunk_sec: 1800,
            clip_sec: 300,
            bitrate_kbps: 20000,
            fps: 60,
            capture: "DP-1".into(),
            codec: "h264".into(),
            replay_storage: "disk".into(),
            audio_tracks: vec!["Rec_Mix".into()],
            require_mountpoint: true,
            min_free_gb: 50,
            session: SessionConfig::default(),
            sounds: true,
            notifications: true,
            ffmpeg_path: "ffmpeg".into(),
            gsr_path: "gpu-screen-recorder".into(),
            gsr_cli_path: "gsr-cli".into(),
        }
    }
}

pub const GB: u64 = 1_000_000_000;
pub const MIN_FREE_FLOOR_GB: u64 = 5;

impl Config {
    pub fn clip_dir(&self) -> PathBuf {
        self.clip_dir
            .clone()
            .unwrap_or_else(|| self.rec_dir.join("Clips"))
    }

    pub fn validate(&self) -> Result<(), String> {
        let e = |m: &str| Err(m.to_string());
        if self.min_free_gb < MIN_FREE_FLOOR_GB {
            return Err(format!("min_free_gb must be at least {MIN_FREE_FLOOR_GB} (disk safety floor cannot be disabled)"));
        }
        if !self.rec_dir.is_absolute() {
            return e("rec_dir must be an absolute path");
        }
        if !self.clip_dir().is_absolute() {
            return e("clip_dir must be an absolute path");
        }
        if self.cap_gb == 0 {
            return e("cap_gb must be > 0");
        }
        if self.chunk_sec < 60 {
            return e("chunk_sec must be at least 60");
        }
        if !(5..=3600).contains(&self.clip_sec) {
            return e("clip_sec must be between 5 and 3600");
        }
        if !(500..=200_000).contains(&self.bitrate_kbps) {
            return e("bitrate_kbps must be between 500 and 200000");
        }
        if !(1..=360).contains(&self.fps) {
            return e("fps must be between 1 and 360");
        }
        if self.capture.trim().is_empty() {
            return e("capture must not be empty");
        }
        if ![
            "h264",
            "hevc",
            "av1",
            "vp8",
            "vp9",
            "hevc_hdr",
            "av1_hdr",
            "hevc_10bit",
            "av1_10bit",
        ]
        .contains(&self.codec.as_str())
        {
            return Err(format!("unsupported codec '{}'", self.codec));
        }
        if !["disk", "ram"].contains(&self.replay_storage.as_str()) {
            return Err(format!(
                "replay_storage must be 'disk' or 'ram', got '{}'",
                self.replay_storage
            ));
        }
        if self.audio_tracks.is_empty() || self.audio_tracks.iter().any(|t| t.trim().is_empty()) {
            return e("audio_tracks must contain at least one non-empty entry");
        }
        match self.session.retention {
            Retention::Days { days: 0 } | Retention::SizeCapGb { gb: 0 } => {
                return e("session retention value must be > 0")
            }
            _ => {}
        }
        Ok(())
    }

    /// Parameters that force a gsr restart when changed.
    pub fn capture_params(&self) -> (String, u32, String, u32, Vec<String>, u32, PathBuf) {
        (
            self.capture.clone(),
            self.fps,
            self.codec.clone(),
            self.bitrate_kbps,
            self.audio_tracks.clone(),
            self.clip_sec,
            self.rec_dir.clone(),
        )
    }

    pub fn load(path: &Path) -> Result<Config, String> {
        let cfg = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str::<Config>(&text)
                .map_err(|e| format!("{}: {e}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        cfg.validate()?;
        Ok(cfg)
    }
}

pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    base.join("reelspace/recorder.json")
}

pub fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state"));
    base.join("reelspace")
}

/// Hours of archive that `cap_gb` holds at `bitrate_kbps` (video only; audio adds ~1%).
pub fn archive_hours(cap_gb: u64, bitrate_kbps: u32) -> f64 {
    cap_gb as f64 * GB as f64 / gb_per_hour(bitrate_kbps) / GB as f64
}

pub fn gb_per_hour(bitrate_kbps: u32) -> f64 {
    bitrate_kbps as f64 * 125.0 * 3600.0 / GB as f64
}

pub fn chunk_bytes(bitrate_kbps: u32, chunk_sec: u64) -> u64 {
    bitrate_kbps as u64 * 125 * chunk_sec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let c = Config::default();
        assert!(c.validate().is_ok());
        assert_eq!(c.mode, Mode::Rolling);
        assert_eq!(c.audio_tracks, vec!["Rec_Mix"]);
        assert_eq!(c.clip_dir(), c.rec_dir.join("Clips"));
    }

    #[test]
    fn partial_json_fills_defaults() {
        let c: Config = serde_json::from_str(
            r#"{"mode":"session","session":{"retention":{"kind":"days","days":7}}}"#,
        )
        .unwrap();
        assert_eq!(c.mode, Mode::Session);
        assert_eq!(c.session.retention, Retention::Days { days: 7 });
        assert_eq!(c.session.min_session_sec, 60);
        assert_eq!(c.cap_gb, 256);
    }

    #[test]
    fn rejects_bad_values() {
        let mut c = Config {
            min_free_gb: 4,
            ..Config::default()
        };
        assert!(c.validate().unwrap_err().contains("min_free_gb"));
        c = Config {
            codec: "x".into(),
            ..Config::default()
        };
        assert!(c.validate().is_err());
        c = Config {
            replay_storage: "tmpfs".into(),
            ..Config::default()
        };
        assert!(c.validate().unwrap_err().contains("replay_storage"));
        c = Config {
            audio_tracks: vec![],
            ..Config::default()
        };
        assert!(c.validate().is_err());
        c = Config {
            rec_dir: "rel".into(),
            ..Config::default()
        };
        assert!(c.validate().is_err());
        assert!(serde_json::from_str::<Config>(r#"{"mode":"bogus"}"#).is_err());
    }

    #[test]
    fn estimates() {
        assert!((gb_per_hour(20000) - 9.0).abs() < 1e-9);
        assert!((archive_hours(256, 20000) - 28.44).abs() < 0.01);
        assert_eq!(chunk_bytes(20000, 1800), 4_500_000_000);
    }

    #[test]
    fn load_missing_is_default_and_invalid_errors() {
        let d = tempfile::tempdir().unwrap();
        assert!(Config::load(&d.path().join("none.json")).is_ok());
        let p = d.path().join("c.json");
        std::fs::write(&p, r#"{"min_free_gb":1}"#).unwrap();
        assert!(Config::load(&p).is_err());
    }
}
