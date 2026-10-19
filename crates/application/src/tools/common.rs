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
    AssemblyPlan, AttachmentReader, DocumentInfo, ExtractReport, ExtractedImage, FileStore,
    FormFlattener, HeadingFinder, HtmlConverter, HtmlSource, ImageConverter, ImageExtractor,
    ImageOptimizer, ImageReport, OcrEngine, OfficeConverter, OfficeEngines, OfficeKind,
    OfficeRequest, OutlineItem, OutlineReader, PackageOptions, PageImage, PageImageWriter,
    PageSize, PdfEdit, PdfEditor, PdfPackager, PdfReader, PdfRepairer, PdfSecurity, PdfVerifier,
    PdfWriter, PictureSpec, PictureWriter, PngImage, ProtectionSource, RenderedImage, Renderer,
    RepairOutcome, ScanPageWriter, SkewDetector, TextExtractor, TiffPages, TiffSpec, TocBuilder,
    TocLine,
};
use crate::runtime::{Password, ProgressEvent};
use crate::tool::{InputDoc, JobContext, OutputArtifact};
use domain::Dpi;
use domain::tools::convert::{ImagesToPdfOptions, PictureFormat};
use domain::tools::optimize::CompressSettings;
use domain::tools::secure::ProtectPlan;

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
    /// Page rendering.
    pub renderer: Arc<dyn Renderer>,
    /// Encryption and decryption.
    pub security: Arc<dyn PdfSecurity>,
    /// Stream compression, object streams and linearization.
    pub packager: Arc<dyn PdfPackager>,
    /// Ways to repair a damaged file, gentlest first.
    pub repairers: Vec<Arc<dyn PdfRepairer>>,
    /// Image downsampling and recompression.
    pub optimizer: Arc<dyn ImageOptimizer>,
    /// Documents made of page pictures.
    pub page_images: Arc<dyn PageImageWriter>,
    /// Attached files.
    pub attachments: Arc<dyn AttachmentReader>,
    /// Heading detection.
    pub headings: Arc<dyn HeadingFinder>,
    /// Page pictures and TIFF files.
    pub pictures: Arc<dyn PictureWriter>,
    /// Pictures taken out of documents.
    pub extractor: Arc<dyn ImageExtractor>,
    /// Office documents to PDF.
    pub office: OfficeEngines,
    /// Web pages to PDF.
    pub html: Arc<dyn HtmlConverter>,
    /// Text recognition.
    pub ocr: Arc<dyn OcrEngine>,
    /// Tilt of scanned pages.
    pub skew: Arc<dyn SkewDetector>,
    /// Pictures of pages to PDF.
    pub scans: Arc<dyn ScanPageWriter>,
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
            verifier: none.clone(),
            renderer: none.clone(),
            security: none.clone(),
            packager: none.clone(),
            repairers: vec![none.clone()],
            optimizer: none.clone(),
            page_images: none.clone(),
            attachments: none.clone(),
            headings: none.clone(),
            pictures: none.clone(),
            extractor: none.clone(),
            office: OfficeEngines {
                microsoft: none.clone(),
                libre: none.clone(),
            },
            html: none.clone(),
            ocr: none.clone(),
            skew: none.clone(),
            scans: none,
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
    fn images_to_pdf(
        &self,
        _: &[PathBuf],
        _: &ImagesToPdfOptions,
        _: &Path,
        _: &Path,
    ) -> Result<(), AppError> {
        missing()
    }
    fn tiff_to_pdf(&self, _: &Path, _: &Path) -> Result<u32, AppError> {
        missing()
    }
}

impl PictureWriter for Unavailable {
    fn write_picture(&self, _: &RenderedImage, _: &PictureSpec, _: &Path) -> Result<(), AppError> {
        missing()
    }
    fn start_tiff(&self, _: &Path, _: &TiffSpec) -> Result<Box<dyn TiffPages>, AppError> {
        missing()
    }
    fn check_picture(&self, _: &Path, _: PictureFormat, _: u32) -> Result<(), AppError> {
        missing()
    }
}

impl OfficeConverter for Unavailable {
    fn name(&self) -> &'static str {
        "Office"
    }
    fn is_available(&self, _: OfficeKind) -> bool {
        false
    }
    fn convert(
        &self,
        _: &Path,
        _: &OfficeRequest,
        _: &Path,
        _: &Path,
        _: &crate::runtime::CancellationToken,
    ) -> Result<(), AppError> {
        missing()
    }
}

impl OcrEngine for Unavailable {
    fn is_available(&self) -> bool {
        false
    }
    fn languages(&self) -> Result<Vec<String>, AppError> {
        Ok(Vec::new())
    }
    fn recognize(
        &self,
        _: &RenderedImage,
        _: &[String],
        _: Dpi,
        _: &Path,
        _: &crate::runtime::CancellationToken,
    ) -> Result<String, AppError> {
        missing()
    }
    fn text_layer(
        &self,
        _: &RenderedImage,
        _: &[String],
        _: Dpi,
        _: &Path,
        _: &Path,
        _: &crate::runtime::CancellationToken,
    ) -> Result<(), AppError> {
        missing()
    }
}

impl SkewDetector for Unavailable {
    fn skew(&self, _: &RenderedImage, _: f32) -> Result<Option<f32>, AppError> {
        missing()
    }
    fn rotate(&self, _: &RenderedImage, _: f32) -> Result<RenderedImage, AppError> {
        missing()
    }
}

impl ScanPageWriter for Unavailable {
    fn scans_to_pdf(
        &self,
        _: &[PathBuf],
        _: &domain::tools::recognize::ScanToPdfOptions,
        _: &Path,
        _: &Path,
        _: &crate::runtime::CancellationToken,
    ) -> Result<u32, AppError> {
        missing()
    }
}

impl HtmlConverter for Unavailable {
    fn is_available(&self) -> bool {
        false
    }
    fn print(
        &self,
        _: &HtmlSource,
        _: &domain::tools::HtmlToPdfOptions,
        _: &Path,
        _: &crate::runtime::CancellationToken,
    ) -> Result<(), AppError> {
        missing()
    }
}

impl ImageExtractor for Unavailable {
    fn extract(
        &self,
        _: &Path,
        _: bool,
        _: (u32, u32),
        _: &mut dyn FnMut(ExtractedImage) -> Result<(), AppError>,
    ) -> Result<ExtractReport, AppError> {
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
    fn page_text_layout(
        &self,
        _: &Path,
        _: Option<&Password>,
        _: PageIndex,
    ) -> Result<String, AppError> {
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

impl Renderer for Unavailable {
    fn render_page(
        &self,
        _: &Path,
        _: Option<&Password>,
        _: PageIndex,
        _: Dpi,
    ) -> Result<RenderedImage, AppError> {
        missing()
    }
    fn render_png(
        &self,
        _: &Path,
        _: Option<&Password>,
        _: PageIndex,
        _: u32,
    ) -> Result<PngImage, AppError> {
        missing()
    }
}

impl PdfSecurity for Unavailable {
    fn protect(
        &self,
        _: &Path,
        _: Option<&Password>,
        _: &ProtectPlan,
        _: &Path,
    ) -> Result<(), AppError> {
        missing()
    }
    fn unlock(&self, _: &Path, _: Option<&Password>, _: &Path) -> Result<(), AppError> {
        missing()
    }
}

impl PdfPackager for Unavailable {
    fn package(
        &self,
        _: &Path,
        _: Option<&Password>,
        _: &PackageOptions,
        _: &Path,
    ) -> Result<(), AppError> {
        missing()
    }
}

impl PdfRepairer for Unavailable {
    fn name(&self) -> &'static str {
        "unavailable"
    }
    fn repair(&self, _: &Path, _: Option<&Password>, _: &Path) -> Result<RepairOutcome, AppError> {
        missing()
    }
}

impl ImageOptimizer for Unavailable {
    fn optimize_images(
        &self,
        _: &Path,
        _: &CompressSettings,
        _: &Path,
    ) -> Result<ImageReport, AppError> {
        missing()
    }
    fn grayscale_images(&self, _: &Path, _: &Path) -> Result<ImageReport, AppError> {
        missing()
    }
}

impl AttachmentReader for Unavailable {
    fn list(
        &self,
        _: &Path,
        _: Option<&Password>,
    ) -> Result<Vec<crate::ports::AttachmentInfo>, AppError> {
        missing()
    }
    fn read(
        &self,
        _: &Path,
        _: Option<&Password>,
        _: &[String],
    ) -> Result<Vec<crate::ports::Attachment>, AppError> {
        missing()
    }
}

impl HeadingFinder for Unavailable {
    fn text_lines(
        &self,
        _: &Path,
        _: Option<&Password>,
    ) -> Result<Vec<domain::tools::structure::TextLine>, AppError> {
        missing()
    }
}

impl PageImageWriter for Unavailable {
    fn pages_to_pdf(&self, _: &[PageImage], _: &Path) -> Result<(), AppError> {
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

/// The protection a result keeps: the input's own, when the input is encrypted and its password
/// is known. A file that opens without a password has nothing to keep (its restrictions, if
/// any, are dropped like those of any rewritten file).
pub fn kept_protection(
    deps: &ToolDeps,
    input: &InputDoc,
) -> Result<Option<ProtectionSource>, AppError> {
    let Some(password) = &input.password else {
        return Ok(None);
    };
    let info = deps.reader.open_info(&input.path, Some(password))?;
    Ok(info.encrypted.then(|| ProtectionSource {
        path: input.path.clone(),
        password: password.clone(),
    }))
}

/// The common path of tools that change a document's structure: unlock a scratch copy, apply
/// `edit`, then repack the result into `tmp`, protected like the input when `protection` is
/// given and unencrypted otherwise.
pub fn edit_pipeline(
    ctx: &JobContext<'_>,
    deps: &ToolDeps,
    input: &InputDoc,
    edit: &PdfEdit,
    protection: Option<&ProtectionSource>,
    tmp: &Path,
) -> Result<(), AppError> {
    let unlocked = Scratch::new(ctx, "pdf");
    deps.security
        .unlock(&input.path, input.password.as_ref(), unlocked.path())?;
    ctx.cancel.check()?;
    let edited = Scratch::new(ctx, "pdf");
    deps.editor.apply(unlocked.path(), edit, edited.path())?;
    ctx.cancel.check()?;
    deps.packager.package(
        edited.path(),
        None,
        &PackageOptions {
            keep_protection: protection.cloned(),
            ..PackageOptions::new(false, true)
        },
        tmp,
    )
}

/// Runs an edit on a copy of the input and publishes the verified result, keeping the input's
/// protection. This is the path of every tool that is "one edit of the document".
pub fn publish_edit(
    ctx: &JobContext<'_>,
    deps: &ToolDeps,
    input: &InputDoc,
    label: &str,
    expected_pages: u32,
    edit: &PdfEdit,
) -> Result<OutputArtifact, AppError> {
    let protection = kept_protection(deps, input)?;
    let password = protection.as_ref().map(|p| &p.password);
    publish_pdf_checked(
        ctx,
        deps,
        input,
        label,
        Some(expected_pages),
        password,
        &mut |tmp| edit_pipeline(ctx, deps, input, edit, protection.as_ref(), tmp),
    )
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
    publish_pdf_checked(ctx, deps, input, label, Some(expected_pages), None, produce)
}

/// Like [`publish_pdf`], for outputs that are encrypted (verified with their own password) or
/// whose page count is not known beforehand (Repair).
pub fn publish_pdf_checked(
    ctx: &JobContext<'_>,
    deps: &ToolDeps,
    input: &InputDoc,
    label: &str,
    expected_pages: Option<u32>,
    output_password: Option<&Password>,
    produce: &mut dyn FnMut(&Path) -> Result<(), AppError>,
) -> Result<OutputArtifact, AppError> {
    ctx.cancel.check()?;
    let dest = ctx.output_path(input, label, "pdf", |_| {})?;
    let path = ctx.files.publish(&dest, produce, &|tmp| {
        deps.verifier.verify(tmp, output_password, expected_pages)
    })?;
    let bytes = ctx.files.len(&path)?;
    Ok(OutputArtifact {
        path,
        pages: expected_pages,
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
