//! Supervisor: owns the gpu-screen-recorder process and the recording state machine.
use super::clip::gsr_cli;
use super::config::{chunk_bytes, Config, Mode, GB};
use super::control::{Control, Event};
use super::files::{self, ActiveRecording};
use super::games::{self, Session, SessionEvent, Tracker};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

const CRIT_GAP: Duration = Duration::from_secs(600);

/// External tools / locations; overridable so tests never touch the live system.
#[derive(Clone)]
pub struct Env {
    pub proc_root: PathBuf,
    pub pactl: String,
    pub systemctl: String,
    pub active_state: PathBuf,
    pub conflicting_units: Vec<String>,
    pub tick: Duration,
    pub guard_interval: Duration,
    pub detect_interval: Duration,
}

impl Env {
    pub fn system() -> Self {
        Self {
            proc_root: "/proc".into(),
            pactl: "pactl".into(),
            systemctl: "systemctl".into(),
            active_state: super::config::state_dir().join("active-recording.json"),
            conflicting_units: vec!["sol-rolling-recorder".into()],
            tick: Duration::from_secs(1),
            guard_interval: Duration::from_secs(60),
            detect_interval: Duration::from_secs(2),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum RecKind {
    Rolling,
    Session(String, Option<u32>),
}

struct Rec {
    kind: RecKind,
    started: Instant,
}

enum Blocked {
    Mount,
    Audio(String),
    Error(String),
}

pub struct Supervisor {
    ctl: Arc<Control>,
    env: Env,
    child: Option<Child>,
    child_started: Instant,
    rec: Option<Rec>,
    tracker: Tracker,
    backoff: Duration,
    next_attempt: Instant,
    last_guard: Instant,
    last_house: Instant,
    last_detect: Instant,
    disk_blocked: bool,
}

fn args_for(cfg: &Config, sock: &std::path::Path) -> Vec<String> {
    let mut a: Vec<String> = [
        "-w",
        &cfg.capture,
        "-f",
        &cfg.fps.to_string(),
        "-k",
        &cfg.codec,
        "-bm",
        "cbr",
        "-q",
        &cfg.bitrate_kbps.to_string(),
        "-c",
        "mkv",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for t in &cfg.audio_tracks {
        a.extend(["-a".into(), t.clone()]);
    }
    a.extend(["-ac", "aac", "-ab", "192", "-v", "no", "-r"].map(String::from));
    a.push(cfg.clip_sec.to_string());
    a.extend(["-replay-storage", "ram", "-o"].map(String::from));
    a.push(cfg.rec_dir.join(".replay").display().to_string());
    a.push("-ro".into());
    a.push(cfg.rec_dir.display().to_string());
    a.push("-ipc".into());
    a.push(sock.display().to_string());
    a
}

impl Supervisor {
    pub fn new(ctl: Arc<Control>, env: Env) -> Self {
        let past = Instant::now() - Duration::from_secs(3600);
        Self {
            ctl,
            env,
            child: None,
            child_started: past,
            rec: None,
            tracker: Tracker::new(3),
            backoff: Duration::from_secs(10),
            next_attempt: past,
            last_guard: past,
            last_house: past,
            last_detect: past,
            disk_blocked: false,
        }
    }

    pub fn run(&mut self) {
        while !self.ctl.shutdown.load(SeqCst) {
            self.tick();
            std::thread::sleep(self.env.tick);
        }
        self.shutdown_gsr(Duration::from_secs(35));
        self.ctl.set_state("stopped", "");
    }

    fn cfg(&self) -> Config {
        self.ctl.cfg()
    }

    fn tick(&mut self) {
        if self.ctl.reload.swap(false, SeqCst) {
            self.reload();
        }
        let cfg = self.cfg();
        if self.last_detect.elapsed() >= self.env.detect_interval {
            self.last_detect = Instant::now();
            self.detect(&cfg);
        }
        if self.ctl.paused.load(SeqCst) {
            if self.child.is_some() {
                self.shutdown_gsr(Duration::from_secs(35));
            }
            self.ctl.set_state("paused", "");
            return;
        }
        if let Some(c) = &mut self.child {
            if let Ok(Some(status)) = c.try_wait() {
                self.child = None;
                self.rec = None; // leftover Video_* is adopted by crash recovery on next start
                if self.child_started.elapsed() > Duration::from_secs(120) {
                    self.backoff = Duration::from_secs(10);
                }
                let msg = format!(
                    "gpu-screen-recorder exited ({status}); restarting in {}s",
                    self.backoff.as_secs()
                );
                self.next_attempt = Instant::now() + self.backoff;
                self.backoff = (self.backoff * 2).min(Duration::from_secs(60));
                self.ctl.set_state("error", &msg);
                self.ctl.critical("Recorder not running", &msg, CRIT_GAP);
                return;
            }
        }
        if self.child.is_none() {
            if Instant::now() >= self.next_attempt {
                self.try_start(&cfg);
            }
            return;
        }
        if self.last_guard.elapsed() >= self.env.guard_interval {
            self.last_guard = Instant::now();
            if let Some(Blocked::Error(e)) = self.guards(&cfg, true) {
                self.shutdown_gsr(Duration::from_secs(35));
                self.ctl.set_state("error", &e);
                self.next_attempt = Instant::now() + Duration::from_secs(60);
                return;
            }
        }
        if self.last_house.elapsed() >= self.env.guard_interval {
            self.last_house = Instant::now();
            self.housekeeping(&cfg);
        }
        self.reconcile(&cfg);
        self.refresh_status(&cfg);
    }

    fn reload(&mut self) {
        match Config::load(&self.ctl.cfg_path) {
            Ok(new) => {
                let old = self.cfg();
                let restart =
                    old.capture_params() != new.capture_params() || old.gsr_path != new.gsr_path;
                self.ctl.set_cfg(new);
                if restart && self.child.is_some() {
                    self.shutdown_gsr(Duration::from_secs(35));
                    self.next_attempt = Instant::now();
                }
                self.last_house = Instant::now() - Duration::from_secs(3600);
                self.ctl.sink.emit(Event::ConfigChanged);
            }
            Err(e) => {
                // keep last good config
                let mut s = self.ctl.status.lock().unwrap();
                s.last_error = format!("config rejected: {e}");
                drop(s);
                self.ctl
                    .set_state("error", &format!("config rejected: {e}"));
            }
        }
    }

    fn detect(&mut self, cfg: &Config) {
        let procs = games::scan_procs(&self.env.proc_root);
        let focus = self.ctl.focused_app_id.lock().unwrap().clone();
        let det = games::detect(
            &procs,
            &cfg.session,
            focus.as_deref(),
            &games::steam_app_name,
        );
        *self.ctl.current_game.lock().unwrap() = det.first().map(|d| (d.name.clone(), d.appid));
        for ev in self.tracker.update(&det, Instant::now()) {
            match ev {
                SessionEvent::Started(d) => {
                    self.ctl.status.lock().unwrap().session_game = d.name.clone();
                    self.ctl.sink.emit(Event::SessionStarted {
                        game: d.name,
                        path: cfg.rec_dir.display().to_string(),
                    });
                }
                SessionEvent::GameAdded(_) => {}
                SessionEvent::Ended(s) => {
                    self.ctl.status.lock().unwrap().session_game.clear();
                    if matches!(
                        self.rec.as_ref().map(|r| &r.kind),
                        Some(RecKind::Session(..))
                    ) {
                        self.finish_recording(cfg, Some(s));
                    }
                }
            }
        }
    }

    /// Preconditions for running gsr at all. `running`: our own gsr is already up (skip mount/audio waits).
    fn guards(&self, cfg: &Config, running: bool) -> Option<Blocked> {
        let sock = self.ctl.sock.display().to_string();
        for p in games::scan_procs(&self.env.proc_root) {
            if games::is_duplicate_capture(&p.1, &sock)
                && Some(p.0) != self.child.as_ref().map(Child::id)
            {
                return Some(Blocked::Error(format!("another gpu-screen-recorder replay/recording is running (pid {}); refusing to double-capture", p.0)));
            }
        }
        for u in &self.env.conflicting_units {
            let active = Command::new(&self.env.systemctl)
                .args(["--user", "is-active", "--quiet", u])
                .status()
                .is_ok_and(|s| s.success());
            if active {
                return Some(Blocked::Error(format!(
                    "{u}.service is active; stop it before using Reelspace as the recorder"
                )));
            }
        }
        if running {
            return None;
        }
        if cfg.require_mountpoint && !files::is_mountpoint(&cfg.rec_dir) {
            return Some(Blocked::Mount);
        }
        if let Some(missing) = missing_audio(&self.env.pactl, &cfg.audio_tracks) {
            return Some(Blocked::Audio(missing));
        }
        None
    }

    fn try_start(&mut self, cfg: &Config) {
        match self.guards(cfg, false) {
            Some(Blocked::Error(e)) => {
                self.ctl.set_state("error", &e);
                self.ctl.critical("Recorder not started", &e, CRIT_GAP);
                self.next_attempt = Instant::now() + Duration::from_secs(60);
                return;
            }
            Some(Blocked::Mount) => {
                self.ctl.set_state(
                    "waiting_mount",
                    &format!("{} is not a mountpoint", cfg.rec_dir.display()),
                );
                self.next_attempt = Instant::now() + Duration::from_secs(60);
                return;
            }
            Some(Blocked::Audio(a)) => {
                self.ctl
                    .set_state("waiting_audio", &format!("audio source '{a}' not found"));
                self.next_attempt = Instant::now() + Duration::from_secs(2);
                return;
            }
            None => {}
        }
        if let Err(e) = std::fs::create_dir_all(cfg.rec_dir.join(".replay"))
            .and_then(|_| std::fs::create_dir_all(cfg.clip_dir()))
        {
            self.ctl
                .set_state("error", &format!("cannot create directories: {e}"));
            self.next_attempt = Instant::now() + Duration::from_secs(60);
            return;
        }
        // adopt files from a previous crash (safe: no gsr of ours is running, duplicate guard passed)
        let active = files::read_active(&self.env.active_state);
        files::recover_leftovers(&cfg.rec_dir, active.as_ref());
        let _ = std::fs::remove_file(&self.env.active_state);
        self.housekeeping(cfg);
        let _ = std::fs::remove_file(&self.ctl.sock);
        let child = Command::new(&cfg.gsr_path)
            .args(args_for(cfg, &self.ctl.sock))
            .stdin(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                self.fail_start(format!("cannot launch {}: {e}", cfg.gsr_path));
                return;
            }
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if gsr_cli(&cfg.gsr_cli_path, &self.ctl.sock, &["status"]).is_ok() {
                break;
            }
            if child.try_wait().ok().flatten().is_some()
                || Instant::now() > deadline
                || self.ctl.shutdown.load(SeqCst)
            {
                let _ = child.kill();
                let _ = child.wait();
                self.fail_start("gpu-screen-recorder did not become ready".into());
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        self.child = Some(child);
        self.child_started = Instant::now();
        self.disk_blocked = false;
        self.ctl.set_state("recording", "");
    }

    fn fail_start(&mut self, msg: String) {
        self.ctl.set_state("error", &msg);
        self.ctl.critical("Recorder not running", &msg, CRIT_GAP);
        self.next_attempt = Instant::now() + self.backoff;
        self.backoff = (self.backoff * 2).min(Duration::from_secs(60));
    }

    fn desired(&self, cfg: &Config) -> Option<RecKind> {
        if self.disk_blocked {
            return None;
        }
        match cfg.mode {
            Mode::Rolling => Some(RecKind::Rolling),
            Mode::Manual => None,
            Mode::Session => self
                .tracker
                .current
                .as_ref()
                .map(|s| RecKind::Session(s.primary.name.clone(), s.primary.appid)),
        }
    }

    fn reconcile(&mut self, cfg: &Config) {
        let want = self.desired(cfg);
        if let Some(r) = &self.rec {
            let rotate = matches!(r.kind, RecKind::Rolling)
                && r.started.elapsed() >= Duration::from_secs(cfg.chunk_sec);
            // also covers mode switch, session end and disk block: the wanted kind no longer matches
            if rotate || want.as_ref() != Some(&r.kind) {
                self.finish_recording(cfg, None);
                if rotate {
                    self.housekeeping(cfg);
                }
            }
        }
        if self.rec.is_none() {
            if let Some(kind) = self.desired(cfg) {
                if self.ensure_space(cfg) {
                    self.begin_recording(cfg, kind);
                }
            }
        }
    }

    fn begin_recording(&mut self, cfg: &Config, kind: RecKind) {
        let (mode, game, appid) = match &kind {
            RecKind::Rolling => ("rolling", None, None),
            RecKind::Session(g, id) => ("session", Some(g.clone()), *id),
        };
        let a = ActiveRecording {
            mode: mode.into(),
            game,
            appid,
            started_at: chrono::Local::now().to_rfc3339(),
            dir: cfg.rec_dir.clone(),
        };
        if let Err(e) = files::write_active(&self.env.active_state, &a) {
            self.ctl
                .set_state("error", &format!("cannot write recovery state: {e}"));
            return;
        }
        match gsr_cli(
            &cfg.gsr_cli_path,
            &self.ctl.sock,
            &["start-replay-recording"],
        ) {
            Ok(_) => {
                self.rec = Some(Rec {
                    kind,
                    started: Instant::now(),
                });
                self.ctl.status.lock().unwrap().chunk_started = chrono::Utc::now().timestamp();
                self.ctl.set_state("recording", "");
            }
            Err(e) => {
                let _ = std::fs::remove_file(&self.env.active_state);
                self.ctl.set_state("error", &e);
            }
        }
    }

    /// Stop the continuous recording and give the file its final name. `ended` = the session that just ended (for metadata).
    fn finish_recording(&mut self, cfg: &Config, ended: Option<Session>) {
        let Some(rec) = self.rec.take() else { return };
        let out = gsr_cli(
            &cfg.gsr_cli_path,
            &self.ctl.sock,
            &["stop-replay-recording"],
        );
        let path = match out {
            Ok(p) => PathBuf::from(p.lines().last().unwrap_or("").trim()),
            Err(e) => {
                self.ctl.set_state("error", &e); // file (if any) is adopted by crash recovery
                return;
            }
        };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let dir = path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| cfg.rec_dir.clone());
        let Some(ts) = files::video_timestamp(&name) else {
            let _ = std::fs::remove_file(&self.env.active_state);
            return;
        };
        let new = match &rec.kind {
            RecKind::Rolling => format!("rec_{ts}.mkv"),
            RecKind::Session(g, _) => files::session_file_name(g, ts),
        };
        let dest = dir.join(&new);
        if !dest.exists() && std::fs::rename(&path, &dest).is_ok() {
            if let (RecKind::Session(g, id), Some(s)) =
                (&rec.kind, ended.or_else(|| self.tracker.current.clone()))
            {
                let dur = rec.started.elapsed();
                if !games::keep_session(dur.as_secs(), cfg.session.min_session_sec) {
                    let _ = std::fs::remove_file(&dest);
                } else {
                    let _ = xattr::set(&dest, "user.reelspace.game", g.as_bytes());
                    let _ =
                        xattr::set(&dest, "user.reelspace.games", s.games.join("\n").as_bytes());
                    if let Some(id) = id {
                        let _ = xattr::set(
                            &dest,
                            "user.reelspace.steam_appid",
                            id.to_string().as_bytes(),
                        );
                    }
                    self.ctl.sink.emit(Event::SessionEnded {
                        game: g.clone(),
                        path: dest.display().to_string(),
                        duration: dur.as_secs_f64(),
                    });
                }
            }
        }
        let _ = std::fs::remove_file(&self.env.active_state);
    }

    /// Free-space floor (all modes, not disable-able). Prunes rec_* then expired sessions; blocks recording if still short.
    fn ensure_space(&mut self, cfg: &Config) -> bool {
        let floor = cfg.min_free_gb * GB;
        let next = chunk_bytes(cfg.bitrate_kbps, cfg.chunk_sec);
        let free = files::free_bytes(&cfg.rec_dir).unwrap_or(0);
        let mut deficit = files::space_deficit(free, floor, next);
        if deficit > 0 {
            let victims = files::select_space_prune(
                &files::list_files(&cfg.rec_dir),
                deficit,
                &cfg.session.retention,
                SystemTime::now(),
            );
            files::delete_checked(&victims);
            deficit =
                files::space_deficit(files::free_bytes(&cfg.rec_dir).unwrap_or(0), floor, next);
        }
        if deficit > 0 {
            if !self.disk_blocked {
                self.disk_blocked = true;
                self.ctl.set_state("error", "disk floor reached");
                self.ctl.critical(
                    "Recording stopped: disk almost full",
                    "Free-space floor reached; replay clips still work.",
                    CRIT_GAP,
                );
            }
            false
        } else {
            if self.disk_blocked {
                self.disk_blocked = false;
                self.ctl.set_state("recording", "");
            }
            true
        }
    }

    fn housekeeping(&mut self, cfg: &Config) {
        let files = files::list_files(&cfg.rec_dir);
        let cap = cfg.cap_gb * GB;
        files::delete_checked(&files::select_cap_prune(
            &files,
            cap,
            chunk_bytes(cfg.bitrate_kbps, cfg.chunk_sec),
        ));
        files::delete_checked(&files::expired_sessions(
            &files,
            &cfg.session.retention,
            SystemTime::now(),
        ));
        if self.rec.is_some() || self.disk_blocked {
            // mid-chunk check: floor only (the next chunk is checked at rotation)
            let free = files::free_bytes(&cfg.rec_dir).unwrap_or(0);
            if free < cfg.min_free_gb * GB {
                self.ensure_space(cfg);
                if self.disk_blocked {
                    self.finish_recording(cfg, None);
                }
            } else if self.disk_blocked {
                self.ensure_space(cfg);
            }
        }
    }

    fn refresh_status(&mut self, cfg: &Config) {
        let files = files::list_files(&cfg.rec_dir);
        let mut s = self.ctl.status.lock().unwrap();
        s.archive_bytes = files::rec_total(&files);
        s.free_bytes = files::free_bytes(&cfg.rec_dir).unwrap_or(0);
        s.replay_seconds = if self.child.is_some() {
            cfg.clip_sec
        } else {
            0
        };
        if self.rec.is_none() {
            s.chunk_started = 0;
        }
    }

    /// SIGINT gsr, wait for it to finalize, then SIGKILL.
    pub fn shutdown_gsr(&mut self, grace: Duration) {
        let Some(mut c) = self.child.take() else {
            return;
        };
        unsafe { libc::kill(c.id() as i32, libc::SIGINT) };
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if c.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if c.try_wait().ok().flatten().is_none() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.rec = None;
        let cfg = self.cfg();
        // gsr finalized its Video_* on SIGINT: give it its proper name
        let active = files::read_active(&self.env.active_state);
        files::recover_leftovers(&cfg.rec_dir, active.as_ref());
        let _ = std::fs::remove_file(&self.env.active_state);
    }
}

/// First configured source (each `a|b` part) missing from PipeWire; `None` if all present or unknown/default sources.
pub fn missing_audio(pactl: &str, tracks: &[String]) -> Option<String> {
    let out = Command::new(pactl)
        .args(["list", "short", "sources"])
        .output()
        .ok()?;
    if !out.status.success() {
        return Some("pactl failed".into());
    }
    let listing = String::from_utf8_lossy(&out.stdout);
    let names: Vec<&str> = listing
        .lines()
        .filter_map(|l| l.split('\t').nth(1))
        .collect();
    tracks
        .iter()
        .flat_map(|t| t.split('|'))
        .filter(|p| !p.contains(':') && !p.starts_with("default_"))
        .find(|p| !names.contains(p))
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gsr_args() {
        let cfg = Config {
            audio_tracks: vec!["Rec_Mix".into(), "Mic|Voice".into()],
            rec_dir: "/r".into(),
            ..Config::default()
        };
        let a = args_for(&cfg, std::path::Path::new("/run/s.sock")).join(" ");
        assert!(a.starts_with("-w DP-1 -f 60 -k h264 -bm cbr -q 20000 -c mkv -a Rec_Mix -a Mic|Voice -ac aac -ab 192 -v no -r 300 -replay-storage ram -o /r/.replay -ro /r -ipc /run/s.sock"));
    }
}
