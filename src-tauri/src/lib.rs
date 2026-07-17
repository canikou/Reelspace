mod commands;
mod db;
mod error;
mod explorer;
mod ffmpeg;
mod jobs;
mod media;
mod models;
mod state;
mod storage;
mod telemetry;

use commands::*;
use state::AppState;
use tauri::Manager;

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
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
            create_clip,
            reveal_in_explorer,
            get_storage_stats,
            get_cleanup_candidates,
            delete_raw_file
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Reelspace");
    app.run(|app_handle, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            app_handle
                .state::<AppState>()
                .thumbnail_scheduler
                .request_shutdown();
        }
    });
}
