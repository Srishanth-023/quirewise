//! Recipe commands: the saved list, export and import through system dialogs, and running a
//! recipe on files or a folder. Progress and results arrive as the same `batch` events as a
//! single-tool run.

use application::dto::{FolderRef, RecipeRoleEntry, RunRecipeRequest};
use application::recipe::MAX_FILE_BYTES;
use application::{AppError, InputDoc};
use domain::naming::OutputNamingPattern;
use domain::recipe::{ChainRole, Recipe};
use domain::{DomainError, ToolId};
use infrastructure::queue::RecipeRun;
use infrastructure::{BatchRequest, folder_inputs};
use std::path::PathBuf;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::state::AppState;

/// Saved recipes, in the order they were saved.
#[tauri::command(async)]
pub fn recipes_list(state: State<'_, AppState>) -> Result<Vec<Recipe>, AppError> {
    state.recipes.list()
}

/// Adds a recipe (empty id) or replaces the one with that id.
#[tauri::command(async)]
pub fn recipe_save(state: State<'_, AppState>, recipe: Recipe) -> Result<Recipe, AppError> {
    state.recipes.save(recipe)
}

/// Removes a recipe.
#[tauri::command(async)]
pub fn recipe_delete(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.recipes.delete(&id)
}

/// Renames a recipe.
#[tauri::command(async)]
pub fn recipe_rename(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<Recipe, AppError> {
    state.recipes.rename(&id, &name)
}

/// Asks where to save a recipe and writes it as JSON. Returns false when the dialog is closed.
#[tauri::command(async)]
pub fn recipe_export(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, AppError> {
    let text = state.recipes.export(&id)?;
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("Quirewise recipe", &["json"])
        .set_file_name("recipe.json")
        .blocking_save_file()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(false);
    };
    // The save dialog already asked the user about replacing an existing file.
    infrastructure::file_store::replace_own_file(&path, text.as_bytes())?;
    Ok(true)
}

/// Asks for a recipe file and adds it to the list. Returns `None` when the dialog is closed.
#[tauri::command(async)]
pub fn recipe_import(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<Recipe>, AppError> {
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("Quirewise recipe", &["json"])
        .blocking_pick_file()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(None);
    };
    let size = std::fs::metadata(&path)
        .map_err(|e| AppError::from_io("read the recipe file", &e))?
        .len();
    if size > MAX_FILE_BYTES as u64 {
        return Err(DomainError::option("file", "this file is too large").into());
    }
    let bytes = std::fs::read(&path).map_err(|e| AppError::from_io("read the recipe file", &e))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| DomainError::option("file", "this is not a readable Quirewise recipe"))?;
    state.recipes.import(&text).map(Some)
}

/// Where each tool may sit in a recipe. The builder offers only tools that can run unattended.
#[tauri::command]
pub fn recipe_roles() -> Vec<RecipeRoleEntry> {
    ToolId::ALL
        .iter()
        .map(|&tool| RecipeRoleEntry {
            tool,
            role: ChainRole::of(tool),
        })
        .collect()
}

/// The files a folder run would start from, so the UI can show a count before running.
#[tauri::command(async)]
pub fn folder_files(folder: FolderRef, with_office: bool) -> Result<Vec<PathBuf>, AppError> {
    folder_inputs(&folder.path, folder.recursive, with_office)
}

/// Runs a recipe on files and/or a folder and returns the job id.
#[tauri::command(async)]
pub fn run_recipe(state: State<'_, AppState>, request: RunRecipeRequest) -> Result<u64, AppError> {
    request.recipe.validate()?;
    let pattern = match &request.naming_pattern {
        Some(text) => OutputNamingPattern::parse(text)?,
        None => state.settings.load()?.naming_pattern,
    };
    let mut inputs: Vec<InputDoc> = request
        .inputs
        .into_iter()
        .map(|i| InputDoc {
            path: i.path,
            password: i.password,
            page_count: None,
        })
        .collect();
    if let Some(folder) = &request.folder {
        for path in folder_inputs(
            &folder.path,
            folder.recursive,
            request.recipe.starts_with_source(),
        )? {
            if !inputs.iter().any(|i| i.path == path) {
                inputs.push(InputDoc::new(path));
            }
        }
    }
    if inputs.is_empty() {
        return Err(DomainError::option(
            "files",
            "add files or choose a folder with documents in it",
        )
        .into());
    }
    let first = request.recipe.steps[0].options.clone();
    Ok(state.jobs.start(BatchRequest {
        tool: first.tool_id(),
        mode: application::InputMode::PerFile,
        options: first,
        inputs,
        output_dir: request.output_dir,
        naming_pattern: pattern,
        without_files: false,
        recipe: Some(RecipeRun {
            recipe: request.recipe,
            temp_dir: state.temp_dir.clone(),
        }),
    }))
}
