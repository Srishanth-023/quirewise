//! The `RunTool` use case: validate inputs, build the plan, execute, return verified outputs.
//!
//! `RunTool` runs inside the worker process (it opens untrusted PDFs). The app process talks to
//! it through the [`UnitRunner`] port, so tests and the CLI can run it in-process.

use domain::ToolId;
use domain::ToolOptions;
use domain::limits::SafetyLimits;
use domain::naming::OutputNamingPattern;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::AppError;
use crate::input::validate_input;
use crate::naming_service::NamingService;
use crate::ports::{Clock, FileStore, LogLevel, Logger, PdfReader};
use crate::registry::ToolRegistry;
use crate::runtime::{CancellationToken, ProgressSink};
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact};

/// One unit of work: one tool run over one file, or over all files for merge-style tools.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitRequest {
    /// Tool to run.
    pub tool: ToolId,
    /// Options for the tool. Their tag must match `tool`.
    pub options: ToolOptions,
    /// Input files, with optional passwords.
    pub inputs: Vec<InputDoc>,
    /// Output folder. `None` means next to the first input.
    pub output_dir: Option<PathBuf>,
    /// Naming pattern for outputs.
    pub naming_pattern: OutputNamingPattern,
}

/// Runs one unit of work. Implemented in-process by [`RunTool`] and over IPC by the supervisor.
pub trait UnitRunner: Send + Sync {
    /// Runs the unit and returns its verified outputs.
    fn run_unit(
        &self,
        request: &UnitRequest,
        cancel: &CancellationToken,
        progress: &dyn ProgressSink,
    ) -> Result<Vec<OutputArtifact>, AppError>;
}

/// In-process implementation of [`UnitRunner`] over the tool registry and ports.
pub struct RunTool {
    registry: Arc<ToolRegistry>,
    reader: Arc<dyn PdfReader>,
    files: Arc<dyn FileStore>,
    clock: Arc<dyn Clock>,
    logger: Arc<dyn Logger>,
    limits: SafetyLimits,
    temp_dir: PathBuf,
}

impl RunTool {
    /// Wires the use case to its ports.
    pub fn new(
        registry: Arc<ToolRegistry>,
        reader: Arc<dyn PdfReader>,
        files: Arc<dyn FileStore>,
        clock: Arc<dyn Clock>,
        logger: Arc<dyn Logger>,
        limits: SafetyLimits,
        temp_dir: PathBuf,
    ) -> Self {
        Self {
            registry,
            reader,
            files,
            clock,
            logger,
            limits,
            temp_dir,
        }
    }

    fn inspect_inputs(
        &self,
        request: &UnitRequest,
        accepts: &[InputKind],
        open_first: bool,
    ) -> Result<Vec<InputDoc>, AppError> {
        let mut inspected = Vec::with_capacity(request.inputs.len());
        for input in &request.inputs {
            let (mut doc, kind) =
                validate_input(self.files.as_ref(), &input.path, accepts, &self.limits)?;
            doc.password = input.password.clone();
            if kind == InputKind::Pdf && open_first {
                let info = self.reader.open_info(&doc.path, doc.password.as_ref())?;
                if info.page_count > self.limits.max_pages {
                    return Err(AppError::new(
                        domain::ErrorCode::PdfLimitExceeded,
                        format!(
                            "This document has {} pages, more than the safety limit of {}. Split it first or raise the limit in Settings.",
                            info.page_count, self.limits.max_pages
                        ),
                    ));
                }
                doc.page_count = Some(info.page_count);
            }
            inspected.push(doc);
        }
        Ok(inspected)
    }

    fn output_dir(request: &UnitRequest, inputs: &[InputDoc]) -> Result<PathBuf, AppError> {
        if let Some(dir) = &request.output_dir {
            return Ok(dir.clone());
        }
        inputs
            .first()
            .and_then(|i| i.path.parent())
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                domain::DomainError::option("output folder", "choose where the result goes").into()
            })
    }
}

impl UnitRunner for RunTool {
    fn run_unit(
        &self,
        request: &UnitRequest,
        cancel: &CancellationToken,
        progress: &dyn ProgressSink,
    ) -> Result<Vec<OutputArtifact>, AppError> {
        let tool = self
            .registry
            .get(request.tool)
            .ok_or_else(|| AppError::internal(&format!("finding the {} tool", request.tool)))?;
        if request.options.tool_id() != request.tool {
            return Err(
                domain::DomainError::option("options", "they belong to a different tool").into(),
            );
        }
        if request.inputs.is_empty() && !tool.runs_without_files(&request.options) {
            return Err(domain::DomainError::option("files", "add at least one file").into());
        }
        cancel.check()?;

        let inputs = self.inspect_inputs(request, tool.accepts(), tool.reads_pages_first())?;
        let plan = tool.validate(&inputs, &request.options)?;
        cancel.check()?;

        let output_dir = Self::output_dir(request, &inputs)?;
        let naming = NamingService::new(Arc::clone(&self.files));
        let ctx = JobContext {
            cancel,
            progress,
            temp_dir: &self.temp_dir,
            naming: &naming,
            logger: self.logger.as_ref(),
            files: self.files.as_ref(),
            clock: self.clock.as_ref(),
            output_dir: &output_dir,
            pattern: &request.naming_pattern,
        };
        self.logger.log(LogLevel::Info, "tool.started");
        let result = tool.execute(&ctx, plan);
        self.logger.log(
            if result.is_ok() {
                LogLevel::Info
            } else {
                LogLevel::Warn
            },
            if result.is_ok() {
                "tool.finished"
            } else {
                "tool.failed"
            },
        );
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{NoProgress, Password};
    use crate::testing::{FixedClock, MemoryFiles, NullLogger, RotateLikeTool, StubReader};
    use crate::tool::{Plan, Tool};
    use domain::tools::RotateOptions;
    use domain::{ErrorCode, PageRange};

    struct Fixture {
        files: Arc<MemoryFiles>,
        run: RunTool,
    }

    fn fixture() -> Fixture {
        let files = Arc::new(MemoryFiles::default());
        files.put("C:\\in\\a.pdf", b"%PDF-1.7 ten pages");
        files.put("C:\\in\\secret.pdf", b"%PDF-1.7 locked");
        files.put("C:\\in\\notes.zip", b"PK\x03\x04");
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(RotateLikeTool)).unwrap();
        let reader = StubReader::default().with("C:\\in\\a.pdf", 10).with_locked(
            "C:\\in\\secret.pdf",
            "pw",
            3,
        );
        let run = RunTool::new(
            Arc::new(registry),
            Arc::new(reader),
            files.clone(),
            Arc::new(FixedClock),
            Arc::new(NullLogger),
            SafetyLimits::default(),
            PathBuf::from("C:\\tmp"),
        );
        Fixture { files, run }
    }

    fn request(path: &str, options: RotateOptions) -> UnitRequest {
        UnitRequest {
            tool: ToolId::Rotate,
            options: ToolOptions::Rotate(options),
            inputs: vec![InputDoc::new(path)],
            output_dir: None,
            naming_pattern: OutputNamingPattern::default(),
        }
    }

    /// A tool that must see files the PDF reader cannot open, like Repair.
    struct BlindTool {
        reads_first: bool,
    }

    impl Tool for BlindTool {
        fn id(&self) -> ToolId {
            ToolId::Repair
        }
        fn group(&self) -> domain::tools::ToolGroup {
            domain::tools::ToolGroup::Optimize
        }
        fn label(&self) -> &'static str {
            "repaired"
        }
        fn accepts(&self) -> &'static [InputKind] {
            &[InputKind::Pdf]
        }
        fn options_schema(&self) -> schemars::Schema {
            schemars::schema_for!(domain::tools::RepairOptions)
        }
        fn reads_pages_first(&self) -> bool {
            self.reads_first
        }
        fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
            Ok(Plan {
                tool: ToolId::Repair,
                inputs: inputs.to_vec(),
                options: opts.clone(),
                total_units: 1,
            })
        }
        fn execute(&self, _: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
            assert_eq!(plan.inputs[0].page_count, None, "no page count was read");
            Ok(vec![OutputArtifact {
                path: PathBuf::from("C:\\in\\fixed.pdf"),
                pages: None,
                bytes: 0,
            }])
        }
    }

    fn blind_run(reads_first: bool) -> Result<Vec<OutputArtifact>, AppError> {
        let files = Arc::new(MemoryFiles::default());
        files.put("C:\\in\\broken.pdf", b"%PDF-1.4 broken");
        let mut registry = ToolRegistry::new();
        registry
            .register(Arc::new(BlindTool { reads_first }))
            .unwrap();
        // The reader knows no documents, so opening anything fails as damaged.
        let run = RunTool::new(
            Arc::new(registry),
            Arc::new(StubReader::default()),
            files,
            Arc::new(FixedClock),
            Arc::new(NullLogger),
            SafetyLimits::default(),
            PathBuf::from("C:\\tmp"),
        );
        run.run_unit(
            &UnitRequest {
                tool: ToolId::Repair,
                options: ToolOptions::defaults_for(ToolId::Repair),
                inputs: vec![InputDoc::new("C:\\in\\broken.pdf")],
                output_dir: None,
                naming_pattern: OutputNamingPattern::default(),
            },
            &CancellationToken::new(),
            &NoProgress,
        )
    }

    #[test]
    fn a_tool_can_opt_out_of_opening_the_file_first() {
        assert!(blind_run(false).is_ok());
    }

    #[test]
    fn other_tools_still_get_a_damaged_file_rejected_up_front() {
        assert_eq!(blind_run(true).unwrap_err().code, ErrorCode::PdfDamaged);
    }

    #[test]
    fn runs_a_tool_and_writes_next_to_the_source_without_touching_it() {
        let f = fixture();
        let before = f.files.get("C:\\in\\a.pdf");
        let out = f
            .run
            .run_unit(
                &request("C:\\in\\a.pdf", RotateOptions::default()),
                &CancellationToken::new(),
                &NoProgress,
            )
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, Path::new("C:\\in\\a (rotated).pdf"));
        assert_eq!(out[0].pages, Some(10));
        assert!(f.files.exists(&out[0].path));
        assert_eq!(
            f.files.get("C:\\in\\a.pdf"),
            before,
            "input must be untouched"
        );
    }

    #[test]
    fn second_run_gets_a_non_colliding_name() {
        let f = fixture();
        let req = request("C:\\in\\a.pdf", RotateOptions::default());
        let token = CancellationToken::new();
        let first = f.run.run_unit(&req, &token, &NoProgress).unwrap();
        let second = f.run.run_unit(&req, &token, &NoProgress).unwrap();
        assert_eq!(first[0].path, Path::new("C:\\in\\a (rotated).pdf"));
        assert_eq!(second[0].path, Path::new("C:\\in\\a (rotated) (2).pdf"));
    }

    #[test]
    fn invalid_inputs_map_to_catalog_codes() {
        let f = fixture();
        let token = CancellationToken::new();
        let opts = RotateOptions::default();

        let zip = f.run.run_unit(
            &request("C:\\in\\notes.zip", opts.clone()),
            &token,
            &NoProgress,
        );
        assert_eq!(zip.unwrap_err().code, ErrorCode::InputNotPdf);

        let locked = f.run.run_unit(
            &request("C:\\in\\secret.pdf", opts.clone()),
            &token,
            &NoProgress,
        );
        assert_eq!(locked.unwrap_err().code, ErrorCode::PdfEncrypted);

        let mut with_pw = request("C:\\in\\secret.pdf", opts.clone());
        with_pw.inputs[0].password = Some(Password::new("pw"));
        assert!(f.run.run_unit(&with_pw, &token, &NoProgress).is_ok());

        let mut wrong_pw = request("C:\\in\\secret.pdf", opts.clone());
        wrong_pw.inputs[0].password = Some(Password::new("nope"));
        assert_eq!(
            f.run
                .run_unit(&wrong_pw, &token, &NoProgress)
                .unwrap_err()
                .code,
            ErrorCode::PdfWrongPassword
        );

        let out_of_range = RotateOptions {
            pages: PageRange::parse("11").unwrap(),
            ..RotateOptions::default()
        };
        let oob = f
            .run
            .run_unit(&request("C:\\in\\a.pdf", out_of_range), &token, &NoProgress);
        assert_eq!(oob.unwrap_err().code, ErrorCode::RangeOutOfBounds);

        let mut empty = request("C:\\in\\a.pdf", opts);
        empty.inputs.clear();
        assert_eq!(
            f.run
                .run_unit(&empty, &token, &NoProgress)
                .unwrap_err()
                .code,
            ErrorCode::OptionInvalid
        );
    }

    #[test]
    fn cancellation_stops_before_any_output_is_written() {
        let f = fixture();
        let token = CancellationToken::new();
        token.cancel();
        let err = f
            .run
            .run_unit(
                &request("C:\\in\\a.pdf", RotateOptions::default()),
                &token,
                &NoProgress,
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::JobCancelled);
        assert!(!f.files.exists(Path::new("C:\\in\\a (rotated).pdf")));
    }

    #[test]
    fn page_limit_is_enforced() {
        let files = Arc::new(MemoryFiles::default());
        files.put("C:\\in\\huge.pdf", b"%PDF-1.7");
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(RotateLikeTool)).unwrap();
        let limits = SafetyLimits {
            max_pages: 5,
            ..SafetyLimits::default()
        };
        let run = RunTool::new(
            Arc::new(registry),
            Arc::new(StubReader::default().with("C:\\in\\huge.pdf", 6)),
            files,
            Arc::new(FixedClock),
            Arc::new(NullLogger),
            limits,
            PathBuf::from("C:\\tmp"),
        );
        let err = run
            .run_unit(
                &request("C:\\in\\huge.pdf", RotateOptions::default()),
                &CancellationToken::new(),
                &NoProgress,
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::PdfLimitExceeded);
    }
}
