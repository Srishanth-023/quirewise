//! Settings commands.

use application::AppError;
use domain::settings::Settings;
use tauri::State;

use crate::state::AppState;

/// Current settings, with defaults when the file does not exist.
#[tauri::command(async)]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, AppError> {
    state.settings.load()
}

/// Validates and saves settings.
#[tauri::command(async)]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), AppError> {
    state.settings.save(&settings)
}
