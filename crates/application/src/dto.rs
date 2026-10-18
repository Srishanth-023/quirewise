//! Data transfer objects for the UI boundary. Every type here derives `JsonSchema`, and
//! `qw schema dtos` exports them so the TypeScript types are generated, never hand-written.

use domain::job::{FileStatus, JobStatus};
use domain::settings::Settings;
use domain::{ToolId, ToolOptions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::input::{SNIFF_BYTES, sniff};
use crate::inspect::{DocumentReport, Suggestion, suggest_for_batch};
use crate::ports::FileStore;
use crate::query::Thumbnail;
use crate::recent::RecentFile;
use crate::registry::ToolDescriptor;
use crate::runtime::Password;
use crate::tool::{InputKind, OutputArtifact};

/// One file the user added, with an optional password for this session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InputRef {
    /// Path of the file.
    pub path: PathBuf,
    /// Password, held in UI memory for the session only.
    pub password: Option<Password>,
}

/// Asks the core to run a tool over a batch of files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunBatchRequest {
    /// Tool id.
    pub tool: ToolId,
    /// Options for the tool.
    pub options: ToolOptions,
    /// Files, in the order the user arranged them.
    pub inputs: Vec<InputRef>,
    /// Output folder, or `None` for next to each source.
    pub output_dir: Option<PathBuf>,
    /// Naming pattern, or `None` for the one in settings.
    pub naming_pattern: Option<String>,
}

/// What `classify_inputs` learned about one dropped path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InputSummary {
    /// The path as given.
    pub path: PathBuf,
    /// File name for display.
    pub name: String,
    /// Detected kind, or `None` when the file is not a PDF or a supported image.
    pub kind: Option<InputKindDto>,
    /// Why the file cannot be used, when it cannot.
    pub error: Option<AppError>,
}

/// Wire form of [`InputKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum InputKindDto {
    /// A PDF document.
    Pdf,
    /// A raster image.
    Image,
}

impl From<InputKind> for InputKindDto {
    fn from(kind: InputKind) -> Self {
        match kind {
            InputKind::Pdf => Self::Pdf,
            InputKind::Image => Self::Image,
        }
    }
}

/// Result of classifying a drop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DropAnalysis {
    /// One entry per dropped path.
    pub files: Vec<InputSummary>,
    /// Tools worth suggesting for this combination of files.
    pub suggestions: Vec<Suggestion>,
}

/// Classifies dropped paths by magic bytes only. No parser touches the files.
pub fn classify_inputs(files: &dyn FileStore, paths: &[PathBuf]) -> DropAnalysis {
    let mut summaries = Vec::with_capacity(paths.len());
    let mut kinds = Vec::new();
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let outcome = classify_one(files, path);
        let (kind, error) = match outcome {
            Ok(kind) => {
                kinds.push(kind);
                (Some(kind.into()), None)
            }
            Err(error) => (None, Some(error)),
        };
        summaries.push(InputSummary {
            path: path.clone(),
            name,
            kind,
            error,
        });
    }
    DropAnalysis {
        files: summaries,
        suggestions: suggest_for_batch(&kinds),
    }
}

fn classify_one(files: &dyn FileStore, path: &Path) -> Result<InputKind, AppError> {
    domain::path_rules::check_path_text(
        &path.to_string_lossy(),
        domain::path_rules::UncPolicy::UserPicked,
    )?;
    let canonical = files.canonicalize(path)?;
    let head = files.read_head(&canonical, SNIFF_BYTES)?;
    sniff(&head).ok_or_else(|| {
        AppError::new(
            domain::ErrorCode::InputNotPdf,
            "This file is not a PDF or a supported image.",
        )
    })
}

/// Progress and results of a running batch, pushed to the UI as events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum BatchEvent {
    /// A unit reported progress.
    Progress {
        /// Job id.
        job: u64,
        /// Index of the unit (file) within the job.
        unit: usize,
        /// Units done, for example pages.
        done: u64,
        /// Units total.
        total: u64,
    },
    /// A unit finished.
    UnitFinished {
        /// Job id.
        job: u64,
        /// Index of the unit.
        unit: usize,
        /// Resulting status.
        status: FileStatus,
        /// Error, when it did not finish.
        error: Option<AppError>,
        /// Outputs, when it finished.
        outputs: Vec<OutputArtifact>,
    },
    /// The whole job finished.
    JobFinished {
        /// Job id.
        job: u64,
        /// Overall status.
        status: JobStatus,
    },
}

/// Every type the UI needs, so one schema export generates all TypeScript types.
///
/// Fields carry no doc comments on purpose: a description next to a `$ref` makes the type
/// generator emit a duplicate inline type for each field.
#[derive(Debug, JsonSchema)]
// Fields are read only by the schema derive, and carry no docs on purpose (see above).
#[allow(dead_code, missing_docs)]
pub struct IpcTypes {
    pub document_report: DocumentReport,
    pub thumbnail: Thumbnail,
    pub recent_file: RecentFile,
    pub tool_descriptor: ToolDescriptor,
    pub drop_analysis: DropAnalysis,
    pub batch_event: BatchEvent,
    pub settings: Settings,
    pub app_error: AppError,
    pub output_artifact: OutputArtifact,
    pub run_batch_request: RunBatchRequest,
}

/// JSON Schema of all UI types, as pretty JSON text.
pub fn ipc_schema_json() -> String {
    let schema = schemars::schema_for!(IpcTypes);
    serde_json::to_string_pretty(&schema).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MemoryFiles;

    fn files() -> MemoryFiles {
        let f = MemoryFiles::default();
        f.put("C:\\a.pdf", b"%PDF-1.7");
        f.put("C:\\b.pdf", b"%PDF-1.4");
        f.put("C:\\p.png", b"\x89PNG\r\n\x1a\n");
        f.put("C:\\z.zip", b"PK\x03\x04");
        f
    }

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn two_pdfs_suggest_merge() {
        let a = classify_inputs(&files(), &[p("C:\\a.pdf"), p("C:\\b.pdf")]);
        assert_eq!(a.suggestions, vec![Suggestion::Merge]);
        assert!(a.files.iter().all(|f| f.kind == Some(InputKindDto::Pdf)));
    }

    #[test]
    fn images_suggest_images_to_pdf() {
        let a = classify_inputs(&files(), &[p("C:\\p.png")]);
        assert_eq!(a.suggestions, vec![Suggestion::ImagesToPdf]);
        assert_eq!(a.files[0].kind, Some(InputKindDto::Image));
    }

    #[test]
    fn unusable_files_carry_a_reason_and_do_not_affect_suggestions() {
        let a = classify_inputs(
            &files(),
            &[p("C:\\z.zip"), p("C:\\missing.pdf"), p("C:\\a.pdf")],
        );
        assert_eq!(
            a.files[0].error.as_ref().map(|e| e.code),
            Some(domain::ErrorCode::InputNotPdf)
        );
        assert_eq!(
            a.files[1].error.as_ref().map(|e| e.code),
            Some(domain::ErrorCode::PathDenied)
        );
        assert_eq!(a.files[2].kind, Some(InputKindDto::Pdf));
        assert!(a.suggestions.is_empty());
        assert_eq!(a.files[0].name, "z.zip");
    }

    #[test]
    fn the_schema_covers_every_ui_type() {
        let json = ipc_schema_json();
        for name in [
            "DocumentReport",
            "Thumbnail",
            "RecentFile",
            "ToolDescriptor",
            "DropAnalysis",
            "BatchEvent",
            "Settings",
            "AppError",
            "OutputArtifact",
            "RunBatchRequest",
        ] {
            assert!(json.contains(&format!("\"{name}\"")), "schema lacks {name}");
        }
    }
}
