//! `reelspace-recorder` entry point: daemon + one-shot D-Bus client commands.
use super::config::{config_path, Config};
use super::daemon::{Env, Supervisor};
use super::dbus::{self, IFACE, NAME, PATH};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedValue;

static SIGNALLED: AtomicBool = AtomicBool::new(false);
extern "C" fn on_signal(_: libc::c_int) {
    SIGNALLED.store(true, SeqCst);
}

fn runtime_dir() -> std::path::PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(Into::into)
        .unwrap_or_else(|| "/tmp".into())
}

fn daemon() -> i32 {
    let path = config_path();
    // Invalid config at startup: run on defaults but surface the error rather than crash-loop.
    let (cfg, cfg_err) = match Config::load(&path) {
        Ok(c) => (c, None),
        Err(e) => (Config::default(), Some(e)),
    };
    let (ctl, _conn) = match dbus::start(cfg, path, runtime_dir().join("reelspace-gsr.sock")) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("reelspace-recorder: cannot own {NAME} on the session bus: {e}");
            return 1;
        }
    };
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as usize);
        libc::signal(libc::SIGTERM, on_signal as *const () as usize);
    }
    let c = ctl.clone();
    std::thread::spawn(move || {
        while !SIGNALLED.load(SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        c.shutdown.store(true, SeqCst);
    });
    if let Some(e) = cfg_err {
        eprintln!("reelspace-recorder: config rejected, using defaults: {e}");
        ctl.status.lock().unwrap().last_error = format!("config rejected: {e}");
    }
    Supervisor::new(ctl, Env::system()).run();
    0
}

fn proxy() -> Result<Proxy<'static>, String> {
    let conn = Connection::session().map_err(|e| e.to_string())?;
    Proxy::new(&conn, NAME, PATH, IFACE).map_err(|e| e.to_string())
}

pub(crate) fn json_of(m: HashMap<String, OwnedValue>) -> serde_json::Value {
    use zbus::zvariant::Value as V;
    m.into_iter()
        .map(|(k, v)| {
            let j = match &*v {
                V::Str(s) => serde_json::json!(s.as_str()),
                V::U32(n) => serde_json::json!(n),
                V::U64(n) => serde_json::json!(n),
                V::I64(n) => serde_json::json!(n),
                V::F64(n) => serde_json::json!(n),
                V::Bool(b) => serde_json::json!(b),
                other => serde_json::json!(format!("{other:?}")),
            };
            (k, j)
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

fn client(args: &[String]) -> Result<(), String> {
    let cmd = args[0].as_str();
    let p = proxy()?;
    let e = |e: zbus::Error| format!("recorder daemon not reachable: {e}");
    match cmd {
        "save-clip" => {
            let secs = args
                .iter()
                .position(|a| a == "--seconds")
                .and_then(|i| args.get(i + 1))
                .map(|s| {
                    s.parse::<u32>()
                        .map_err(|_| "--seconds needs a number".to_string())
                })
                .transpose()?
                .unwrap_or(0);
            let id: String = p.call("SaveClip", &(secs,)).map_err(e)?;
            if id.is_empty() {
                eprintln!("a clip save is already in progress");
            }
        }
        "status" => {
            let m: HashMap<String, OwnedValue> = p.call("Status", &()).map_err(e)?;
            println!("{}", serde_json::to_string_pretty(&json_of(m)).unwrap());
        }
        "pause" => p.call::<_, _, ()>("Pause", &()).map_err(e)?,
        "resume" => p.call::<_, _, ()>("Resume", &()).map_err(e)?,
        "reload" => p.call::<_, _, ()>("ReloadConfig", &()).map_err(e)?,
        _ => return Err(format!("unknown command '{cmd}'")),
    }
    Ok(())
}

pub fn main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("run") => daemon(),
        // gsr -sc target: results are collected from gsr-cli, so the hook has nothing to do.
        Some("hook") => 0,
        Some("--help" | "-h") => {
            println!("usage: reelspace-recorder [run | save-clip [--seconds N] | status | pause | resume | reload | hook <file> <type>]");
            0
        }
        Some(_) => match client(&args) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("reelspace-recorder: {e}");
                1
            }
        },
    }
}
