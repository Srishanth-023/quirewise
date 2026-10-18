//! Quirewise desktop entry point. Commands live in `commands`, the event bridge in `events`.
#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;
mod state;

fn main() {
    let result = tauri::Builder::default()
        .manage(state::AppState)
        .invoke_handler(tauri::generate_handler![commands::app_version])
        .run(tauri::generate_context!());
    if let Err(err) = result {
        eprintln!("Quirewise failed to start: {err}");
        std::process::exit(1);
    }
}
