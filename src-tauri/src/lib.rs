mod commands;
pub mod db;
pub mod error;
mod explorer;
pub mod ffmpeg;
mod jobs;
pub mod media;
pub mod models;
pub mod portability;
mod recovery;
mod state;
mod storage;
mod telemetry;

use commands::*;
use state::AppState;
use tauri::Manager;

pub fn run() -> error::AppResult<()> {
    let built = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let state = AppState::new(app.handle())?;
            state.telemetry.event(
                "info",
                "app.started",
                serde_json::json!({
                    "version": env!("CARGO_PKG_VERSION"),
                    "backgroundJobCapacity": 1,
                    "interactiveJobCapacity": 1,
                    "exportJobCapacity": 1,
                }),
            );
            let worker_state = state.clone();
            app.manage(state);
            start_thumbnail_worker(worker_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_state,
            save_settings,
            scan_sources,
            list_media,
            get_media_audio_tracks,
            get_audio_track_preview,
            cancel_audio_preview_session,
            log_client_event,
            repair_thumbnails,
            update_media_tags,
            update_media_notes,
            list_collections,
            list_collection_media,
            create_collection,
            add_to_collection,
            export_library_data,
            import_library_data,
            create_clip,
            reveal_in_explorer,
            get_storage_stats,
            get_cleanup_candidates,
            delete_raw_file
        ])
        .build(tauri::generate_context!());
    let app = match built {
        Ok(app) => app,
        Err(error) => {
            recovery::record_startup_failure(&error.to_string());
            return Err(error.into());
        }
    };
    app.run(|app_handle, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            app_handle
                .state::<AppState>()
                .thumbnail_scheduler
                .request_shutdown();
            app_handle.state::<AppState>().recovery.finish();
        }
    });
    Ok(())
}
