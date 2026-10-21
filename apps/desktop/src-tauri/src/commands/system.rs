//! Version, tool list, file picker and "open result" commands.

use application::AppError;
use application::registry::ToolDescriptor;
use std::path::PathBuf;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::state::AppState;

/// Extensions offered in the file picker: PDFs and the image formats Images to PDF reads.
const PICKER_EXTENSIONS: [&str; 10] = [
    "pdf", "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "heic",
];

/// Returns the application version for the About dialog.
#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Every registered tool with its options schema. The UI builds its tool list from this.
#[tauri::command(async)]
pub fn list_tools(state: State<'_, AppState>) -> Vec<ToolDescriptor> {
    state.registry.list()
}

/// Shows the system file picker and returns the chosen paths, empty when cancelled.
#[tauri::command(async)]
pub fn pick_files(app: AppHandle) -> Vec<PathBuf> {
    app.dialog()
        .file()
        .add_filter("PDFs and images", &PICKER_EXTENSIONS)
        .blocking_pick_files()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .collect()
}

/// Shows the system folder picker and returns the chosen folder, `None` when cancelled.
#[tauri::command(async)]
pub fn pick_folder(app: AppHandle) -> Option<PathBuf> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
}

/// Picture formats a watermark can use.
const IMAGE_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

/// Shows the system file picker for an option that takes a path: `kind` is `image` for a picture,
/// `package` for an offline component package, `certificate` for a PFX or P12 file, `trust` for
/// certificates to trust, anything else for any file. Returns the chosen paths, empty when cancelled.
#[tauri::command(async)]
pub fn pick_paths(app: AppHandle, kind: String, multiple: bool) -> Vec<PathBuf> {
    let mut dialog = app.dialog().file();
    match kind.as_str() {
        "image" => dialog = dialog.add_filter("Pictures", &IMAGE_EXTENSIONS),
        "package" => dialog = dialog.add_filter("Quirewise packages", &["qwpkg"]),
        "certificate" => dialog = dialog.add_filter("Certificate files", &["pfx", "p12"]),
        "trust" => dialog = dialog.add_filter("Certificates", &["cer", "crt", "pem", "der"]),
        _ => {}
    }
    let picked = if multiple {
        dialog.blocking_pick_files().unwrap_or_default()
    } else {
        dialog.blocking_pick_file().into_iter().collect()
    };
    picked
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .collect()
}

fn known_output(state: &AppState, path: &std::path::Path) -> Result<(), AppError> {
    if state.jobs.is_known_output(path) {
        Ok(())
    } else {
        Err(AppError::new(
            domain::ErrorCode::PathDenied,
            "Only files that Quirewise just created can be opened from here.",
        ))
    }
}

/// Opens a result file with its default app. Only files produced by a job are allowed, so web
/// content cannot use this command to launch arbitrary files.
#[tauri::command(async)]
pub fn open_output(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
) -> Result<(), AppError> {
    known_output(&state, &path)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|_| AppError::internal("opening the file"))
}

/// Shows a result file in Explorer. Restricted to job outputs like [`open_output`].
#[tauri::command(async)]
pub fn reveal_output(
    app: AppHandle,
    state: State<'_, AppState>,
    path: PathBuf,
) -> Result<(), AppError> {
    known_output(&state, &path)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|_| AppError::internal("showing the file"))
}
