//! Commands of the tools that use the language model: what can be chosen, which models are
//! ready, and the preview of Smart Split. The model itself runs in the worker.

use application::dto::{AiCatalog, ai_catalog as catalog};
use application::query::{DocumentQuery, QueryResult};
use application::{AppError, CancellationToken, Password};
use domain::tools::ai::{AiOptions, SmartSplitOptions, SplitPart};
use domain::tools::redact::RedactOptions;
use std::path::PathBuf;
use tauri::State;

use crate::state::AppState;

/// The models that can be downloaded, with their licenses, and the languages that can be written.
#[tauri::command]
pub fn ai_catalog() -> AiCatalog {
    catalog()
}

/// The models that are ready to run: the downloaded ones, or those of the Ollama server on this
/// computer (the options must carry the person's agreement for that).
#[tauri::command(async)]
pub fn ai_models(state: State<'_, AppState>, ai: AiOptions) -> Result<Vec<String>, AppError> {
    match state
        .queries
        .query(&DocumentQuery::AiModels { ai }, &CancellationToken::new())?
    {
        QueryResult::AiModels(list) => Ok(list),
        _ => Err(AppError::internal("listing the models")),
    }
}

/// The parts Smart Split would make, for the person to check before anything is written.
#[tauri::command(async)]
pub fn smart_split_preview(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Option<Password>,
    options: SmartSplitOptions,
) -> Result<Vec<SplitPart>, AppError> {
    match state.queries.query(
        &DocumentQuery::SmartSplit {
            path,
            password,
            options,
        },
        &CancellationToken::new(),
    )? {
        QueryResult::SmartSplit(parts) => Ok(parts),
        _ => Err(AppError::internal("proposing the parts")),
    }
}

/// The personal details the model finds on the pages Redact searches, to add to the words.
#[tauri::command(async)]
pub fn redact_suggestions(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Option<Password>,
    options: RedactOptions,
) -> Result<Vec<String>, AppError> {
    match state.queries.query(
        &DocumentQuery::RedactSuggestions {
            path,
            password,
            options,
        },
        &CancellationToken::new(),
    )? {
        QueryResult::RedactTerms(terms) => Ok(terms),
        _ => Err(AppError::internal("asking the model")),
    }
}
