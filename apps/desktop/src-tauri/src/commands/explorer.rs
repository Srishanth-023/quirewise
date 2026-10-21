//! Explorer integration commands and the launch request (files and a tool named on the command
//! line, from an Explorer entry or "Open with").

use application::AppError;
use domain::DomainError;
use domain::explorer::{Launch, MenuItem};
use infrastructure::explorer::{self, RegExe};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::AppState;

fn not_portable(state: &AppState) -> Result<(), AppError> {
    if state.portable {
        return Err(DomainError::option(
            "explorer menu",
            "the Explorer menu is not available in portable mode, because it would write to this computer's registry",
        )
        .into());
    }
    Ok(())
}

/// True when the Quirewise submenu is in the right-click menu of PDF files.
#[tauri::command(async)]
pub fn explorer_status() -> bool {
    explorer::is_installed(&RegExe::current_user())
}

/// The tools the submenu can offer.
#[tauri::command]
pub fn explorer_tools() -> Vec<domain::ToolId> {
    domain::ToolId::ALL
        .into_iter()
        .filter(|t| domain::explorer::offered_in_menu(*t))
        .collect()
}

/// Adds the submenu with `items` and the "Open with" entry for the current user, and remembers
/// the chosen tools in the settings.
#[tauri::command(async)]
pub fn explorer_enable(
    state: State<'_, AppState>,
    items: Vec<MenuItem>,
    open_title: String,
) -> Result<(), AppError> {
    not_portable(&state)?;
    let exe = std::env::current_exe().map_err(|e| AppError::from_io("locate the app", &e))?;
    explorer::install(&RegExe::current_user(), &exe, &items, &open_title)?;
    let mut settings = state.settings.load()?;
    settings.explorer_tools = items.iter().map(|i| i.tool).collect();
    state.settings.save(&settings)
}

/// Removes the submenu and the "Open with" entry.
#[tauri::command(async)]
pub fn explorer_disable(state: State<'_, AppState>) -> Result<(), AppError> {
    not_portable(&state)?;
    explorer::uninstall(&RegExe::current_user())
}

/// What the app was started with, once. Later calls return nothing.
#[tauri::command]
pub fn take_launch(state: State<'_, AppState>) -> Launch {
    std::mem::take(
        &mut *state
            .launch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

/// Name of the event that carries a launch request from a second start to the running window.
pub const LAUNCH_EVENT: &str = "launch";

/// A second start (another Explorer entry, "Open with") hands its command line to the running
/// app, which adds the files and comes to the front, instead of opening another window.
pub fn forward_launch(app: &AppHandle, args: &[String], cwd: &str) {
    let mut launch = domain::explorer::parse_launch(args.iter().skip(1)).unwrap_or_default();
    for file in &mut launch.files {
        if file.is_relative() {
            *file = PathBuf::from(cwd).join(&*file);
        }
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    // Nothing to tell when the window is gone; the next start reads its own command line.
    let _ = app.emit(LAUNCH_EVENT, launch);
}
