//! Batch job commands. Progress and results arrive as `batch` events.

use application::dto::RunBatchRequest;
use application::{AppError, InputDoc};
use domain::DomainError;
use domain::naming::OutputNamingPattern;
use infrastructure::BatchRequest;
use tauri::State;

use crate::state::AppState;

/// Starts a batch and returns its job id.
#[tauri::command(async)]
pub fn run_batch(state: State<'_, AppState>, request: RunBatchRequest) -> Result<u64, AppError> {
    let tool = state.registry.get(request.tool).ok_or_else(|| {
        AppError::new(
            domain::ErrorCode::ComponentMissing,
            format!("The {} tool is not available in this build.", request.tool),
        )
    })?;
    if request.options.tool_id() != request.tool {
        return Err(DomainError::option("options", "they belong to a different tool").into());
    }
    let pattern = match &request.naming_pattern {
        Some(text) => OutputNamingPattern::parse(text)?,
        None => state.settings.load()?.naming_pattern,
    };
    let inputs = request
        .inputs
        .into_iter()
        .map(|i| InputDoc {
            path: i.path,
            password: i.password,
            page_count: None,
        })
        .collect();
    Ok(state.jobs.start(BatchRequest {
        tool: request.tool,
        mode: tool.input_mode(),
        options: request.options,
        inputs,
        output_dir: request.output_dir,
        naming_pattern: pattern,
    }))
}

/// Asks a running job to stop. Files already finished keep their results.
#[tauri::command(async)]
pub fn cancel_job(state: State<'_, AppState>, job: u64) -> bool {
    state.jobs.cancel(job)
}

/// Runs again only the files that failed.
#[tauri::command(async)]
pub fn retry_failed(state: State<'_, AppState>, job: u64) -> Result<(), AppError> {
    state.jobs.retry_failed(job)
}
