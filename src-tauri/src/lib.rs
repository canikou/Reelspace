mod commands;
mod db;
mod error;
mod explorer;
mod ffmpeg;
mod media;
mod models;
mod state;
mod storage;

use commands::*;
use state::AppState;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::new(app.handle())?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_state,
            save_settings,
            scan_sources,
            list_media,
            update_media_tags,
            update_media_notes,
            list_collections,
            create_collection,
            add_to_collection,
            create_clip,
            reveal_in_explorer,
            get_storage_stats,
            get_cleanup_candidates,
            delete_raw_file
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Reelspace");
}
