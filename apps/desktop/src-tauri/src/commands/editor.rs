//! Commands of the editor: what is on a page that can be changed or filled, the fonts text can be
//! set in, and the signatures the person keeps. Reading documents happens in the worker; the app
//! process only forwards, and keeps the saved signatures sealed by Windows.

use application::query::{DocumentQuery, QueryResult};
use application::{AppError, CancellationToken, Password};
use domain::forms::{FieldSuggestion, FormField, TextBlock};
use domain::signatures::{NewSavedSignature, SavedSignature};
use std::path::PathBuf;
use tauri::State;

use crate::state::AppState;

fn ask(state: &AppState, query: &DocumentQuery) -> Result<QueryResult, AppError> {
    state.queries.query(query, &CancellationToken::new())
}

/// The installed fonts that text can be set in.
#[tauri::command(async)]
pub fn editor_fonts(state: State<'_, AppState>) -> Result<Vec<String>, AppError> {
    match ask(&state, &DocumentQuery::EditorFonts)? {
        QueryResult::EditorFonts(list) => Ok(list),
        _ => Err(AppError::internal("listing the fonts")),
    }
}

/// The fields of the form of a document.
#[tauri::command(async)]
pub fn form_fields(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Option<Password>,
) -> Result<Vec<FormField>, AppError> {
    match ask(&state, &DocumentQuery::FormFields { path, password })? {
        QueryResult::FormFields(list) => Ok(list),
        _ => Err(AppError::internal("reading the form")),
    }
}

/// Places on a page that look like fields to fill in. Page counts from 0.
#[tauri::command(async)]
pub fn suggest_fields(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Option<Password>,
    page: u32,
) -> Result<Vec<FieldSuggestion>, AppError> {
    match ask(
        &state,
        &DocumentQuery::SuggestFields {
            path,
            password,
            page,
        },
    )? {
        QueryResult::Suggestions(list) => Ok(list),
        _ => Err(AppError::internal("looking for fields")),
    }
}

/// The blocks of text of a page, for changing them. Page counts from 0.
#[tauri::command(async)]
pub fn text_blocks(
    state: State<'_, AppState>,
    path: PathBuf,
    password: Option<Password>,
    page: u32,
) -> Result<Vec<TextBlock>, AppError> {
    match ask(
        &state,
        &DocumentQuery::TextBlocks {
            path,
            password,
            page,
        },
    )? {
        QueryResult::TextBlocks(list) => Ok(list),
        _ => Err(AppError::internal("reading the page text")),
    }
}

/// The saved signatures, initials and stamps.
#[tauri::command(async)]
pub fn signatures_list(state: State<'_, AppState>) -> Result<Vec<SavedSignature>, AppError> {
    state.signatures.list()
}

/// Keeps a signature, sealed for this Windows user.
#[tauri::command(async)]
pub fn signature_save(
    state: State<'_, AppState>,
    signature: NewSavedSignature,
) -> Result<SavedSignature, AppError> {
    state.signatures.save(&signature)
}

/// Forgets a saved signature.
#[tauri::command(async)]
pub fn signature_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.signatures.delete(&id)
}
