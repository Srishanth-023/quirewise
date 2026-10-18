//! Recent files commands.

use application::{AppError, RecentFile};
use std::path::PathBuf;
use tauri::State;

use crate::state::AppState;

/// Recent files that still exist, newest first.
#[tauri::command(async)]
pub fn recent_list(state: State<'_, AppState>) -> Result<Vec<RecentFile>, AppError> {
    state.recent.list()
}

/// Records that a file was opened.
#[tauri::command(async)]
pub fn recent_add(state: State<'_, AppState>, path: PathBuf) -> Result<(), AppError> {
    state.recent.add(&path)
}

/// Clears the list.
#[tauri::command(async)]
pub fn recent_clear(state: State<'_, AppState>) -> Result<(), AppError> {
    state.recent.clear()
}
