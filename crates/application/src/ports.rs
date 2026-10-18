//! Ports: traits that engines and infrastructure implement. Tools and use cases depend on these
//! and never on concrete engines.

use domain::components::ComponentEntry;
use domain::naming::DateTimeParts;
use domain::settings::Settings;
use domain::tools::common::{FractionBox, Region, RightAngle};
use domain::tools::document::{AnnotationKind, MetadataChange};
use domain::tools::optimize::CompressSettings;
use domain::tools::secure::{ProtectPlan, SanitizeOptions};
use domain::{Dpi, PageIndex};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::inspect::{PdfiumFacts, StructureReport};
use crate::runtime::Password;

/// Basic facts about an opened PDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentInfo {
    /// Number of pages.
    pub page_count: u32,
    /// True when the file is encrypted (it was opened with the supplied password).
    pub encrypted: bool,
}

/// A rendered page as tightly packed RGBA bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA bytes, `width * height * 4` long.
    pub rgba: Vec<u8>,
}

/// Result of a PDF/A or PDF/UA validation run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    /// True when the document passes the checked profile.
    pub compliant: bool,
    /// Human-readable findings.
    pub issues: Vec<String>,
}

/// Reads PDF structure. Implemented by the PDFium adapter and runs only in the worker.
pub trait PdfReader: Send + Sync {
    /// Opens a document and reports its basic facts.
    ///
    /// Errors: `QW-PDF-ENCRYPTED`, `QW-PDF-WRONG-PASSWORD`, `QW-PDF-DAMAGED`,
    /// `QW-PDF-LIMIT-EXCEEDED`.
    fn open_info(&self, path: &Path, password: Option<&Password>)
    -> Result<DocumentInfo, AppError>;

    /// Width and height in points of every page, in order, before rotation is applied.
    fn page_sizes(
        &self,
        path: &Path,
        password: Option<&Password>,
    ) -> Result<Vec<PageSize>, AppError>;
}

/// The size of one page in PDF points (1/72 inch).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSize {
    /// Width in points.
    pub width_pt: f32,
    /// Height in points.
    pub height_pt: f32,
}

/// Document facts only PDFium can give reliably (it applies the page tree, fonts and objects).
pub trait PdfFacts: Send + Sync {
    /// Reads PDFium-derived facts. Errors: encryption, damage and limit codes as for
    /// [`PdfReader::open_info`].
    fn facts(&self, path: &Path, password: Option<&Password>) -> Result<PdfiumFacts, AppError>;
}

/// Raw structure facts read without rendering (Encrypt dictionary, JavaScript, XMP, sizes).
pub trait StructureFacts: Send + Sync {
    /// Reads structure facts. Works on encrypted files without a password for the facts that
    /// are not themselves encrypted.
    fn structure(
        &self,
        path: &Path,
        password: Option<&Password>,
    ) -> Result<StructureReport, AppError>;
}

/// Checks that a written PDF is sound before success is reported (AGENTS.md section 7).
pub trait PdfVerifier: Send + Sync {
    /// Reopens `path`, runs the structural check and, when given, compares the page count.
    ///
    /// Error: `QW-OUTPUT-VERIFY-FAILED`.
    fn verify(
        &self,
        path: &Path,
        password: Option<&Password>,
        expected_pages: Option<u32>,
    ) -> Result<(), AppError>;
}

/// One input of an [`AssemblyPlan`].
#[derive(Debug, Clone)]
pub struct SourceDoc {
    /// The PDF file.
    pub path: PathBuf,
    /// Its password, when it is encrypted.
    pub password: Option<Password>,
}

/// One page of an assembled document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlannedPage {
    /// A page of one of the plan's sources, optionally turned.
    Source {
        /// Index into [`AssemblyPlan::sources`].
        source: usize,
        /// Zero-based page of that source.
        page: PageIndex,
        /// Rotation added to the page.
        rotate: RightAngle,
    },
    /// A new blank page.
    Blank {
        /// Width in points.
        width_pt: f32,
        /// Height in points.
        height_pt: f32,
    },
}

/// A new document built from pages of several sources and blank pages.
#[derive(Debug, Clone)]
pub struct AssemblyPlan {
    /// Input documents.
    pub sources: Vec<SourceDoc>,
    /// The pages of the result, in order. Pages may repeat.
    pub pages: Vec<PlannedPage>,
}

/// Writes PDF output.
pub trait PdfWriter: Send + Sync {
    /// Copies `pages` (in order) from `source` into a new, unencrypted document at `dest`.
    /// Document-level features of `source` such as outlines and forms are kept where possible.
    fn copy_pages(
        &self,
        source: &Path,
        password: Option<&Password>,
        pages: &[PageIndex],
        dest: &Path,
    ) -> Result<(), AppError>;

    /// Writes `source` to `dest` with the given pages turned further. Everything else is kept.
    fn rotate(
        &self,
        source: &Path,
        password: Option<&Password>,
        rotations: &[(PageIndex, RightAngle)],
        dest: &Path,
    ) -> Result<(), AppError>;

    /// Builds a new, unencrypted document from `plan`. Scratch files go into `temp_dir` and are
    /// removed before returning.
    fn assemble(&self, plan: &AssemblyPlan, temp_dir: &Path, dest: &Path) -> Result<(), AppError>;
}

/// A bookmark to write into a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineEntry {
    /// Text shown in the bookmark panel.
    pub title: String,
    /// The page it jumps to.
    pub page: PageIndex,
    /// Nesting depth, 0 for the top level.
    pub level: u32,
}

/// An edit applied to a finished, unencrypted PDF.
#[derive(Debug, Clone, PartialEq)]
pub enum PdfEdit {
    /// Replaces the document outline with these bookmarks.
    SetOutline(Vec<OutlineEntry>),
    /// Sets the crop box of pages to a fraction of their current visible area.
    CropFractions(Vec<(PageIndex, FractionBox)>),
    /// Scales every page's content to fit pages of this size, centered.
    ScalePagesTo {
        /// Target width in points.
        width_pt: f32,
        /// Target height in points.
        height_pt: f32,
    },
    /// Changes document information fields (and the XMP packet when there is one).
    SetMetadata(MetadataChange),
    /// Removes annotations of these kinds from these pages.
    RemoveAnnotations {
        /// Kinds to remove.
        kinds: Vec<AnnotationKind>,
        /// Pages to clean.
        pages: Vec<PageIndex>,
    },
    /// Removes hidden or risky content.
    Sanitize(SanitizeOptions),
}

/// Applies structural edits to PDFs.
pub trait PdfEditor: Send + Sync {
    /// Writes `source` with `edit` applied to `dest`.
    fn apply(&self, source: &Path, edit: &PdfEdit, dest: &Path) -> Result<(), AppError>;
}

/// Encrypts and decrypts documents.
pub trait PdfSecurity: Send + Sync {
    /// Writes `source` to `dest` encrypted as `plan` says. The new passwords never appear on a
    /// command line or in a file.
    fn protect(
        &self,
        source: &Path,
        password: Option<&Password>,
        plan: &ProtectPlan,
        dest: &Path,
    ) -> Result<(), AppError>;

    /// Writes an unencrypted copy of `source`. Needs the open password when there is one.
    fn unlock(
        &self,
        source: &Path,
        password: Option<&Password>,
        dest: &Path,
    ) -> Result<(), AppError>;
}

/// How a finished document is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageOptions {
    /// Write for fast web view.
    pub linearize: bool,
    /// Pack objects into compressed object streams.
    pub object_streams: bool,
}

/// Rewrites the file structure: stream compression, object streams and linearization.
pub trait PdfPackager: Send + Sync {
    /// Writes an unencrypted, repacked copy of `source` to `dest`.
    fn package(
        &self,
        source: &Path,
        password: Option<&Password>,
        options: &PackageOptions,
        dest: &Path,
    ) -> Result<(), AppError>;
}

/// What a repair attempt found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepairOutcome {
    /// Number of problems the engine reported and worked around.
    pub problems: usize,
}

/// One way to rewrite a damaged document into a sound one. The Repair tool tries them in turn.
pub trait PdfRepairer: Send + Sync {
    /// Short name for the log, such as `qpdf`.
    fn name(&self) -> &'static str;

    /// Writes a repaired copy of `source` to `dest`.
    fn repair(
        &self,
        source: &Path,
        password: Option<&Password>,
        dest: &Path,
    ) -> Result<RepairOutcome, AppError>;
}

/// What Compress did to the images.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImageReport {
    /// Images found.
    pub found: usize,
    /// Images that were replaced by a smaller version.
    pub replaced: usize,
}

/// Reduces the size of the pictures inside a document.
pub trait ImageOptimizer: Send + Sync {
    /// Writes `source` (unencrypted) to `dest` with images downsampled and recompressed. An
    /// image only changes when the new version is smaller.
    fn optimize_images(
        &self,
        source: &Path,
        settings: &CompressSettings,
        dest: &Path,
    ) -> Result<ImageReport, AppError>;
}

/// One page picture for [`PageImageWriter`].
#[derive(Debug, Clone)]
pub struct PageImage {
    /// A PNG file.
    pub png: PathBuf,
    /// Page width in points.
    pub width_pt: f32,
    /// Page height in points.
    pub height_pt: f32,
}

/// Builds a document whose pages are pictures.
pub trait PageImageWriter: Send + Sync {
    /// Writes one page per image, each `width_pt` by `height_pt`, to `dest`.
    fn pages_to_pdf(&self, pages: &[PageImage], dest: &Path) -> Result<(), AppError>;
}

/// One line of a generated table of contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocLine {
    /// Text of the line, usually a file name.
    pub title: String,
    /// One-based page number the line points to.
    pub page_number: u32,
}

/// Builds a table of contents document.
pub trait TocBuilder: Send + Sync {
    /// Returns the number of pages a table of contents with `lines` lines needs.
    fn page_count(&self, lines: usize) -> u32;

    /// Writes a table of contents document with pages of the given size to `dest`.
    fn build(
        &self,
        lines: &[TocLine],
        width_pt: f32,
        height_pt: f32,
        dest: &Path,
    ) -> Result<(), AppError>;
}

/// Turns an image file into a one-page PDF.
pub trait ImageConverter: Send + Sync {
    /// Writes a PDF with the image filling one page of the image's own proportions.
    fn image_to_pdf(&self, image: &Path, dest: &Path) -> Result<(), AppError>;
}

/// A bookmark read from a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineItem {
    /// Bookmark title.
    pub title: String,
    /// Target page when the bookmark points at one.
    pub page: Option<PageIndex>,
    /// Nesting depth, 1 for the top level.
    pub level: u32,
}

/// Reads document outlines (bookmarks).
pub trait OutlineReader: Send + Sync {
    /// All bookmarks in reading order.
    fn outline(
        &self,
        path: &Path,
        password: Option<&Password>,
    ) -> Result<Vec<OutlineItem>, AppError>;
}

/// Flattens interactive form fields and annotations into page content.
pub trait FormFlattener: Send + Sync {
    /// Writes `source` to `dest` with fields flattened.
    fn flatten_forms(
        &self,
        source: &Path,
        password: Option<&Password>,
        dest: &Path,
    ) -> Result<(), AppError>;
}

/// Renders pages to pixels.
pub trait Renderer: Send + Sync {
    /// Renders one page at the given resolution.
    fn render_page(
        &self,
        path: &Path,
        password: Option<&Password>,
        page: PageIndex,
        dpi: Dpi,
    ) -> Result<RenderedImage, AppError>;

    /// Renders one page so its longest edge is at most `max_edge` pixels and returns it as PNG.
    fn render_png(
        &self,
        path: &Path,
        password: Option<&Password>,
        page: PageIndex,
        max_edge: u32,
    ) -> Result<PngImage, AppError>;
}

/// A PNG-encoded image with its pixel size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PngImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// PNG file bytes.
    pub png: Vec<u8>,
}

/// Extracts text.
pub trait TextExtractor: Send + Sync {
    /// Returns the text of one page.
    fn page_text(
        &self,
        path: &Path,
        password: Option<&Password>,
        page: PageIndex,
    ) -> Result<String, AppError>;

    /// Returns the text inside `region` of one page.
    fn text_in_region(
        &self,
        path: &Path,
        password: Option<&Password>,
        page: PageIndex,
        region: &Region,
    ) -> Result<String, AppError>;
}

/// Optical character recognition.
pub trait OcrEngine: Send + Sync {
    /// Recognizes text in an image using Tesseract language codes such as `eng`.
    fn recognize(&self, image: &RenderedImage, languages: &[String]) -> Result<String, AppError>;
}

/// Converts Office documents to PDF.
pub trait OfficeConverter: Send + Sync {
    /// Converts `source` to a PDF at `dest`.
    fn convert_to_pdf(&self, source: &Path, dest: &Path) -> Result<(), AppError>;
}

/// Converts HTML to PDF.
pub trait HtmlConverter: Send + Sync {
    /// Prints a local file or consented URL to a PDF at `dest`.
    fn html_to_pdf(&self, source: &str, dest: &Path) -> Result<(), AppError>;
}

/// Produces CMS signatures. Private keys never leave the implementation.
pub trait Signer: Send + Sync {
    /// Signs a SHA-256 digest and returns a DER-encoded CMS structure.
    fn sign_digest(&self, digest: &[u8]) -> Result<Vec<u8>, AppError>;
}

/// Validates PDF/A and PDF/UA conformance.
pub trait PdfaValidator: Send + Sync {
    /// Validates `path` against `profile` (`None` auto-detects the claimed level).
    fn validate(&self, path: &Path, profile: Option<&str>) -> Result<ValidationReport, AppError>;
}

/// Local language model engine, off by default.
pub trait LlmEngine: Send + Sync {
    /// Completes a prompt. Only runs after the user enabled Local AI.
    fn complete(&self, prompt: &str) -> Result<String, AppError>;
}

/// File system access with the integrity rules of AGENTS.md section 7.
pub trait FileStore: Send + Sync {
    /// Resolves a path to its canonical form, rejecting reparse-point escapes.
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, AppError>;

    /// True when something exists at `path`.
    fn exists(&self, path: &Path) -> bool;

    /// File size in bytes.
    fn len(&self, path: &Path) -> Result<u64, AppError>;

    /// Reads up to `max` bytes from the start of a file, for magic-byte checks.
    fn read_head(&self, path: &Path, max: usize) -> Result<Vec<u8>, AppError>;

    /// Removes a scratch file. Failure is ignored: leftovers are wiped with the session folder.
    fn remove_file(&self, path: &Path);

    /// Last modification time in seconds since the Unix epoch, when known.
    fn modified_unix(&self, path: &Path) -> Option<u64>;

    /// Like [`FileStore::write_atomic`], for engines that write to a path instead of a stream.
    ///
    /// `produce` receives a temp path in the destination folder and must create the file there.
    /// After `verify` passes, the file is published at `dest` without replacing anything.
    fn publish(
        &self,
        dest: &Path,
        produce: &mut dyn FnMut(&Path) -> Result<(), AppError>,
        verify: &dyn Fn(&Path) -> Result<(), AppError>,
    ) -> Result<PathBuf, AppError>;

    /// Writes `dest` atomically: temp file in the destination folder, flush, `verify`, rename.
    ///
    /// Never replaces an existing file. On any failure the temp file is removed and the error
    /// is returned. `verify` receives the temp path and must reopen and check the output.
    fn write_atomic(
        &self,
        dest: &Path,
        write: &mut dyn FnMut(&mut dyn Write) -> io::Result<()>,
        verify: &dyn Fn(&Path) -> Result<(), AppError>,
    ) -> Result<PathBuf, AppError>;
}

/// Loads and saves user settings.
pub trait SettingsStore: Send + Sync {
    /// Loads settings, migrating older versions and falling back to defaults when missing.
    fn load(&self) -> Result<Settings, AppError>;

    /// Saves settings atomically.
    fn save(&self, settings: &Settings) -> Result<(), AppError>;
}

/// Lists installed on-demand components.
pub trait ComponentStore: Send + Sync {
    /// Components installed and verified on this machine.
    fn installed(&self) -> Result<Vec<ComponentEntry>, AppError>;
}

/// Source of the current local date and time.
pub trait Clock: Send + Sync {
    /// Current local time.
    fn now(&self) -> DateTimeParts;
}

/// Severity of a log event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// Normal operation.
    Info,
    /// Recoverable problem.
    Warn,
    /// Failure.
    Error,
}

/// Structured logging. Callers must never pass document content, passwords or raw paths.
pub trait Logger: Send + Sync {
    /// Records an event with a stable name such as `job.finished`.
    fn log(&self, level: LogLevel, event: &str);
}
