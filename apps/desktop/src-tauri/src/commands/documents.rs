//! Read-only document commands. Parsing happens in the worker; the app process only forwards.

use application::dto::{DropAnalysis, classify_inputs as classify};
use application::query::{DocumentQuery, QueryResult, Thumbnail};
use application::{AppError, CancellationToken, DocumentReport, Password};
use std::path::PathBuf;
use tauri::State;

use crate::state::AppState;

/// Classifies dropped paths by magic bytes and suggests tools.
#[tauri::command(async)]
pub fn classify_inputs(state: State<'_, AppState>, paths: Vec<PathBuf>) -> DropAnalysis {
    classify(state.files.as_ref(), &paths)
}

/// Facts about a PDF for the Inspect panel. A missing password on an encrypted file returns a
/// locked report; a wrong password is an error.
#[tauri::command(async)]
pub fn inspect_document(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Option<Password>,
) -> Result<DocumentReport, AppError> {
    let query = DocumentQuery::Inspect { path, password };
    match state.queries.query(&query, &CancellationToken::new())? {
        QueryResult::Inspect(report) => Ok(*report),
        _ => Err(AppError::internal("inspecting the file")),
    }
}

/// Renders one page as a PNG preview.
#[tauri::command(async)]
pub fn render_thumbnail(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Option<Password>,
    page: u32,
    max_edge: u32,
) -> Result<Thumbnail, AppError> {
    let query = DocumentQuery::Thumbnail {
        path,
        password,
        page,
        max_edge,
    };
    match state.queries.query(&query, &CancellationToken::new())? {
        QueryResult::Thumbnail(thumbnail) => Ok(thumbnail),
        _ => Err(AppError::internal("rendering the page")),
    }
}

/// The text recognition languages that are installed, as Tesseract codes such as `eng`.
#[tauri::command(async)]
pub fn ocr_languages(state: State<'_, AppState>) -> Result<Vec<String>, AppError> {
    match state
        .queries
        .query(&DocumentQuery::OcrLanguages, &CancellationToken::new())?
    {
        QueryResult::OcrLanguages(list) => Ok(list),
        _ => Err(AppError::internal("listing the languages")),
    }
}
