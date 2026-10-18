//! Event bridge: job events from the core to the UI.

use application::dto::BatchEvent;
use infrastructure::EventSink;
use tauri::{AppHandle, Emitter};

/// Name of the event the UI listens to for batch progress and results.
pub const BATCH_EVENT: &str = "batch";

/// Forwards batch events to the main window.
pub struct TauriSink {
    app: AppHandle,
}

impl TauriSink {
    /// Creates a sink that emits through `app`.
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl EventSink for TauriSink {
    fn emit(&self, event: BatchEvent) {
        // A closed window has nobody to tell; the job result is still recorded in the manager.
        let _ = self.app.emit(BATCH_EVENT, event);
    }
}
