//! Shared application state managed by Tauri.

/// State handed to every command. Grows in Phase 1 with the job queue handle.
#[derive(Debug)]
pub struct AppState;
