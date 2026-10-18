//! Typed commands exposed to the UI. Each maps a DTO to an application use case.

/// Returns the application version for the About dialog.
#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
