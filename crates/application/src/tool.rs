//! The tool contract (MASTER_PROMPT.md section 4) and the types it exchanges.

use domain::naming::{NameContext, OutputNamingPattern};
use domain::{ToolId, ToolOptions};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::naming_service::NamingService;
use crate::ports::{Clock, FileStore, Logger};
use crate::runtime::{CancellationToken, Password, ProgressSink};

/// Kinds of input a tool can accept, detected from magic bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputKind {
    /// A PDF document.
    Pdf,
    /// A raster image (JPEG, PNG, GIF, BMP, TIFF, WebP, HEIC).
    Image,
}

/// How a batch maps onto tool runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum InputMode {
    /// Each file runs on its own, with its own status (compress, rotate, split).
    PerFile,
    /// All files are combined into one run (merge, alternate and mix).
    AllTogether,
}

/// One input file for a tool run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputDoc {
    /// Canonical path of the file.
    pub path: PathBuf,
    /// Password for this file, held in memory only.
    pub password: Option<Password>,
    /// Page count, filled in by validation for PDFs.
    pub page_count: Option<u32>,
}

impl InputDoc {
    /// Creates an input that has not been inspected yet.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            password: None,
            page_count: None,
        }
    }

    /// The file name without extension, used for the `{filename}` token.
    pub fn stem(&self) -> String {
        self.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// The validated work a tool will do. Built by [`Tool::validate`], consumed by [`Tool::execute`].
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Which tool.
    pub tool: ToolId,
    /// Inspected inputs.
    pub inputs: Vec<InputDoc>,
    /// Validated options.
    pub options: ToolOptions,
    /// Total progress units (usually pages), so progress is determinate.
    pub total_units: u64,
}

/// A finished, verified output file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OutputArtifact {
    /// Where the output was written.
    pub path: PathBuf,
    /// Page count of the output, when it is a PDF.
    pub pages: Option<u32>,
    /// Size in bytes.
    pub bytes: u64,
}

/// Validation failures use the same boundary error as everything else.
pub type ValidationError = AppError;
/// Execution failures use the same boundary error as everything else.
pub type ToolError = AppError;

/// Everything a running tool may use. Borrowed for the duration of one run.
pub struct JobContext<'a> {
    /// Cancellation token, checked once per page.
    pub cancel: &'a CancellationToken,
    /// Progress sink for determinate progress.
    pub progress: &'a dyn ProgressSink,
    /// Scratch directory for this session.
    pub temp_dir: &'a Path,
    /// Naming service shared by the job.
    pub naming: &'a NamingService,
    /// Logger.
    pub logger: &'a dyn Logger,
    /// Atomic file store.
    pub files: &'a dyn FileStore,
    /// Clock for `{date}` and `{time}`.
    pub clock: &'a dyn Clock,
    /// Folder outputs are written to.
    pub output_dir: &'a Path,
    /// Naming pattern chosen for this run.
    pub pattern: &'a OutputNamingPattern,
}

impl JobContext<'_> {
    /// Picks and reserves a free output path for `input`, using the tool label and pattern.
    ///
    /// `fill` can set `page`, `pages`, `bookmark`, `text` or `sequence` on the context.
    pub fn output_path(
        &self,
        input: &InputDoc,
        tool_label: &str,
        extension: &str,
        fill: impl FnOnce(&mut NameContext<'_>),
    ) -> Result<PathBuf, AppError> {
        let stem = input.stem();
        let mut ctx = NameContext {
            filename: &stem,
            tool: tool_label,
            now: self.clock.now(),
            sequence: None,
            page: None,
            pages: None,
            bookmark: None,
            text: None,
        };
        fill(&mut ctx);
        self.naming
            .output_path(self.output_dir, self.pattern, &ctx, extension)
    }
}

/// A PDF tool. Registered once in the [`ToolRegistry`](crate::registry::ToolRegistry).
pub trait Tool: Send + Sync {
    /// Stable identifier.
    fn id(&self) -> ToolId;

    /// Navigation group.
    fn group(&self) -> domain::tools::ToolGroup;

    /// Short label used for the `{tool}` naming token, for example `rotated`.
    fn label(&self) -> &'static str;

    /// Input kinds this tool accepts.
    fn accepts(&self) -> &'static [InputKind];

    /// How a batch maps onto runs.
    fn input_mode(&self) -> InputMode {
        InputMode::PerFile
    }

    /// JSON Schema of the options, used to generate the option form.
    fn options_schema(&self) -> schemars::Schema;

    /// Checks inputs and options and returns the plan. No output is written here.
    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, ValidationError>;

    /// Runs the plan. Must check cancellation per page and report progress.
    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, ToolError>;
}
