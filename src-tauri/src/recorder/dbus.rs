//! D-Bus service (org.reelspace.Recorder1), signal emission, desktop notifications and sounds.
use super::config::{archive_hours, GB};
use super::control::{Control, Event, Flags, Sink};
use super::{clip, files};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use zbus::blocking::Connection;
use zbus::zvariant::{OwnedValue, Value};

pub const NAME: &str = "org.reelspace.Recorder";
pub const PATH: &str = "/org/reelspace/Recorder";
pub const IFACE: &str = "org.reelspace.Recorder1";

pub struct Service(pub Arc<Control>);

fn ov<'a, T: Into<Value<'a>>>(v: T) -> OwnedValue {
    OwnedValue::try_from(v.into()).expect("no fds")
}

pub fn status_map(ctl: &Control) -> HashMap<String, OwnedValue> {
    let cfg = ctl.cfg();
    let s = ctl.status.lock().unwrap().clone();
    let state = if ctl.paused.load(std::sync::atomic::Ordering::SeqCst) {
        "paused".to_string()
    } else {
        s.state
    };
    let mut m = HashMap::new();
    m.insert("state".into(), ov(state));
    m.insert("detail".into(), ov(s.detail));
    m.insert("mode".into(), ov(cfg.mode.as_str().to_string()));
    m.insert("replay_seconds".into(), ov(s.replay_seconds));
    m.insert("archive_bytes".into(), ov(s.archive_bytes));
    m.insert("cap_bytes".into(), ov(cfg.cap_gb * GB));
    m.insert("free_bytes".into(), ov(s.free_bytes));
    m.insert("min_free_bytes".into(), ov(cfg.min_free_gb * GB));
    m.insert(
        "archive_hours_estimate".into(),
        ov(archive_hours(cfg.cap_gb, cfg.bitrate_kbps)),
    );
    m.insert("current_chunk_started".into(), ov(s.chunk_started));
    m.insert("session_game".into(), ov(s.session_game));
    m.insert("last_error".into(), ov(s.last_error));
    m
}

#[zbus::interface(name = "org.reelspace.Recorder1")]
impl Service {
    fn save_clip(&self, seconds: u32) -> String {
        clip::save_clip(&self.0, seconds)
    }
    fn status(&self) -> HashMap<String, OwnedValue> {
        status_map(&self.0)
    }
    fn pause(&self) {
        self.0
            .paused
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    fn resume(&self) {
        self.0
            .paused
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
    fn reload_config(&self) {
        self.0
            .reload
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    /// Deletes every `rec_*.mkv` in rec_dir (nothing else).
    fn delete_rolling_archive(&self) -> u64 {
        files::delete_rolling_archive(&self.0.cfg().rec_dir)
    }

    #[zbus(signal)]
    async fn state_changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        state: &str,
        detail: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn config_changed(emitter: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn clip_saving(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        seconds: u32,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn clip_saved(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        path: &str,
        duration_s: f64,
        game: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn clip_failed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        reason: &str,
        raw_path: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn session_started(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        game: &str,
        path: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn session_ended(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        game: &str,
        path: &str,
        duration_s: f64,
    ) -> zbus::Result<()>;
}

// ---------- sounds ----------

pub fn play_sound(id: &str) {
    let id = id.to_string();
    std::thread::spawn(move || {
        let ok = Command::new("canberra-gtk-play")
            .args(["-i", &id, "-d", "reelspace"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            let _ = Command::new("pw-play")
                .arg(format!("/usr/share/sounds/freedesktop/stereo/{id}.oga"))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    });
}

// ---------- notifications ----------

pub struct Notifier {
    conn: Connection,
    paths: Mutex<HashMap<u32, PathBuf>>,
}

const NOTIF_DEST: &str = "org.freedesktop.Notifications";
const NOTIF_PATH: &str = "/org/freedesktop/Notifications";

impl Notifier {
    pub fn new(conn: Connection) -> Arc<Self> {
        let n = Arc::new(Self {
            conn: conn.clone(),
            paths: Mutex::new(HashMap::new()),
        });
        let n2 = n.clone();
        std::thread::spawn(move || n2.listen());
        n
    }

    fn listen(&self) {
        let Ok(rule) = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(NOTIF_DEST)
            .and_then(|b| b.member("ActionInvoked"))
            .map(|b| b.build())
        else {
            return;
        };
        let Ok(iter) = zbus::blocking::MessageIterator::for_match_rule(rule, &self.conn, None)
        else {
            return;
        };
        for msg in iter.flatten() {
            let Ok((id, key)) = msg.body().deserialize::<(u32, String)>() else {
                continue;
            };
            let Some(path) = self.paths.lock().unwrap().get(&id).cloned() else {
                continue;
            };
            match key.as_str() {
                "open" => {
                    let _ = Command::new("reelspace")
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn();
                }
                "show" => {
                    let uri = format!("file://{}", path.display());
                    let _ = self.conn.call_method(
                        Some("org.freedesktop.FileManager1"),
                        "/org/freedesktop/FileManager1",
                        Some("org.freedesktop.FileManager1"),
                        "ShowItems",
                        &(vec![uri], "reelspace"),
                    );
                }
                _ => {}
            }
        }
    }

    pub fn notify(
        &self,
        summary: &str,
        body: &str,
        icon: &str,
        critical: bool,
        path: Option<&str>,
    ) {
        let mut hints: HashMap<&str, Value> = HashMap::new();
        hints.insert("desktop-entry", Value::from("Reelspace"));
        if critical {
            hints.insert("urgency", Value::from(2u8));
        }
        let actions: Vec<&str> = if path.is_some() {
            vec!["open", "Open", "show", "Show in folder"]
        } else {
            vec![]
        };
        let reply = self.conn.call_method(
            Some(NOTIF_DEST),
            NOTIF_PATH,
            Some(NOTIF_DEST),
            "Notify",
            &(
                "Reelspace",
                0u32,
                icon,
                summary,
                body,
                actions,
                hints,
                -1i32,
            ),
        );
        if let (Ok(r), Some(p)) = (reply, path) {
            if let Ok(id) = r.body().deserialize::<u32>() {
                self.paths.lock().unwrap().insert(id, PathBuf::from(p));
            }
        }
    }
}

/// Maps core events to D-Bus signals, sounds and notifications.
pub struct Outputs {
    pub conn: Connection,
    pub notifier: Arc<Notifier>,
    pub flags: Arc<Flags>,
}

fn mss(d: f64) -> String {
    let t = d.round() as u64;
    format!("{}:{:02}", t / 60, t % 60)
}

impl Outputs {
    fn signal<B: serde::Serialize + zbus::zvariant::DynamicType>(&self, name: &str, body: &B) {
        let _ = self.conn.emit_signal(None::<&str>, PATH, IFACE, name, body);
    }
}

impl Sink for Outputs {
    fn emit(&self, e: Event) {
        use std::sync::atomic::Ordering::SeqCst;
        let sound = |id: &str| {
            if self.flags.sounds.load(SeqCst) {
                play_sound(id)
            }
        };
        let notify = self.flags.notifications.load(SeqCst);
        match e {
            Event::StateChanged { state, detail } => self.signal("StateChanged", &(state, detail)),
            Event::ConfigChanged => self.signal("ConfigChanged", &()),
            Event::ClipSaving(s) => {
                sound("camera-shutter");
                self.signal("ClipSaving", &(s,));
            }
            Event::ClipSaved {
                path,
                duration,
                game,
            } => {
                sound("complete");
                if notify {
                    let name = std::path::Path::new(&path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&path)
                        .to_string();
                    self.notifier.notify(
                        &format!("Clip saved ({})", mss(duration)),
                        &name,
                        "media-record",
                        false,
                        Some(&path),
                    );
                }
                self.signal("ClipSaved", &(path, duration, game));
            }
            Event::ClipFailed { reason, raw_path } => {
                sound("dialog-warning");
                if notify {
                    let body = if raw_path.is_empty() {
                        reason.clone()
                    } else {
                        format!("Raw clip kept: {raw_path}")
                    };
                    let summary = if raw_path.is_empty() {
                        "Clip NOT saved"
                    } else {
                        "Clip failed to convert"
                    };
                    self.notifier
                        .notify(summary, &body, "dialog-warning", true, None);
                }
                self.signal("ClipFailed", &(reason, raw_path));
            }
            Event::SessionStarted { game, path } => self.signal("SessionStarted", &(game, path)),
            Event::SessionEnded {
                game,
                path,
                duration,
            } => self.signal("SessionEnded", &(game, path, duration)),
            Event::Critical { summary, body } => {
                if notify {
                    self.notifier
                        .notify(&summary, &body, "dialog-warning", true, None);
                }
            }
        }
    }
}

/// Track the GNOME extension's focused window, if/when it exists.
pub fn watch_focus(conn: Connection, ctl: Arc<Control>) {
    std::thread::spawn(move || {
        let Ok(rule) = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.reelspace.Shell")
            .and_then(|b| b.member("FocusedWindowChanged"))
            .map(|b| b.build())
        else {
            return;
        };
        let Ok(iter) = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, None) else {
            return;
        };
        for msg in iter.flatten() {
            if let Ok((_title, app_id)) = msg.body().deserialize::<(String, String)>() {
                *ctl.focused_app_id.lock().unwrap() = Some(app_id).filter(|a| !a.is_empty());
            }
        }
    });
}

/// Service connection first (so `Outputs` can emit), then the control block. Returns the connection that owns the name.
pub fn start(
    cfg: super::config::Config,
    cfg_path: PathBuf,
    sock: PathBuf,
) -> zbus::Result<(Arc<Control>, Connection)> {
    let conn = Connection::session()?;
    let flags = Arc::new(Flags::default());
    let sink = Arc::new(Outputs {
        conn: conn.clone(),
        notifier: Notifier::new(conn.clone()),
        flags: flags.clone(),
    });
    let ctl = Control::new(cfg, cfg_path, sock, sink, flags);
    conn.object_server().at(PATH, Service(ctl.clone()))?;
    conn.request_name(NAME)?;
    watch_focus(conn.clone(), ctl.clone());
    Ok((ctl, conn))
}
