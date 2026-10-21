//! `qw run`: runs any registered tool on files through the same queue the app uses.

use application::batch::BatchReport;
use application::{
    AppError, InputDoc, InputMode, Password, ProgressEvent, ToolRegistry, UnitRunner,
};
use domain::naming::OutputNamingPattern;
use domain::{DomainError, ToolId, ToolOptions};
use infrastructure::queue::{BatchObserver, BatchRequest, JobQueue};
use std::path::PathBuf;
use std::sync::Arc;

struct Silent;

impl BatchObserver for Silent {
    fn on_progress(&self, _unit: usize, _event: ProgressEvent) {}
    fn on_finished(&self, _unit: usize, _report: &application::batch::FileReport) {}
}

/// Parses `--options` JSON into the options of `tool`, filling every missing field from the
/// defaults. The `tool` tag is added here so callers only write the tool's own fields.
pub fn parse_options(tool: ToolId, json: Option<&str>) -> Result<ToolOptions, DomainError> {
    let Some(text) = json else {
        return Ok(ToolOptions::defaults_for(tool));
    };
    let mut value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| DomainError::option("options", format!("not valid JSON ({e})")))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| DomainError::option("options", "must be a JSON object"))?;
    object.insert(
        "tool".to_owned(),
        serde_json::Value::String(tool.to_string()),
    );
    serde_json::from_value(value).map_err(|e| DomainError::option("options", e.to_string()))
}

/// What one `qw run` needs.
pub struct RunArgs<'a> {
    /// Tool id, for example `rotate`.
    pub tool: &'a str,
    /// Input files.
    pub files: &'a [PathBuf],
    /// Options JSON, or none for the defaults.
    pub options: Option<&'a str>,
    /// Output folder, or none for next to each source.
    pub out: Option<PathBuf>,
    /// Password applied to every input.
    pub password: Option<Password>,
}

/// Runs the tool and returns one line per result. Any failed unit makes the whole call an error
/// whose message lists every unit, finished or not.
pub fn run_tool(
    registry: &ToolRegistry,
    runner: Arc<dyn UnitRunner>,
    args: RunArgs<'_>,
) -> Result<String, AppError> {
    let id: ToolId = args.tool.parse()?;
    let tool = registry.get(id).ok_or_else(|| {
        AppError::new(
            domain::ErrorCode::ComponentMissing,
            format!("The {id} tool is not registered in this build."),
        )
    })?;
    let options = parse_options(id, args.options)?;
    let without_files = args.files.is_empty() && tool.runs_without_files(&options);
    if args.files.is_empty() && !without_files {
        return Err(DomainError::option("files", "add at least one file").into());
    }
    let inputs: Vec<InputDoc> = args
        .files
        .iter()
        .map(|path| {
            let mut doc = InputDoc::new(path);
            doc.password = args.password.clone();
            doc
        })
        .collect();
    let cores = std::thread::available_parallelism().map_or(2, usize::from);
    let slots = domain::settings::Settings::default().heavy_job_slots(cores);
    let request = BatchRequest {
        tool: id,
        mode: tool.input_mode(),
        options,
        inputs,
        output_dir: args.out,
        naming_pattern: OutputNamingPattern::default(),
        without_files,
        recipe: None,
    };
    let report =
        JobQueue::new(runner, slots).run(&request, &application::CancellationToken::new(), &Silent);
    summarize(&report)
}

/// What one `qw recipe` needs.
pub struct RecipeArgs<'a> {
    /// An exported recipe file.
    pub recipe: &'a std::path::Path,
    /// Input files.
    pub files: &'a [PathBuf],
    /// A folder to take files from, in addition to `files`.
    pub folder: Option<PathBuf>,
    /// Also scan sub-folders.
    pub recursive: bool,
    /// Output folder, or none for next to each source.
    pub out: Option<PathBuf>,
    /// Password applied to every input.
    pub password: Option<Password>,
}

/// Runs an exported recipe on files and/or a folder, like `run_tool` does for one tool.
pub fn run_recipe(runner: Arc<dyn UnitRunner>, args: RecipeArgs<'_>) -> Result<String, AppError> {
    let text = std::fs::read(args.recipe)
        .map_err(|e| AppError::from_io("read the recipe file", &e))
        .and_then(|bytes| {
            if bytes.len() > application::recipe::MAX_FILE_BYTES {
                return Err(DomainError::option("recipe", "this file is too large").into());
            }
            String::from_utf8(bytes)
                .map_err(|_| DomainError::option("recipe", "this is not a Quirewise recipe").into())
        })?;
    let file: domain::recipe::RecipeFile = serde_json::from_str(&text)
        .map_err(|_| DomainError::option("recipe", "this is not a readable Quirewise recipe"))?;
    file.check()?;
    let mut paths = args.files.to_vec();
    if let Some(folder) = &args.folder {
        let found = infrastructure::folder_inputs(
            folder,
            args.recursive,
            file.recipe.starts_with_source(),
        )?;
        for path in found {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    if paths.is_empty() {
        return Err(
            DomainError::option("files", "add files or a folder with documents in it").into(),
        );
    }
    let inputs = paths
        .iter()
        .map(|path| {
            let mut doc = InputDoc::new(path);
            doc.password = args.password.clone();
            doc
        })
        .collect();
    let first = file.recipe.steps[0].options.clone();
    let temp = std::env::temp_dir().join(format!("Quirewise-cli-{}", std::process::id()));
    let request = BatchRequest {
        tool: first.tool_id(),
        mode: InputMode::PerFile,
        options: first,
        inputs,
        output_dir: args.out,
        naming_pattern: OutputNamingPattern::default(),
        without_files: false,
        recipe: Some(infrastructure::queue::RecipeRun {
            recipe: file.recipe,
            temp_dir: temp.clone(),
        }),
    };
    let cores = std::thread::available_parallelism().map_or(2, usize::from);
    let slots = domain::settings::Settings::default().heavy_job_slots(cores);
    let report =
        JobQueue::new(runner, slots).run(&request, &application::CancellationToken::new(), &Silent);
    let _ = std::fs::remove_dir_all(&temp);
    summarize(&report)
}

fn summarize(report: &BatchReport) -> Result<String, AppError> {
    let mut lines = Vec::new();
    let mut first_error: Option<AppError> = None;
    for file in &report.files {
        let inputs: Vec<String> = file
            .inputs
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        let inputs = inputs.join(" + ");
        match &file.error {
            None => {
                for output in &file.outputs {
                    lines.push(format!(
                        "{inputs} -> {} ({} pages, {} bytes)",
                        output.path.display(),
                        output.pages.unwrap_or(0),
                        output.bytes
                    ));
                }
            }
            Some(error) => {
                lines.push(format!("{inputs}: {error}"));
                first_error.get_or_insert_with(|| error.clone());
            }
        }
    }
    let text = lines.join("\n");
    match first_error {
        None => Ok(text),
        Some(error) => Err(AppError::new(error.code, format!("\n{text}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::{CancellationToken, OutputArtifact, ProgressSink, UnitRequest};
    use domain::ErrorCode;

    #[test]
    fn options_json_is_tagged_and_filled_with_defaults() {
        let o = parse_options(ToolId::Rotate, Some(r#"{"angle":"180"}"#)).unwrap();
        let ToolOptions::Rotate(r) = o else {
            panic!("wrong tool")
        };
        assert_eq!(r.angle.degrees(), 180);
        assert_eq!(r.pages.to_string(), "all");
        assert_eq!(
            parse_options(ToolId::SplitEveryN, None).unwrap(),
            ToolOptions::defaults_for(ToolId::SplitEveryN)
        );
    }

    #[test]
    fn bad_options_are_rejected_with_the_option_code() {
        for bad in ["not json", "[1]", r#"{"angel":"90"}"#, r#"{"angle":"45"}"#] {
            let err = parse_options(ToolId::Rotate, Some(bad)).unwrap_err();
            assert_eq!(err.code(), ErrorCode::OptionInvalid, "{bad}");
        }
    }

    struct Fake;
    impl UnitRunner for Fake {
        fn run_unit(
            &self,
            request: &UnitRequest,
            _: &CancellationToken,
            _: &dyn ProgressSink,
        ) -> Result<Vec<OutputArtifact>, AppError> {
            let name = request.inputs[0].path.to_string_lossy().into_owned();
            if name.contains("bad") {
                return Err(AppError::new(ErrorCode::PdfDamaged, "damaged"));
            }
            Ok(vec![OutputArtifact {
                path: PathBuf::from(format!("{name}.out")),
                pages: Some(2),
                bytes: 9,
            }])
        }
    }

    fn registry() -> ToolRegistry {
        crate::default_registry().unwrap()
    }

    fn args<'a>(files: &'a [PathBuf]) -> RunArgs<'a> {
        RunArgs {
            tool: "rotate",
            files,
            options: None,
            out: None,
            password: None,
        }
    }

    #[test]
    fn prints_one_line_per_result() {
        let files = vec![PathBuf::from("a.pdf"), PathBuf::from("b.pdf")];
        let text = run_tool(&registry(), Arc::new(Fake), args(&files)).unwrap();
        assert!(
            text.contains("a.pdf -> a.pdf.out (2 pages, 9 bytes)"),
            "{text}"
        );
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn a_failed_file_makes_the_run_fail_but_still_lists_everything() {
        let files = vec![PathBuf::from("ok.pdf"), PathBuf::from("bad.pdf")];
        let err = run_tool(&registry(), Arc::new(Fake), args(&files)).unwrap_err();
        assert_eq!(err.code, ErrorCode::PdfDamaged);
        assert!(err.message.contains("ok.pdf -> ok.pdf.out"));
        assert!(err.message.contains("bad.pdf: QW-PDF-DAMAGED"));
    }

    #[test]
    fn unknown_tools_and_missing_registry_entries_are_reported() {
        let files = vec![PathBuf::from("a.pdf")];
        let err = run_tool(
            &registry(),
            Arc::new(Fake),
            RunArgs {
                tool: "nope",
                ..args(&files)
            },
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
        let empty = ToolRegistry::new();
        let err = run_tool(&empty, Arc::new(Fake), args(&files)).unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentMissing);
    }

    fn recipe_file(dir: &std::path::Path, tools: &[ToolId]) -> PathBuf {
        use domain::recipe::{Recipe, RecipeFile, RecipeStep};
        let file = RecipeFile::wrap(&Recipe {
            id: String::new(),
            name: "R".into(),
            steps: tools
                .iter()
                .map(|t| RecipeStep {
                    options: ToolOptions::defaults_for(*t),
                })
                .collect(),
        });
        let path = dir.join("r.json");
        std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
        path
    }

    fn recipe_args<'a>(recipe: &'a std::path::Path, files: &'a [PathBuf]) -> RecipeArgs<'a> {
        RecipeArgs {
            recipe,
            files,
            folder: None,
            recursive: false,
            out: None,
            password: None,
        }
    }

    #[test]
    fn a_recipe_runs_every_step_and_reports_the_last_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let recipe = recipe_file(dir.path(), &[ToolId::Rotate, ToolId::Compress]);
        let files = vec![PathBuf::from("a.pdf")];
        let text = run_recipe(Arc::new(Fake), recipe_args(&recipe, &files)).unwrap();
        assert!(text.contains("a.pdf -> "), "{text}");
        assert!(text.contains(".out.out"), "two steps chained: {text}");
    }

    #[test]
    fn a_recipe_folder_is_scanned_for_pdfs() {
        let dir = tempfile::tempdir().unwrap();
        let recipe = recipe_file(dir.path(), &[ToolId::Rotate]);
        let docs = dir.path().join("docs");
        std::fs::create_dir(&docs).unwrap();
        std::fs::write(docs.join("one.pdf"), b"x").unwrap();
        std::fs::write(docs.join("note.txt"), b"x").unwrap();
        let args = RecipeArgs {
            folder: Some(docs),
            ..recipe_args(&recipe, &[])
        };
        let text = run_recipe(Arc::new(Fake), args).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("one.pdf"));
    }

    #[test]
    fn bad_recipes_and_empty_runs_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let files = vec![PathBuf::from("a.pdf")];
        let missing = dir.path().join("none.json");
        assert!(run_recipe(Arc::new(Fake), recipe_args(&missing, &files)).is_err());
        let junk = dir.path().join("junk.json");
        std::fs::write(&junk, b"{}").unwrap();
        assert_eq!(
            run_recipe(Arc::new(Fake), recipe_args(&junk, &files))
                .unwrap_err()
                .code,
            ErrorCode::OptionInvalid
        );
        let bad_chain = recipe_file(dir.path(), &[ToolId::Merge]);
        assert!(run_recipe(Arc::new(Fake), recipe_args(&bad_chain, &files)).is_err());
        let ok = recipe_file(dir.path(), &[ToolId::Rotate]);
        assert!(run_recipe(Arc::new(Fake), recipe_args(&ok, &[])).is_err());
    }
}
