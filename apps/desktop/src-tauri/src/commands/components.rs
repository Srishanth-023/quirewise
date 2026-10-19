//! Component commands: what is installed, what could be, and installing, importing and removing.
//!
//! Anything that uses the network needs `consent`, which the UI sends only after the person
//! confirmed in a dialog that names what will be downloaded.

use application::AppError;
use application::dto::ComponentProgress;
use application::runtime::{ProgressEvent, ProgressSink};
use domain::ErrorCode;
use domain::components::ComponentEntry;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

/// Name of the event that carries download progress. Matches the UI.
const PROGRESS_EVENT: &str = "component-progress";

fn consent_needed() -> AppError {
    AppError::new(
        ErrorCode::NetworkConsentRequired,
        "This needs the internet. Quirewise asks first and only downloads what you confirm.",
    )
}

/// Components installed on this computer.
#[tauri::command(async)]
pub fn components_installed(state: State<'_, AppState>) -> Result<Vec<ComponentEntry>, AppError> {
    let mut list = state.components.installed()?;
    list.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(list)
}

/// Components that can be installed on this computer, from the signed list. Needs the internet.
#[tauri::command(async)]
pub fn components_available(
    state: State<'_, AppState>,
    consent: bool,
) -> Result<Vec<ComponentEntry>, AppError> {
    if !consent {
        return Err(consent_needed());
    }
    state.installer.available()
}

struct EventProgress {
    app: AppHandle,
    id: String,
}

impl ProgressSink for EventProgress {
    fn report(&self, event: ProgressEvent) {
        // A lost progress event only makes the bar jump; the install itself is not affected.
        let _ = self.app.emit(
            PROGRESS_EVENT,
            ComponentProgress {
                id: self.id.clone(),
                done: event.done,
                total: event.total,
            },
        );
    }
}

/// Downloads, verifies and installs a component.
#[tauri::command(async)]
pub fn component_install(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    consent: bool,
) -> Result<ComponentEntry, AppError> {
    if !consent {
        return Err(consent_needed());
    }
    let progress = EventProgress {
        app,
        id: id.clone(),
    };
    state.installer.install(&id, &progress)
}

/// Installs a component from a `.qwpkg` file the person chose.
#[tauri::command(async)]
pub fn component_import(
    state: State<'_, AppState>,
    path: PathBuf,
) -> Result<ComponentEntry, AppError> {
    state.installer.import(&path)
}

/// Removes a component.
#[tauri::command(async)]
pub fn component_remove(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.installer.remove(&id)
}
