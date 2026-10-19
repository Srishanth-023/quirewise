//! Quirewise desktop entry point. Commands live in `commands`, the event bridge in `events`.
#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;
mod state;

use events::TauriSink;
use state::AppState;
use std::sync::Arc;
use tauri::Manager;

fn main() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let sink = Arc::new(TauriSink::new(app.handle().clone()));
            let state = AppState::build(sink).map_err(|e| e.to_string())?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::app_version,
            commands::system::list_tools,
            commands::system::pick_files,
            commands::system::pick_paths,
            commands::documents::classify_inputs,
            commands::documents::inspect_document,
            commands::documents::render_thumbnail,
            commands::documents::ocr_languages,
            commands::scan::scan_available,
            commands::scan::scan_acquire,
            commands::scan::save_capture,
            commands::recent::recent_list,
            commands::recent::recent_add,
            commands::recent::recent_clear,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::jobs::run_batch,
            commands::jobs::cancel_job,
            commands::jobs::retry_failed,
            commands::components::components_installed,
            commands::components::components_available,
            commands::components::component_install,
            commands::components::component_import,
            commands::components::component_remove,
            commands::system::open_output,
            commands::system::reveal_output,
        ])
        .run(tauri::generate_context!());
    if let Err(err) = result {
        eprintln!("Quirewise failed to start: {err}");
        std::process::exit(1);
    }
}
