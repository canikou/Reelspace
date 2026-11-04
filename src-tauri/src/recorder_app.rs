//! Linux-only app side of the recorder: Tauri commands over the daemon's session D-Bus API,
//! systemd user-unit helpers, and the tray icon.
use crate::error::{app_error, user_error, AppResult, ErrorCode};
use crate::recorder::cli::json_of;
use crate::recorder::config::{config_path, Config};
use crate::recorder::dbus::{IFACE, NAME, PATH};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};
use zbus::blocking::{fdo::DBusProxy, Connection, Proxy};
use zbus::zvariant::OwnedValue;

const UNIT: &str = "reelspace-recorder.service";

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| {
        app_error(ErrorCode::Internal, "Recorder task failed").with_detail(e.to_string())
    })?
}

fn dbus_err(e: impl std::fmt::Display) -> crate::error::AppError {
    app_error(
        ErrorCode::SourceUnavailable,
        "The recorder could not be reached",
    )
    .with_detail(e.to_string())
}

/// Runs `f` against the daemon; `Ok(None)` when nothing owns the bus name.
fn with_daemon<R>(f: impl FnOnce(&Proxy) -> zbus::Result<R>) -> AppResult<Option<R>> {
    let conn = Connection::session().map_err(dbus_err)?;
    let owned = DBusProxy::new(&conn)
        .map_err(dbus_err)?
        .name_has_owner(NAME.try_into().expect("valid bus name"))
        .map_err(dbus_err)?;
    if !owned {
        return Ok(None);
    }
    let proxy = Proxy::new(&conn, NAME, PATH, IFACE).map_err(dbus_err)?;
    f(&proxy).map(Some).map_err(dbus_err)
}

fn require_daemon<R>(f: impl FnOnce(&Proxy) -> zbus::Result<R>) -> AppResult<R> {
    with_daemon(f)?.ok_or_else(|| {
        app_error(
            ErrorCode::SourceUnavailable,
            "The background recorder is not running",
        )
    })
}

fn status_json() -> AppResult<Value> {
    Ok(
        match with_daemon(|p| p.call::<_, _, HashMap<String, OwnedValue>>("Status", &()))? {
            Some(m) => {
                let mut v = json_of(m);
                v["running"] = json!(true);
                v
            }
            None => json!({ "running": false, "state": "not_running" }),
        },
    )
}

#[tauri::command]
pub async fn recorder_get_config() -> AppResult<Config> {
    blocking(|| Config::load(&config_path()).map_err(user_error)).await
}

/// Validates and writes the config, then asks a running daemon to reload it.
/// Returns whether a daemon picked it up; a missing daemon is not an error.
#[tauri::command]
pub async fn recorder_save_config(config: Config) -> AppResult<bool> {
    blocking(move || {
        config.validate().map_err(user_error)?;
        let path = config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(&config).map_err(|e| user_error(e.to_string()))?,
        )?;
        std::fs::rename(&tmp, &path)?;
        Ok(matches!(
            with_daemon(|p| p.call::<_, _, ()>("ReloadConfig", &())),
            Ok(Some(()))
        ))
    })
    .await
}

#[tauri::command]
pub async fn recorder_status() -> AppResult<Value> {
    blocking(status_json).await
}

#[tauri::command]
pub async fn recorder_save_clip() -> AppResult<bool> {
    // An empty job id means a save was already running.
    blocking(|| Ok(!require_daemon(|p| p.call::<_, _, String>("SaveClip", &(0u32,)))?.is_empty()))
        .await
}

#[tauri::command]
pub async fn recorder_pause() -> AppResult<()> {
    blocking(|| require_daemon(|p| p.call::<_, _, ()>("Pause", &()))).await
}

#[tauri::command]
pub async fn recorder_resume() -> AppResult<()> {
    blocking(|| require_daemon(|p| p.call::<_, _, ()>("Resume", &()))).await
}

/// Deletes only rec_*.mkv in rec_dir (enforced by the daemon); returns bytes freed.
#[tauri::command]
pub async fn recorder_delete_rolling_archive() -> AppResult<u64> {
    blocking(|| require_daemon(|p| p.call::<_, _, u64>("DeleteRollingArchive", &()))).await
}

fn systemctl(args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("systemctl")
        .arg("--user")
        .args(args)
        .arg(UNIT)
        .output()
}

fn word(args: &[&str]) -> String {
    match systemctl(args) {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                "unknown".into()
            } else {
                s
            }
        }
        Err(_) => "unavailable".into(),
    }
}

#[tauri::command]
pub async fn recorder_service_state() -> AppResult<Value> {
    blocking(|| Ok(json!({ "enabled": word(&["is-enabled"]), "active": word(&["is-active"]) })))
        .await
}

/// Only ever called from an explicit user click.
#[tauri::command]
pub async fn recorder_enable_service() -> AppResult<()> {
    blocking(|| {
        let out = systemctl(&["enable", "--now"]).map_err(|e| {
            app_error(ErrorCode::Internal, "systemctl is not available").with_detail(e.to_string())
        })?;
        if out.status.success() {
            return Ok(());
        }
        Err(app_error(
            ErrorCode::Internal,
            "Could not enable the background recorder",
        )
        .with_detail(String::from_utf8_lossy(&out.stderr).to_string()))
    })
    .await
}

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let save = MenuItem::with_id(app, "save", "Save clip", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Pause recording", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open library", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = MenuBuilder::new(app)
        .items(&[&save, &pause, &open, &quit])
        .build()?;
    let mut builder = TrayIconBuilder::with_id("recorder")
        .tooltip("Reelspace recorder")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "save" => {
                std::thread::spawn(|| {
                    with_daemon(|p| p.call::<_, _, String>("SaveClip", &(0u32,)))
                });
            }
            "pause" => {
                std::thread::spawn(|| {
                    let paused = matches!(status_json(), Ok(v) if v["state"] == "paused");
                    let method = if paused { "Resume" } else { "Pause" };
                    let _ = with_daemon(|p| p.call::<_, _, ()>(method, &()));
                });
            }
            "open" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.unminimize();
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let tray = builder.build(app)?;
    std::thread::spawn(move || loop {
        let state = status_json()
            .ok()
            .and_then(|v| v["state"].as_str().map(String::from))
            .unwrap_or_else(|| "not_running".into());
        let _ = tray.set_tooltip(Some(format!(
            "Reelspace recorder: {}",
            state.replace('_', " ")
        )));
        let _ = pause.set_text(if state == "paused" {
            "Resume recording"
        } else {
            "Pause recording"
        });
        std::thread::sleep(Duration::from_secs(5));
    });
    Ok(())
}
