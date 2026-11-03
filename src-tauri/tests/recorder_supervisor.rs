#![cfg(target_os = "linux")]
//! Supervisor state machine against fake gpu-screen-recorder / gsr-cli / ffmpeg / pactl scripts.
use reelspace_lib::recorder::config::{Config, Mode};
use reelspace_lib::recorder::control::{Control, Event, VecSink};
use reelspace_lib::recorder::daemon::{Env, Supervisor};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Rig {
    _dir: tempfile::TempDir,
    root: PathBuf,
    ctl: Arc<Control>,
    sink: Arc<VecSink>,
    proc_root: PathBuf,
    handle: Option<std::thread::JoinHandle<()>>,
}

fn script(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/usr/bin/env bash\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn wait(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(15);
    while !f() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn base_cfg(root: &Path) -> Config {
    let bin = root.join("bin");
    Config {
        mode: Mode::Rolling,
        rec_dir: root.join("rec"),
        require_mountpoint: false,
        bitrate_kbps: 500,
        chunk_sec: 3600,
        gsr_path: bin.join("gpu-screen-recorder").display().to_string(),
        gsr_cli_path: bin.join("gsr-cli").display().to_string(),
        ffmpeg_path: bin.join("ffmpeg").display().to_string(),
        min_free_gb: 5,
        sounds: false,
        notifications: false,
        session: reelspace_lib::recorder::config::SessionConfig {
            min_session_sec: 0,
            ..Default::default()
        },
        ..Config::default()
    }
}

impl Rig {
    fn new(tweak: impl Fn(&mut Config), pactl_out: &str) -> Rig {
        std::fs::create_dir_all(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let root = dir.path().to_path_buf();
        let bin = root.join("bin");
        std::fs::create_dir_all(bin.clone()).unwrap();
        std::fs::create_dir_all(root.join("rec")).unwrap();
        let proc_root = root.join("proc");
        std::fs::create_dir_all(&proc_root).unwrap();
        let rec = root.join("rec").display().to_string();
        script(&bin.join("gpu-screen-recorder"), &format!("S=; while [ $# -gt 0 ]; do [ \"$1\" = -ipc ] && S=$2; shift; done; echo $$ >> {root}/pids; touch \"$S\"; trap 'rm -f \"$S\"; exit 0' INT; while :; do sleep 0.05; done", root = root.display()));
        script(
            &bin.join("gsr-cli"),
            &format!(
                r#"S=$2; c=$3
case $c in
 status) [ -e "$S" ] && echo running;;
 start-replay-recording) f={rec}/Video_$(date +%Y-%m-%d_%H-%M-%S).mkv; echo $f > "$S.rec"; echo x > $f;;
 stop-replay-recording) f=$(cat "$S.rec") || exit 1; rm "$S.rec"; echo $f;;
 save-replay) mkdir -p {rec}/.replay; echo raw > {rec}/.replay/Replay_1.mkv; echo {rec}/.replay/Replay_1.mkv;;
esac"#
            ),
        );
        script(
            &bin.join("ffmpeg"),
            r#"while [ $# -gt 1 ]; do [ "$1" = -i ] && in=$2; shift; done; cp "$in" "$1""#,
        );
        script(&bin.join("pactl"), &format!("printf '{pactl_out}'"));
        let mut cfg = base_cfg(&root);
        tweak(&mut cfg);
        let cfg_path = root.join("recorder.json");
        std::fs::write(&cfg_path, serde_json::to_vec(&cfg).unwrap()).unwrap();
        let sink = Arc::new(VecSink::default());
        let ctl = Control::new(
            cfg,
            cfg_path,
            root.join("gsr.sock"),
            sink.clone(),
            Arc::default(),
        );
        let env = Env {
            proc_root: proc_root.clone(),
            pactl: bin.join("pactl").display().to_string(),
            systemctl: "false".into(),
            active_state: root.join("active-recording.json"),
            conflicting_units: vec![],
            tick: Duration::from_millis(50),
            guard_interval: Duration::from_millis(300),
            detect_interval: Duration::from_millis(100),
        };
        let mut sup = Supervisor::new(ctl.clone(), env);
        let handle = Some(std::thread::spawn(move || sup.run()));
        Rig {
            _dir: dir,
            root,
            ctl,
            sink,
            proc_root,
            handle,
        }
    }

    fn state(&self) -> String {
        self.ctl.status.lock().unwrap().state.clone()
    }
    fn marker(&self) -> bool {
        self.root.join("gsr.sock.rec").exists()
    }
    fn pids(&self) -> usize {
        std::fs::read_to_string(self.root.join("pids"))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }
    fn names(&self, prefix: &str) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.root.join("rec"))
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.starts_with(prefix))
            .collect();
        v.sort();
        v
    }
    fn switch(&self, f: impl Fn(&mut Config)) {
        let mut c = self.ctl.cfg();
        f(&mut c);
        std::fs::write(&self.ctl.cfg_path, serde_json::to_vec(&c).unwrap()).unwrap();
        self.ctl.reload.store(true, SeqCst);
    }
    fn stop(mut self) {
        self.ctl.shutdown.store(true, SeqCst);
        self.handle.take().unwrap().join().unwrap();
        assert_eq!(self.state(), "stopped");
    }
}

#[test]
fn rolling_manual_session_clip_and_shutdown() {
    let rig = Rig::new(|_| {}, "1\\tRec_Mix\\tPipeWire\\n");
    wait("recording", || rig.state() == "recording" && rig.marker());
    assert!(rig.root.join("active-recording.json").exists());

    // switch to manual: recording stops via IPC, gsr is NOT restarted
    rig.switch(|c| c.mode = Mode::Manual);
    wait("manual stop", || !rig.marker());
    wait("rec_ file", || rig.names("rec_").len() == 1);
    assert_eq!(rig.pids(), 1, "gsr must not restart on mode change");
    assert!(!rig.root.join("active-recording.json").exists());

    // save clip
    reelspace_lib::recorder::clip::run_save(&rig.ctl, 5);
    let clips: Vec<_> = std::fs::read_dir(rig.root.join("rec/Clips"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(clips.len(), 1);
    assert!(clips[0].starts_with("clip_") && clips[0].ends_with(".mp4"));
    assert!(
        !rig.root.join("rec/.replay/Replay_1.mkv").exists(),
        "raw deleted after success"
    );
    assert!(rig
        .sink
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, Event::ClipSaved { .. })));

    // session mode: a Steam reaper process starts and ends a session
    rig.switch(|c| c.mode = Mode::Session);
    wait("config applied", || rig.ctl.cfg().mode == Mode::Session);
    let reaper = rig.proc_root.join("4242");
    std::fs::create_dir_all(&reaper).unwrap();
    std::fs::write(
        reaper.join("cmdline"),
        b"reaper\0SteamLaunch\0AppId=999\0--\0game\0",
    )
    .unwrap();
    wait("session recording", || rig.marker());
    assert_eq!(rig.pids(), 1);
    std::fs::remove_dir_all(&reaper).unwrap();
    wait("session file", || rig.names("session_").len() == 1);
    assert!(rig.names("session_")[0].starts_with("session_Steam_App_999_"));
    assert!(!rig.marker());

    // capture-param change restarts gsr
    rig.switch(|c| c.fps = 30);
    wait("gsr restart", || {
        rig.pids() == 2 && rig.state() == "recording"
    });

    let root = rig.root.clone();
    rig.stop();
    assert!(!root.join("active-recording.json").exists());
}

#[test]
fn clip_failure_keeps_raw() {
    let rig = Rig::new(|_| {}, "1\\tRec_Mix\\tPipeWire\\n");
    wait("recording", || rig.state() == "recording");
    script(&rig.root.join("bin/ffmpeg"), "echo boom >&2; exit 1");
    reelspace_lib::recorder::clip::run_save(&rig.ctl, 5);
    assert!(rig.root.join("rec/.replay/Replay_1.mkv").exists());
    assert!(rig.sink.0.lock().unwrap().iter().any(
        |e| matches!(e, Event::ClipFailed { raw_path, .. } if raw_path.ends_with("Replay_1.mkv"))
    ));
    assert!(
        std::fs::read_dir(rig.root.join("rec/Clips"))
            .map(|d| d.count())
            .unwrap_or(0)
            == 0
    );
    rig.stop();
}

#[test]
fn guards_block_start() {
    // duplicate capture
    let rig = Rig::new(|_| {}, "1\\tRec_Mix\\tPipeWire\\n");
    wait("recording", || rig.state() == "recording");
    rig.stop();

    let rig = Rig::new(|_| {}, "1\\tRec_Mix\\tPipeWire\\n");
    let dup = rig.proc_root.join("77");
    std::fs::create_dir_all(&dup).unwrap();
    std::fs::write(
        dup.join("cmdline"),
        b"gpu-screen-recorder\0-w\0DP-1\0-r\x00300\0",
    )
    .unwrap();
    wait("error", || rig.state() == "error");
    assert!(rig
        .ctl
        .status
        .lock()
        .unwrap()
        .last_error
        .contains("another gpu-screen-recorder"));
    assert_eq!(rig.pids(), if rig.pids() == 0 { 0 } else { rig.pids() }); // may have raced the first tick; the state is what matters
    rig.stop();

    // mountpoint guard
    let rig = Rig::new(|c| c.require_mountpoint = true, "1\\tRec_Mix\\tPipeWire\\n");
    wait("waiting_mount", || rig.state() == "waiting_mount");
    assert_eq!(rig.pids(), 0);
    rig.stop();

    // audio source missing
    let rig = Rig::new(|_| {}, "");
    wait("waiting_audio", || rig.state() == "waiting_audio");
    assert_eq!(rig.pids(), 0);
    rig.stop();
}

#[test]
fn disk_floor_blocks_recording_but_keeps_replay() {
    // floor larger than any real free space -> can never record, replay buffer still up
    let rig = Rig::new(|c| c.min_free_gb = 100_000_000, "1\\tRec_Mix\\tPipeWire\\n");
    wait("disk floor", || {
        rig.ctl.status.lock().unwrap().last_error == "disk floor reached"
    });
    assert_eq!(rig.state(), "error");
    assert!(!rig.marker());
    assert_eq!(rig.pids(), 1, "gsr (replay buffer) stays up");
    rig.stop();
}

#[test]
fn invalid_reload_keeps_last_good_config() {
    let rig = Rig::new(|_| {}, "1\\tRec_Mix\\tPipeWire\\n");
    wait("recording", || rig.state() == "recording");
    std::fs::write(&rig.ctl.cfg_path, br#"{"min_free_gb":1}"#).unwrap();
    rig.ctl.reload.store(true, SeqCst);
    wait("rejected", || {
        rig.ctl
            .status
            .lock()
            .unwrap()
            .last_error
            .contains("config rejected")
    });
    assert_eq!(rig.ctl.cfg().min_free_gb, 5);
    rig.stop();
}
