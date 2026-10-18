//! Helpers shared by the tool implementations: the bundle of ports they use, output publishing,
//! scratch files and progress.

use domain::ErrorCode;
use domain::PageIndex;
use domain::tools::common::{Region, RightAngle};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::AppError;
use crate::inspect::PdfiumFacts;
use crate::ports::{
    AssemblyPlan, DocumentInfo, FileStore, FormFlattener, ImageConverter, OutlineItem,
    OutlineReader, PageSize, PdfEdit, PdfEditor, PdfReader, PdfVerifier, PdfWriter, TextExtractor,
    TocBuilder, TocLine,
};
use crate::runtime::{Password, ProgressEvent};
use crate::tool::{InputDoc, JobContext, OutputArtifact};

/// The ports the Organize tools are built on. Cloning is cheap (shared handles).
#[derive(Clone)]
pub struct ToolDeps {
    /// Page counts and sizes.
    pub reader: Arc<dyn PdfReader>,
    /// Page copying, rotation and assembly.
    pub writer: Arc<dyn PdfWriter>,
    /// Outline, crop and scale edits.
    pub editor: Arc<dyn PdfEditor>,
    /// Table of contents pages.
    pub toc: Arc<dyn TocBuilder>,
    /// Image to PDF conversion.
    pub images: Arc<dyn ImageConverter>,
    /// Bookmark reading.
    pub outlines: Arc<dyn OutlineReader>,
    /// Text extraction.
    pub text: Arc<dyn TextExtractor>,
    /// Form flattening.
    pub flattener: Arc<dyn FormFlattener>,
    /// Output verification.
    pub verifier: Arc<dyn PdfVerifier>,
}

impl ToolDeps {
    /// Ports that report `QW-COMPONENT-MISSING` for everything. Used where only the tool
    /// descriptions are needed, such as `qw tools list`.
    pub fn unavailable() -> Self {
        let none = Arc::new(Unavailable);
        Self {
            reader: none.clone(),
            writer: none.clone(),
            editor: none.clone(),
            toc: none.clone(),
            images: none.clone(),
            outlines: none.clone(),
            text: none.clone(),
            flattener: none.clone(),
            verifier: none,
        }
    }
}

struct Unavailable;

fn missing<T>() -> Result<T, AppError> {
    Err(AppError::new(
        ErrorCode::ComponentMissing,
        "The PDF engine is not available here.",
    ))
}

impl PdfReader for Unavailable {
    fn open_info(&self, _: &Path, _: Option<&Password>) -> Result<DocumentInfo, AppError> {
        missing()
    }
    fn page_sizes(&self, _: &Path, _: Option<&Password>) -> Result<Vec<PageSize>, AppError> {
        missing()
    }
}

impl PdfWriter for Unavailable {
    fn copy_pages(
        &self,
        _: &Path,
        _: Option<&Password>,
        _: &[PageIndex],
        _: &Path,
    ) -> Result<(), AppError> {
        missing()
    }
    fn rotate(
        &self,
        _: &Path,
        _: Option<&Password>,
        _: &[(PageIndex, RightAngle)],
        _: &Path,
    ) -> Result<(), AppError> {
        missing()
    }
    fn assemble(&self, _: &AssemblyPlan, _: &Path, _: &Path) -> Result<(), AppError> {
        missing()
    }
}

impl PdfEditor for Unavailable {
    fn apply(&self, _: &Path, _: &PdfEdit, _: &Path) -> Result<(), AppError> {
        missing()
    }
}

impl TocBuilder for Unavailable {
    fn page_count(&self, _: usize) -> u32 {
        1
    }
    fn build(&self, _: &[TocLine], _: f32, _: f32, _: &Path) -> Result<(), AppError> {
        missing()
    }
}

impl ImageConverter for Unavailable {
    fn image_to_pdf(&self, _: &Path, _: &Path) -> Result<(), AppError> {
        missing()
    }
}

impl OutlineReader for Unavailable {
    fn outline(&self, _: &Path, _: Option<&Password>) -> Result<Vec<OutlineItem>, AppError> {
        missing()
    }
}

impl TextExtractor for Unavailable {
    fn page_text(&self, _: &Path, _: Option<&Password>, _: PageIndex) -> Result<String, AppError> {
        missing()
    }
    fn text_in_region(
        &self,
        _: &Path,
        _: Option<&Password>,
        _: PageIndex,
        _: &Region,
    ) -> Result<String, AppError> {
        missing()
    }
}

impl FormFlattener for Unavailable {
    fn flatten_forms(&self, _: &Path, _: Option<&Password>, _: &Path) -> Result<(), AppError> {
        missing()
    }
}

impl PdfVerifier for Unavailable {
    fn verify(&self, _: &Path, _: Option<&Password>, _: Option<u32>) -> Result<(), AppError> {
        missing()
    }
}

impl crate::ports::PdfFacts for Unavailable {
    fn facts(&self, _: &Path, _: Option<&Password>) -> Result<PdfiumFacts, AppError> {
        missing()
    }
}

/// The error for options that belong to a different tool. `RunTool` prevents it, so reaching
/// this means a programming mistake rather than bad user input.
pub fn wrong_options() -> AppError {
    AppError::internal("reading the tool options")
}

/// The page count recorded by input validation.
pub fn page_count(input: &InputDoc) -> u32 {
    input.page_count.unwrap_or(0)
}

/// The first input, which single-file tools run on.
pub fn first_input(inputs: &[InputDoc]) -> Result<&InputDoc, AppError> {
    inputs
        .first()
        .ok_or_else(|| AppError::internal("finding the input file"))
}

/// A short phrase for naming an output after its pages: `page 5`, `pages 1-3` or
/// `selected pages` when they are not one run.
pub fn pages_label(pages: &[PageIndex]) -> String {
    let numbers: Vec<u32> = pages.iter().map(|p| p.get() + 1).collect();
    match numbers.as_slice() {
        [] => "no pages".to_owned(),
        [one] => format!("page {one}"),
        [first, .., last] if numbers.windows(2).all(|w| w[1] == w[0] + 1) => {
            format!("pages {first}-{last}")
        }
        _ => "selected pages".to_owned(),
    }
}

/// Keeps count of finished work and reports it so progress bars stay determinate.
pub struct Progress<'a> {
    ctx: &'a JobContext<'a>,
    done: u64,
    total: u64,
}

impl<'a> Progress<'a> {
    /// Starts at zero out of `total` units and reports it.
    pub fn start(ctx: &'a JobContext<'a>, total: u64) -> Self {
        let progress = Self {
            ctx,
            done: 0,
            total,
        };
        progress.report();
        progress
    }

    /// Records `units` finished units.
    pub fn advance(&mut self, units: u64) {
        self.done = (self.done + units).min(self.total);
        self.report();
    }

    fn report(&self) {
        self.ctx.progress.report(ProgressEvent {
            done: self.done,
            total: self.total,
        });
    }
}

static SCRATCH_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A scratch file in the session temp folder, removed when dropped.
pub struct Scratch<'a> {
    path: PathBuf,
    files: &'a dyn FileStore,
}

impl<'a> Scratch<'a> {
    /// Reserves a unique scratch path with the given extension. The file is not created.
    pub fn new(ctx: &'a JobContext<'a>, extension: &str) -> Self {
        let n = SCRATCH_COUNTER.fetch_add(1, Ordering::SeqCst);
        Self {
            path: ctx
                .temp_dir
                .join(format!("qw-{}-{n}.{extension}", std::process::id())),
            files: ctx.files,
        }
    }

    /// The scratch path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch<'_> {
    fn drop(&mut self) {
        self.files.remove_file(&self.path);
    }
}

/// Publishes one PDF output: picks a free name, lets `produce` write the temp file, verifies it
/// (reopen, `qpdf --check`, page count) and renames it into place without overwriting anything.
pub fn publish_pdf(
    ctx: &JobContext<'_>,
    deps: &ToolDeps,
    input: &InputDoc,
    label: &str,
    expected_pages: u32,
    produce: &mut dyn FnMut(&Path) -> Result<(), AppError>,
) -> Result<OutputArtifact, AppError> {
    ctx.cancel.check()?;
    let dest = ctx.output_path(input, label, "pdf", |_| {})?;
    let path = ctx.files.publish(&dest, produce, &|tmp| {
        deps.verifier.verify(tmp, None, Some(expected_pages))
    })?;
    let bytes = ctx.files.len(&path)?;
    Ok(OutputArtifact {
        path,
        pages: Some(expected_pages),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages(v: &[u32]) -> Vec<PageIndex> {
        v.iter().map(|p| PageIndex::new(*p)).collect()
    }

    #[test]
    fn labels_describe_runs_and_singles() {
        assert_eq!(pages_label(&pages(&[4])), "page 5");
        assert_eq!(pages_label(&pages(&[0, 1, 2])), "pages 1-3");
        assert_eq!(pages_label(&pages(&[0, 2])), "selected pages");
        assert_eq!(pages_label(&[]), "no pages");
    }

    #[test]
    fn unavailable_ports_report_a_missing_component() {
        let deps = ToolDeps::unavailable();
        let err = deps.reader.open_info(Path::new("a.pdf"), None).unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentMissing);
    }
}
