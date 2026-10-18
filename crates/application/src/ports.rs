//! Ports: traits that engines and infrastructure implement. Tools and use cases depend on these
//! and never on concrete engines.

use domain::components::ComponentEntry;
use domain::naming::DateTimeParts;
use domain::settings::Settings;
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

/// Writes PDF output.
pub trait PdfWriter: Send + Sync {
    /// Copies `pages` (in order) from `source` into a new document written to `dest`.
    fn copy_pages(
        &self,
        source: &Path,
        password: Option<&Password>,
        pages: &[PageIndex],
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
