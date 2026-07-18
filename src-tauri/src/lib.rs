mod commands;
pub mod db;
pub mod error;
mod explorer;
mod features;
pub mod ffmpeg;
mod jobs;
pub mod media;
pub mod models;
pub mod portability;
mod recovery;
mod state;
mod storage;
mod telemetry;
mod watcher;
mod youtube;

use commands::*;
use features::*;
use state::AppState;
use tauri::Manager;
use watcher::start_library_watcher;
use youtube::*;

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
            let archive_worker_state = state.clone();
            let watcher_state = state.clone();
            app.manage(state);
            start_thumbnail_worker(worker_state);
            start_youtube_worker(archive_worker_state);
            start_library_watcher(watcher_state);
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
            delete_raw_file,
            list_games,
            create_game,
            list_game_assignments,
            rename_game,
            merge_games,
            assign_game,
            list_archive_statuses,
            set_keep_local,
            queue_archive,
            list_upload_jobs,
            control_upload_queue,
            list_media_events,
            remove_from_collection,
            set_collection_storage_default,
            detect_recording_apps,
            launch_recording_app,
            remove_local_copy,
            open_remote_copy,
            repair_library,
            open_diagnostics_folder,
            export_diagnostics,
            get_archive_account,
            connect_youtube,
            disconnect_youtube,
            delete_youtube_remote
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
