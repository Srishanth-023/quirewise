//! In-memory fakes for unit tests. Compiled only for tests.

use domain::naming::DateTimeParts;
use domain::tools::RotateOptions;
use domain::{ErrorCode, ToolId, ToolOptions};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::error::AppError;
use crate::ports::{Clock, DocumentInfo, FileStore, LogLevel, Logger, PdfReader};
use crate::runtime::Password;
use crate::tool::{
    InputDoc, InputKind, JobContext, OutputArtifact, Plan, PlannedOutput, Tool, ToolError,
    ValidationError,
};

/// A file store backed by a map. Paths are used verbatim.
#[derive(Default)]
pub struct MemoryFiles {
    files: Mutex<HashMap<PathBuf, Vec<u8>>>,
}

impl MemoryFiles {
    pub fn put(&self, path: &str, bytes: &[u8]) {
        self.lock().insert(PathBuf::from(path), bytes.to_vec());
    }

    pub fn remove(&self, path: &str) {
        self.lock().remove(Path::new(path));
    }

    pub fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.lock().get(Path::new(path)).cloned()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Vec<u8>>> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl FileStore for MemoryFiles {
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, AppError> {
        if self.exists(path) {
            Ok(path.to_path_buf())
        } else {
            Err(AppError::from_io(
                "open the file",
                &io::Error::from(io::ErrorKind::NotFound),
            ))
        }
    }

    fn exists(&self, path: &Path) -> bool {
        self.lock().contains_key(path)
    }

    fn len(&self, path: &Path) -> Result<u64, AppError> {
        self.lock()
            .get(path)
            .map(|b| b.len() as u64)
            .ok_or_else(|| {
                AppError::from_io("read the file", &io::Error::from(io::ErrorKind::NotFound))
            })
    }

    fn read_head(&self, path: &Path, max: usize) -> Result<Vec<u8>, AppError> {
        self.lock()
            .get(path)
            .map(|b| b.iter().take(max).copied().collect())
            .ok_or_else(|| {
                AppError::from_io("read the file", &io::Error::from(io::ErrorKind::NotFound))
            })
    }

    fn write_atomic(
        &self,
        dest: &Path,
        write: &mut dyn FnMut(&mut dyn Write) -> io::Result<()>,
        verify: &dyn Fn(&Path) -> Result<(), AppError>,
    ) -> Result<PathBuf, AppError> {
        if self.exists(dest) {
            return Err(AppError::new(
                ErrorCode::PathDenied,
                "The output already exists.",
            ));
        }
        let mut buf = Vec::new();
        write(&mut buf).map_err(|e| AppError::from_io("write the output", &e))?;
        verify(dest)?;
        self.lock().insert(dest.to_path_buf(), buf);
        Ok(dest.to_path_buf())
    }
}

/// A clock fixed at 2026-10-18 09:05:07.
pub struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> DateTimeParts {
        DateTimeParts {
            year: 2026,
            month: 10,
            day: 18,
            hour: 9,
            minute: 5,
            second: 7,
        }
    }
}

/// A logger that discards events.
pub struct NullLogger;

impl Logger for NullLogger {
    fn log(&self, _level: LogLevel, _event: &str) {}
}

/// A PDF reader with canned answers.
#[derive(Default)]
pub struct StubReader {
    docs: HashMap<PathBuf, (u32, Option<String>)>,
}

impl StubReader {
    pub fn with(mut self, path: &str, pages: u32) -> Self {
        self.docs.insert(PathBuf::from(path), (pages, None));
        self
    }

    pub fn with_locked(mut self, path: &str, password: &str, pages: u32) -> Self {
        self.docs
            .insert(PathBuf::from(path), (pages, Some(password.to_owned())));
        self
    }
}

impl PdfReader for StubReader {
    fn open_info(
        &self,
        path: &Path,
        password: Option<&Password>,
    ) -> Result<DocumentInfo, AppError> {
        let (pages, required) = self
            .docs
            .get(path)
            .ok_or_else(|| AppError::new(ErrorCode::PdfDamaged, "damaged"))?;
        match (required, password) {
            (None, _) => Ok(DocumentInfo {
                page_count: *pages,
                encrypted: false,
            }),
            (Some(_), None) => Err(AppError::new(ErrorCode::PdfEncrypted, "password needed")),
            (Some(want), Some(got)) if got.expose() == want => Ok(DocumentInfo {
                page_count: *pages,
                encrypted: true,
            }),
            (Some(_), Some(_)) => Err(AppError::new(ErrorCode::PdfWrongPassword, "wrong password")),
        }
    }
}

/// A rotate-shaped tool over fake output, used to exercise the use cases.
#[derive(Default)]
pub struct RotateLikeTool;

impl Tool for RotateLikeTool {
    fn id(&self) -> ToolId {
        ToolId::Rotate
    }

    fn group(&self) -> domain::tools::ToolGroup {
        domain::tools::ToolGroup::Edit
    }

    fn label(&self) -> &'static str {
        "rotated"
    }

    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }

    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(RotateOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, ValidationError> {
        let ToolOptions::Rotate(rotate) = opts;
        let input = inputs
            .first()
            .ok_or_else(|| AppError::internal("validating the rotate plan"))?;
        let count = input.page_count.unwrap_or(0);
        let selection = rotate.select_pages(count)?;
        Ok(Plan {
            tool: ToolId::Rotate,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            outputs: vec![PlannedOutput {
                input_index: 0,
                pages: Some(selection),
                extension: "pdf".to_owned(),
            }],
            total_units: u64::from(count),
        })
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, ToolError> {
        let mut artifacts = Vec::new();
        for planned in &plan.outputs {
            let input = &plan.inputs[planned.input_index];
            let pages = planned.pages.as_ref().map_or(0, |p| p.len());
            for done in 0..pages {
                ctx.cancel.check()?;
                ctx.progress.report(crate::runtime::ProgressEvent {
                    done: done as u64,
                    total: pages as u64,
                });
            }
            let path = ctx.output_path(input, self.label(), &planned.extension, |_| {})?;
            let page_count = input.page_count.unwrap_or(0);
            let written = ctx.files.write_atomic(
                &path,
                &mut |w| w.write_all(b"%PDF-1.7 rotated"),
                &|_| Ok(()),
            )?;
            artifacts.push(OutputArtifact {
                path: written,
                pages: Some(page_count),
                bytes: 16,
            });
        }
        Ok(artifacts)
    }
}
