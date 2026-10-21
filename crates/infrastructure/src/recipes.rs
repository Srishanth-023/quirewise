//! JSON-file store for saved recipes, and the folder scan used to run a recipe on a folder.

use application::AppError;
use application::recipe::RecipeStore;
use domain::recipe::Recipe;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::file_store::replace_own_file;

/// Saved recipes in one JSON file in the roaming app data folder.
#[derive(Debug, Clone)]
pub struct JsonRecipeStore {
    path: PathBuf,
}

impl JsonRecipeStore {
    /// Creates a store for the file at `path`.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl RecipeStore for JsonRecipeStore {
    fn load(&self) -> Result<Vec<Recipe>, AppError> {
        match fs::read(&self.path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(list) => Ok(list),
                // Keep the damaged file for the user instead of silently overwriting it later.
                Err(_) => {
                    let _ = fs::rename(&self.path, self.path.with_extension("damaged"));
                    Ok(Vec::new())
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(AppError::from_io("read your recipes", &e)),
        }
    }

    fn save(&self, recipes: &[Recipe]) -> Result<(), AppError> {
        let bytes = serde_json::to_vec_pretty(recipes)
            .map_err(|_| AppError::internal("saving your recipes"))?;
        replace_own_file(&self.path, &bytes)
    }
}

/// Most files one folder run may start.
pub const MAX_FOLDER_FILES: usize = 2000;
/// Deepest sub-folder level scanned.
const MAX_DEPTH: usize = 8;

const PDF: &[&str] = &["pdf"];
const OFFICE: &[&str] = &[
    "docx", "doc", "docm", "dotx", "rtf", "odt", "pptx", "ppt", "pps", "ppsx", "odp", "xlsx",
    "xls", "xlsm", "ods",
];

/// Windows marks symbolic links, junctions and other reparse points with this attribute.
const REPARSE_POINT: u32 = 0x400;
const HIDDEN: u32 = 0x2;

#[cfg(windows)]
fn attributes(meta: &fs::Metadata) -> u32 {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes()
}

#[cfg(not(windows))]
fn attributes(_: &fs::Metadata) -> u32 {
    0
}

/// The files in `folder` a recipe can start from: PDFs, plus Office files when the first step
/// converts them. Links, hidden files and Office lock files (`~$name`) are skipped, so a folder
/// run never follows a link out of the folder. Sorted by name.
pub fn folder_inputs(
    folder: &Path,
    recursive: bool,
    with_office: bool,
) -> Result<Vec<PathBuf>, AppError> {
    let mut found = Vec::new();
    scan(folder, recursive, with_office, 0, &mut found)?;
    found.sort_by_key(|p| p.to_string_lossy().to_lowercase());
    Ok(found)
}

fn scan(
    dir: &Path,
    recursive: bool,
    with_office: bool,
    depth: usize,
    found: &mut Vec<PathBuf>,
) -> Result<(), AppError> {
    let entries = fs::read_dir(dir).map_err(|e| AppError::from_io("read the folder", &e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        let attrs = attributes(&meta);
        if meta.file_type().is_symlink() || attrs & (REPARSE_POINT | HIDDEN) != 0 {
            continue;
        }
        if meta.is_dir() {
            if recursive && depth < MAX_DEPTH {
                scan(&path, recursive, with_office, depth + 1, found)?;
            }
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("~$") || name.starts_with('.') {
            continue;
        }
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let wanted = PDF.contains(&ext.as_str()) || (with_office && OFFICE.contains(&ext.as_str()));
        if meta.is_file() && wanted {
            if found.len() >= MAX_FOLDER_FILES {
                return Err(domain::DomainError::option(
                    "folder",
                    format!(
                        "this folder has more than {MAX_FOLDER_FILES} files; pick a smaller one"
                    ),
                )
                .into());
            }
            found.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::ToolId;
    use domain::ToolOptions;
    use domain::recipe::RecipeStep;
    use tempfile::tempdir;

    fn recipe(name: &str) -> Recipe {
        Recipe {
            id: format!("id-{name}"),
            name: name.into(),
            steps: vec![RecipeStep {
                options: ToolOptions::defaults_for(ToolId::Compress),
            }],
        }
    }

    #[test]
    fn round_trips_and_starts_empty() {
        let dir = tempdir().unwrap();
        let store = JsonRecipeStore::new(dir.path().join("sub").join("recipes.json"));
        assert!(store.load().unwrap().is_empty());
        store.save(&[recipe("a"), recipe("b")]).unwrap();
        assert_eq!(store.load().unwrap(), vec![recipe("a"), recipe("b")]);
    }

    #[test]
    fn a_damaged_file_is_set_aside_not_overwritten() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("recipes.json");
        fs::write(&path, b"{ not json").unwrap();
        let store = JsonRecipeStore::new(path.clone());
        assert!(store.load().unwrap().is_empty());
        assert_eq!(
            fs::read(path.with_extension("damaged")).unwrap(),
            b"{ not json"
        );
    }

    fn touch(dir: &Path, name: &str) {
        fs::write(dir.join(name), b"x").unwrap();
    }

    #[test]
    fn picks_pdfs_sorted_and_skips_lock_files() {
        let dir = tempdir().unwrap();
        for n in ["b.pdf", "A.PDF", "c.docx", "~$d.pdf", "e.txt"] {
            touch(dir.path(), n);
        }
        fs::create_dir(dir.path().join("sub")).unwrap();
        touch(&dir.path().join("sub"), "f.pdf");

        let flat = folder_inputs(dir.path(), false, false).unwrap();
        let names: Vec<_> = flat
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, ["A.PDF", "b.pdf"]);

        let deep = folder_inputs(dir.path(), true, true).unwrap();
        let names: Vec<_> = deep
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, ["A.PDF", "b.pdf", "c.docx", "f.pdf"]);
    }

    #[test]
    fn an_unreadable_folder_is_an_error() {
        let dir = tempdir().unwrap();
        assert!(folder_inputs(&dir.path().join("missing"), false, false).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn junctions_are_not_followed() {
        let outside = tempdir().unwrap();
        touch(outside.path(), "secret.pdf");
        let dir = tempdir().unwrap();
        let link = dir.path().join("link");
        // mklink /J needs no elevation, unlike a symbolic link.
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !made {
            return;
        }
        assert!(folder_inputs(dir.path(), true, false).unwrap().is_empty());
    }
}
