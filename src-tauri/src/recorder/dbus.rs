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
        // Subscribe before returning so no signal can slip in ahead of the match rule.
        match Self::subscribe(&conn) {
            Ok(iter) => {
                let n2 = n.clone();
                std::thread::spawn(move || n2.listen(iter));
            }
            Err(e) => eprintln!("reelspace-recorder: notification actions disabled: {e}"),
        }
        n
    }

    fn subscribe(conn: &Connection) -> zbus::Result<zbus::blocking::MessageIterator> {
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(NOTIF_DEST)?
            .member("ActionInvoked")?
            .build();
        zbus::blocking::MessageIterator::for_match_rule(rule, conn, None)
    }

    fn listen(&self, iter: zbus::blocking::MessageIterator) {
        for msg in iter {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("reelspace-recorder: notification signal error: {e}");
                    continue;
                }
            };
            let (id, key) = match msg.body().deserialize::<(u32, String)>() {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("reelspace-recorder: bad ActionInvoked body: {e}");
                    continue;
                }
            };
            let Some(path) = self.paths.lock().unwrap().get(&id).cloned() else {
                continue; // someone else's notification
            };
            match key.as_str() {
                "open" => {
                    if let Err(e) = Command::new("reelspace")
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        eprintln!("reelspace-recorder: cannot launch reelspace: {e}");
                    }
                }
                "show" => {
                    let uri = url::Url::from_file_path(&path)
                        .map_or_else(|_| format!("file://{}", path.display()), String::from);
                    if let Err(e) = self.conn.call_method(
                        Some("org.freedesktop.FileManager1"),
                        "/org/freedesktop/FileManager1",
                        Some("org.freedesktop.FileManager1"),
                        "ShowItems",
                        &(vec![uri], "reelspace"),
                    ) {
                        eprintln!("reelspace-recorder: ShowItems failed: {e}");
                    }
                }
                other => eprintln!("reelspace-recorder: unknown notification action '{other}'"),
            }
        }
        eprintln!("reelspace-recorder: notification listener stopped");
    }

    /// Never blocks: callers include D-Bus method handlers, and a blocking call on
    /// `conn` from inside its own executor deadlocks the whole service.
    pub fn notify(
        self: &Arc<Self>,
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
        let (n, icon, summary, body) = (
            self.clone(),
            icon.to_string(),
            summary.to_string(),
            body.to_string(),
        );
        let path = path.map(str::to_string);
        std::thread::spawn(move || n.send(&icon, &summary, &body, actions, hints, path));
    }

    fn send(
        &self,
        icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<&str>,
        hints: HashMap<&str, Value>,
        path: Option<String>,
    ) {
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
        match (reply, path) {
            (Ok(r), Some(p)) => match r.body().deserialize::<u32>() {
                Ok(id) => {
                    self.paths.lock().unwrap().insert(id, PathBuf::from(p));
                }
                Err(e) => eprintln!("reelspace-recorder: bad Notify reply: {e}"),
            },
            (Err(e), _) => eprintln!("reelspace-recorder: Notify failed: {e}"),
            _ => {}
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
    start_on(Connection::session()?, cfg, cfg_path, sock)
}

fn start_on(
    conn: Connection,
    cfg: super::config::Config,
    cfg_path: PathBuf,
    sock: PathBuf,
) -> zbus::Result<(Arc<Control>, Connection)> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::time::{Duration, Instant};

    /// Private session bus; killed on drop. `None` when dbus-daemon is unavailable.
    struct Bus(std::process::Child, String);
    impl Bus {
        fn start() -> Option<Self> {
            let mut c = Command::new("dbus-daemon")
                .args(["--session", "--print-address", "--nofork"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;
            let mut addr = String::new();
            BufReader::new(c.stdout.take()?).read_line(&mut addr).ok()?;
            Some(Self(c, addr.trim().to_string()))
        }
        fn connect(&self) -> Connection {
            zbus::blocking::connection::Builder::address(self.1.as_str())
                .unwrap()
                .build()
                .unwrap()
        }
    }
    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    struct FakeNotifications;
    #[zbus::interface(name = "org.freedesktop.Notifications")]
    impl FakeNotifications {
        #[allow(clippy::too_many_arguments)]
        fn notify(
            &self,
            _app: &str,
            _replaces: u32,
            _icon: &str,
            _summary: &str,
            _body: &str,
            _actions: Vec<String>,
            _hints: HashMap<String, OwnedValue>,
            _timeout: i32,
        ) -> u32 {
            211
        }
    }

    struct FakeFileManager(Arc<Mutex<Vec<String>>>);
    #[zbus::interface(name = "org.freedesktop.FileManager1")]
    impl FakeFileManager {
        fn show_items(&self, uris: Vec<String>, _startup_id: &str) {
            self.0.lock().unwrap().extend(uris);
        }
    }

    #[test]
    fn action_invoked_show_calls_show_items() {
        let Some(bus) = Bus::start() else {
            eprintln!("dbus-daemon unavailable; skipping");
            return;
        };
        let calls = Arc::new(Mutex::new(Vec::new()));
        let server = bus.connect();
        server
            .object_server()
            .at("/org/freedesktop/Notifications", FakeNotifications)
            .unwrap();
        server
            .object_server()
            .at(
                "/org/freedesktop/FileManager1",
                FakeFileManager(calls.clone()),
            )
            .unwrap();
        server.request_name(NOTIF_DEST).unwrap();
        server.request_name("org.freedesktop.FileManager1").unwrap();

        let client = bus.connect();
        let notifier = Notifier::new(client.clone());
        notifier.notify("s", "b", "i", false, Some("/tmp/a b/clip_1.mp4"));
        let t = Instant::now();
        while notifier.paths.lock().unwrap().is_empty() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let me = client.unique_name().unwrap().to_owned();

        // GNOME may broadcast or unicast to the Notify caller; both must work.
        for dest in [None, Some(me.as_str())] {
            calls.lock().unwrap().clear();
            server
                .emit_signal(
                    dest,
                    "/org/freedesktop/Notifications",
                    NOTIF_DEST,
                    "ActionInvoked",
                    &(211u32, "show"),
                )
                .unwrap();
            let t = Instant::now();
            while calls.lock().unwrap().is_empty() && t.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(
                *calls.lock().unwrap(),
                vec!["file:///tmp/a%20b/clip_1.mp4".to_string()],
                "dest {dest:?}"
            );
        }
    }

    /// Regression: SaveClip while paused emitted Notify synchronously inside the
    /// method handler and deadlocked the daemon's connection.
    #[test]
    fn save_clip_while_paused_does_not_deadlock() {
        let Some(bus) = Bus::start() else {
            eprintln!("dbus-daemon unavailable; skipping");
            return;
        };
        let server = bus.connect();
        server
            .object_server()
            .at("/org/freedesktop/Notifications", FakeNotifications)
            .unwrap();
        server.request_name(NOTIF_DEST).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let (ctl, _conn) = start_on(
            bus.connect(),
            super::super::config::Config::default(),
            dir.path().join("recorder.json"),
            dir.path().join("gsr.sock"),
        )
        .unwrap();
        ctl.paused.store(true, std::sync::atomic::Ordering::SeqCst);

        let client = bus.connect();
        let call = |member: &str| {
            let (c, m) = (client.clone(), member.to_string());
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(
                    c.call_method(
                        Some(NAME),
                        PATH,
                        Some("org.reelspace.Recorder1"),
                        m.as_str(),
                        &(),
                    )
                    .is_ok(),
                );
            });
            rx.recv_timeout(Duration::from_secs(2))
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let c = client.clone();
        std::thread::spawn(move || {
            let _ = tx.send(
                c.call_method(
                    Some(NAME),
                    PATH,
                    Some("org.reelspace.Recorder1"),
                    "SaveClip",
                    &(0u32,),
                )
                .is_ok(),
            );
        });
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)),
            Ok(true),
            "SaveClip hung"
        );
        assert_eq!(call("Status"), Ok(true), "Status hung after SaveClip");
    }
}
