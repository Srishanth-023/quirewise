//! Recipes (Appendix A row 58): the saved list, export and import, and the chained run.
//!
//! The chain runs one file at a time through a [`UnitRunner`], so every step keeps the worker
//! isolation and the verified output of a normal run. Each step except the last writes into its
//! own scratch folder with the plain `{filename}` pattern, which keeps the final name clean.

use domain::naming::OutputNamingPattern;
use domain::recipe::{Recipe, RecipeFile};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::error::AppError;
use crate::ports::FileStore;
use crate::recent::UnixClock;
use crate::run_tool::{UnitRequest, UnitRunner};
use crate::runtime::{CancellationToken, ProgressEvent, ProgressSink};
use crate::tool::{InputDoc, OutputArtifact};

/// An exported recipe file larger than this is not read.
pub const MAX_FILE_BYTES: usize = 1 << 20;
/// Most recipes kept.
pub const MAX_RECIPES: usize = 200;

/// Persistence for the recipe list.
pub trait RecipeStore: Send + Sync {
    /// Loads the saved recipes. A missing or damaged file loads as empty.
    fn load(&self) -> Result<Vec<Recipe>, AppError>;
    /// Replaces the saved recipes.
    fn save(&self, recipes: &[Recipe]) -> Result<(), AppError>;
}

/// The recipe list use cases.
pub struct Recipes {
    store: Arc<dyn RecipeStore>,
    clock: Arc<dyn UnixClock>,
    lock: Mutex<()>,
}

impl Recipes {
    /// Wires the list to its store.
    pub fn new(store: Arc<dyn RecipeStore>, clock: Arc<dyn UnixClock>) -> Self {
        Self {
            store,
            clock,
            lock: Mutex::new(()),
        }
    }

    /// All recipes, in the order they were saved.
    pub fn list(&self) -> Result<Vec<Recipe>, AppError> {
        self.store.load()
    }

    /// Saves a recipe. An empty id adds a new one; a known id replaces that recipe.
    pub fn save(&self, mut recipe: Recipe) -> Result<Recipe, AppError> {
        recipe.validate()?;
        recipe.name = recipe.name.trim().to_owned();
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut all = self.store.load()?;
        match all.iter().position(|r| r.id == recipe.id) {
            Some(i) if !recipe.id.is_empty() => all[i] = recipe.clone(),
            _ => {
                if all.len() >= MAX_RECIPES {
                    return Err(domain::DomainError::option(
                        "recipes",
                        format!("keep at most {MAX_RECIPES} recipes"),
                    )
                    .into());
                }
                recipe.id = self.new_id(&all);
                all.push(recipe.clone());
            }
        }
        self.store.save(&all)?;
        Ok(recipe)
    }

    /// Removes a recipe. A missing id is not an error.
    pub fn delete(&self, id: &str) -> Result<(), AppError> {
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut all = self.store.load()?;
        all.retain(|r| r.id != id);
        self.store.save(&all)
    }

    /// Renames a recipe and returns it.
    pub fn rename(&self, id: &str, name: &str) -> Result<Recipe, AppError> {
        let mut recipe = self
            .list()?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| domain::DomainError::option("recipe", "that recipe no longer exists"))?;
        recipe.name = name.to_owned();
        self.save(recipe)
    }

    /// The JSON text of an exported recipe.
    pub fn export(&self, id: &str) -> Result<String, AppError> {
        let recipe = self
            .list()?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| domain::DomainError::option("recipe", "that recipe no longer exists"))?;
        serde_json::to_string_pretty(&RecipeFile::wrap(&recipe))
            .map_err(|_| AppError::internal("exporting the recipe"))
    }

    /// Adds the recipe in `text` (an exported file) as a new entry.
    pub fn import(&self, text: &str) -> Result<Recipe, AppError> {
        if text.len() > MAX_FILE_BYTES {
            return Err(domain::DomainError::option("file", "this file is too large").into());
        }
        let file: RecipeFile = serde_json::from_str(text).map_err(|_| {
            domain::DomainError::option("file", "this is not a readable Quirewise recipe")
        })?;
        file.check()?;
        self.save(Recipe {
            id: String::new(),
            ..file.recipe
        })
    }

    fn new_id(&self, existing: &[Recipe]) -> String {
        let base = self.clock.unix_seconds();
        (0u32..)
            .map(|n| format!("r{base}-{n}"))
            .find(|id| existing.iter().all(|r| &r.id != id))
            .unwrap_or_default()
    }
}

/// One file's journey through a recipe.
pub struct RecipeUnit<'a> {
    /// The recipe to run. Checked again here.
    pub recipe: &'a Recipe,
    /// The input, with its password.
    pub input: InputDoc,
    /// Folder for the final outputs. `None` means next to the input.
    pub output_dir: Option<PathBuf>,
    /// Naming pattern for the final outputs.
    pub naming_pattern: OutputNamingPattern,
    /// Session scratch folder for intermediate files.
    pub temp_dir: &'a Path,
}

static RUNS: AtomicU64 = AtomicU64::new(0);

/// Scales a step's progress into its share of the whole recipe.
struct StepProgress<'a> {
    inner: &'a dyn ProgressSink,
    step: u64,
    steps: u64,
}

const SCALE: u64 = 1000;

impl ProgressSink for StepProgress<'_> {
    fn report(&self, event: ProgressEvent) {
        let part = (event.done.min(event.total) * SCALE)
            .checked_div(event.total)
            .unwrap_or(0);
        self.inner.report(ProgressEvent {
            done: self.step * SCALE + part,
            total: self.steps * SCALE,
        });
    }
}

/// Runs every step of the recipe on one file and returns the last step's outputs.
///
/// Intermediate files are removed afterwards, also when a step fails.
pub fn run_recipe_unit(
    runner: &dyn UnitRunner,
    files: &dyn FileStore,
    unit: &RecipeUnit<'_>,
    cancel: &CancellationToken,
    progress: &dyn ProgressSink,
) -> Result<Vec<OutputArtifact>, AppError> {
    unit.recipe.validate()?;
    let final_dir = match &unit.output_dir {
        Some(dir) => dir.clone(),
        None => unit
            .input
            .path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                domain::DomainError::option("output folder", "choose where the result goes")
            })?,
    };
    let run = RUNS.fetch_add(1, Ordering::SeqCst);
    let steps = unit.recipe.steps.len();
    let plain = OutputNamingPattern::parse("{filename}")?;
    let mut scratch: Vec<PathBuf> = Vec::new();
    let result = (|| {
        let mut current = unit.input.clone();
        for (i, step) in unit.recipe.steps.iter().enumerate() {
            cancel.check()?;
            let is_last = i + 1 == steps;
            let request = UnitRequest {
                tool: step.tool(),
                options: step.options.clone(),
                inputs: vec![current.clone()],
                output_dir: Some(if is_last {
                    final_dir.clone()
                } else {
                    unit.temp_dir.join(format!("recipe-{run}-{i}"))
                }),
                naming_pattern: if is_last {
                    unit.naming_pattern.clone()
                } else {
                    plain.clone()
                },
            };
            let sink = StepProgress {
                inner: progress,
                step: i as u64,
                steps: steps as u64,
            };
            let outputs = runner
                .run_unit(&request, cancel, &sink)
                .map_err(|e| e.with_context(&format!("Step {} ({})", i + 1, step.tool())))?;
            if is_last {
                return Ok(outputs);
            }
            let [only] = outputs.as_slice() else {
                return Err(AppError::new(
                    domain::ErrorCode::OptionInvalid,
                    format!(
                        "Step {} ({}) wrote {} files, so the next step has nothing to continue with.",
                        i + 1,
                        step.tool(),
                        outputs.len()
                    ),
                ));
            };
            scratch.push(only.path.clone());
            current = InputDoc::new(only.path.clone());
        }
        Ok(Vec::new())
    })();
    for path in &scratch {
        files.remove_file(path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::NoProgress;
    use crate::testing::MemoryFiles;
    use domain::ToolId;
    use domain::ToolOptions;
    use domain::recipe::RecipeStep;

    #[derive(Default)]
    struct MemStore(Mutex<Vec<Recipe>>);
    impl RecipeStore for MemStore {
        fn load(&self) -> Result<Vec<Recipe>, AppError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn save(&self, recipes: &[Recipe]) -> Result<(), AppError> {
            *self.0.lock().unwrap() = recipes.to_vec();
            Ok(())
        }
    }

    struct Tick;
    impl UnixClock for Tick {
        fn unix_seconds(&self) -> u64 {
            7
        }
    }

    fn recipes() -> Recipes {
        Recipes::new(Arc::new(MemStore::default()), Arc::new(Tick))
    }

    fn recipe(name: &str, tools: &[ToolId]) -> Recipe {
        Recipe {
            id: String::new(),
            name: name.into(),
            steps: tools
                .iter()
                .map(|t| RecipeStep {
                    options: ToolOptions::defaults_for(*t),
                })
                .collect(),
        }
    }

    #[test]
    fn save_rename_delete_and_unique_ids() {
        let r = recipes();
        let a = r.save(recipe("A", &[ToolId::Rotate])).unwrap();
        let b = r.save(recipe("B", &[ToolId::Compress])).unwrap();
        assert_ne!(a.id, b.id);
        let renamed = r.rename(&a.id, "  Tidy ").unwrap();
        assert_eq!(renamed.name, "Tidy");
        assert_eq!(r.list().unwrap().len(), 2, "rename replaces, never adds");
        r.delete(&a.id).unwrap();
        r.delete("missing").unwrap();
        assert_eq!(r.list().unwrap(), vec![b]);
        assert!(r.save(recipe("", &[ToolId::Rotate])).is_err());
        assert!(r.rename("missing", "x").is_err());
    }

    #[test]
    fn export_then_import_adds_a_new_entry() {
        let r = recipes();
        let a = r
            .save(recipe("A", &[ToolId::Rotate, ToolId::Compress]))
            .unwrap();
        let text = r.export(&a.id).unwrap();
        assert!(text.contains("quirewise-recipe"));
        let imported = r.import(&text).unwrap();
        assert_ne!(imported.id, a.id);
        assert_eq!(imported.steps, a.steps);
        assert_eq!(r.list().unwrap().len(), 2);
    }

    #[test]
    fn bad_imports_are_refused_with_a_reason() {
        let r = recipes();
        assert!(r.import("{ nope").is_err());
        assert!(r.import(r#"{"format":"x","version":1}"#).is_err());
        let a = r.save(recipe("A", &[ToolId::Rotate])).unwrap();
        let text = r
            .export(&a.id)
            .unwrap()
            .replace("\"version\": 1", "\"version\": 99");
        assert!(r.import(&text).is_err());
        assert!(r.import(&"x".repeat(MAX_FILE_BYTES + 1)).is_err());
        assert_eq!(r.list().unwrap().len(), 1);
    }

    /// Writes `<dir>/<stem>.pdf` into the memory store and records every request.
    struct Fake {
        files: Arc<MemoryFiles>,
        seen: Mutex<Vec<UnitRequest>>,
        fail_tool: Option<ToolId>,
        two_outputs_from: Option<ToolId>,
    }

    impl UnitRunner for Fake {
        fn run_unit(
            &self,
            request: &UnitRequest,
            cancel: &CancellationToken,
            progress: &dyn ProgressSink,
        ) -> Result<Vec<OutputArtifact>, AppError> {
            cancel.check()?;
            self.seen.lock().unwrap().push(request.clone());
            progress.report(ProgressEvent { done: 1, total: 2 });
            if Some(request.tool) == self.fail_tool {
                return Err(AppError::new(domain::ErrorCode::PdfDamaged, "broken"));
            }
            let dir = request.output_dir.clone().unwrap();
            let stem = request.inputs[0].stem();
            let name = format!(
                "{}.pdf",
                if request.naming_pattern.as_str() == "{filename}" {
                    stem
                } else {
                    format!("{stem} (done)")
                }
            );
            let make = |n: &str| {
                let path = dir.join(n);
                self.files.put(path.to_str().unwrap(), b"%PDF-1.7");
                OutputArtifact {
                    path,
                    pages: Some(1),
                    bytes: 8,
                }
            };
            if Some(request.tool) == self.two_outputs_from {
                return Ok(vec![make("p1.pdf"), make("p2.pdf")]);
            }
            Ok(vec![make(&name)])
        }
    }

    fn fake(fail_tool: Option<ToolId>, two: Option<ToolId>) -> (Fake, Arc<MemoryFiles>) {
        let files = Arc::new(MemoryFiles::default());
        files.put("C:\\in\\a.pdf", b"%PDF-1.7");
        (
            Fake {
                files: files.clone(),
                seen: Mutex::new(Vec::new()),
                fail_tool,
                two_outputs_from: two,
            },
            files,
        )
    }

    fn unit<'a>(recipe: &'a Recipe) -> RecipeUnit<'a> {
        RecipeUnit {
            recipe,
            input: InputDoc::new("C:\\in\\a.pdf"),
            output_dir: None,
            naming_pattern: OutputNamingPattern::default(),
            temp_dir: Path::new("C:\\tmp"),
        }
    }

    #[derive(Default)]
    struct Ticks(Mutex<Vec<(u64, u64)>>);
    impl ProgressSink for Ticks {
        fn report(&self, event: ProgressEvent) {
            self.0.lock().unwrap().push((event.done, event.total));
        }
    }

    #[test]
    fn chains_steps_and_cleans_up_the_scratch_files() {
        let (runner, files) = fake(None, None);
        let r = recipe("R", &[ToolId::Rotate, ToolId::Compress, ToolId::Linearize]);
        let ticks = Ticks::default();
        let out = run_recipe_unit(
            &runner,
            files.as_ref(),
            &unit(&r),
            &CancellationToken::new(),
            &ticks,
        )
        .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, Path::new("C:\\in\\a (done).pdf"));
        let seen = runner.seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0].inputs[0].path, Path::new("C:\\in\\a.pdf"));
        assert!(seen[1].inputs[0].path.starts_with("C:\\tmp"));
        assert_eq!(seen[1].inputs[0].stem(), "a", "the stem survives the chain");
        assert_eq!(seen[2].output_dir.as_deref(), Some(Path::new("C:\\in")));
        assert!(!files.exists(&seen[1].inputs[0].path), "scratch removed");
        assert!(!files.exists(&seen[2].inputs[0].path), "scratch removed");
        assert!(files.exists(Path::new("C:\\in\\a.pdf")), "input untouched");
        let t = ticks.0.lock().unwrap();
        assert!(t.iter().all(|(d, total)| d <= total && *total == 3000));
        assert!(
            t.windows(2).all(|w| w[0].0 <= w[1].0),
            "never goes backwards"
        );
    }

    #[test]
    fn a_failing_step_names_itself_and_still_cleans_up() {
        let (runner, files) = fake(Some(ToolId::Compress), None);
        let r = recipe("R", &[ToolId::Rotate, ToolId::Compress, ToolId::Linearize]);
        let err = run_recipe_unit(
            &runner,
            files.as_ref(),
            &unit(&r),
            &CancellationToken::new(),
            &NoProgress,
        )
        .unwrap_err();
        assert_eq!(err.code, domain::ErrorCode::PdfDamaged);
        assert!(err.message.contains("Step 2"), "{}", err.message);
        let first = runner.seen.lock().unwrap()[1].inputs[0].path.clone();
        assert!(!files.exists(&first), "scratch removed after failure");
    }

    #[test]
    fn a_middle_step_with_several_outputs_stops_the_chain() {
        let (runner, files) = fake(None, Some(ToolId::Rotate));
        let r = recipe("R", &[ToolId::Rotate, ToolId::Compress]);
        let err = run_recipe_unit(
            &runner,
            files.as_ref(),
            &unit(&r),
            &CancellationToken::new(),
            &NoProgress,
        )
        .unwrap_err();
        assert_eq!(err.code, domain::ErrorCode::OptionInvalid);
        assert_eq!(runner.seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn an_invalid_recipe_and_a_cancel_run_nothing() {
        let (runner, files) = fake(None, None);
        let bad = recipe("R", &[ToolId::Merge]);
        assert!(
            run_recipe_unit(
                &runner,
                files.as_ref(),
                &unit(&bad),
                &CancellationToken::new(),
                &NoProgress
            )
            .is_err()
        );
        let good = recipe("R", &[ToolId::Rotate]);
        let token = CancellationToken::new();
        token.cancel();
        let err = run_recipe_unit(&runner, files.as_ref(), &unit(&good), &token, &NoProgress)
            .unwrap_err();
        assert_eq!(err.code, domain::ErrorCode::JobCancelled);
        assert!(runner.seen.lock().unwrap().is_empty());
    }
}
