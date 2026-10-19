//! Ports: traits that engines and infrastructure implement. Tools and use cases depend on these
//! and never on concrete engines.

use domain::components::ComponentEntry;
use domain::naming::DateTimeParts;
use domain::settings::Settings;
use domain::stamp::PageStamps;
use domain::tools::common::{FractionBox, Region, RightAngle};
use domain::tools::convert::{ImagesToPdfOptions, PictureColor, PictureFormat, TiffCompression};
use domain::tools::document::{AnnotationKind, MetadataChange};
use domain::tools::layout::{FlipDirection, NUpOptions};
use domain::tools::office::SheetOrientation;
use domain::tools::optimize::CompressSettings;
use domain::tools::secure::{ProtectPlan, SanitizeOptions};
use domain::tools::structure::{LabelRange, TextLine};
use domain::{Dpi, PageIndex, PageRange};
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
    /// Draws text and pictures on pages.
    Stamp(Vec<PageStamps>),
    /// Sets the crop box (and optionally the media box) of pages to a fraction of their visible
    /// area.
    CropPages {
        /// The pages and what to keep of each.
        crops: Vec<(PageIndex, FractionBox)>,
        /// Also cut the media box.
        media_box: bool,
    },
    /// Changes pages to a new size, optionally scaling and centering their content.
    ResizePages {
        /// Target size of a portrait page in points.
        width_pt: f32,
        /// Target size of a portrait page in points.
        height_pt: f32,
        /// Empty space kept around the content.
        margin_pt: f32,
        /// Scale the content to fit; otherwise it keeps its size.
        scale: bool,
        /// Turn the target to match each page's orientation.
        match_orientation: bool,
    },
    /// Mirrors pages.
    Flip {
        /// Which way.
        direction: FlipDirection,
        /// Which pages.
        pages: Vec<PageIndex>,
    },
    /// Several edits, one after the other.
    Sequence(Vec<PdfEdit>),
    /// Turns the content of pages about the center of the page as displayed. The angle is in
    /// degrees, counter-clockwise; the page keeps its size.
    Deskew(Vec<(PageIndex, f32)>),
    /// Draws the single page of each file over the page it is listed with. The recognizer writes
    /// these pages with invisible text, so a scanned page becomes searchable.
    TextLayers(Vec<(PageIndex, PathBuf)>),
    /// Puts several pages on each sheet.
    NUp(NUpOptions),
    /// Turns colored text and lines gray (pictures are handled separately).
    GrayscaleContent,
    /// Replaces the page labels. An empty list removes them.
    SetPageLabels(Vec<LabelRange>),
    /// Attaches files.
    AddAttachments(Vec<PathBuf>),
    /// Removes attachments by name.
    RemoveAttachments(Vec<String>),
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

/// An encrypted document whose passwords, key length and permissions a finished copy keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectionSource {
    /// The encrypted document.
    pub path: PathBuf,
    /// A password that opens it.
    pub password: Password,
}

/// How a finished document is packed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageOptions {
    /// Write for fast web view.
    pub linearize: bool,
    /// Pack objects into compressed object streams.
    pub object_streams: bool,
    /// Encrypt the result the way this document is encrypted. Without it the result is
    /// unencrypted.
    pub keep_protection: Option<ProtectionSource>,
}

impl PackageOptions {
    /// Options for an unencrypted result.
    pub const fn new(linearize: bool, object_streams: bool) -> Self {
        Self {
            linearize,
            object_streams,
            keep_protection: None,
        }
    }
}

/// Rewrites the file structure: stream compression, object streams and linearization.
pub trait PdfPackager: Send + Sync {
    /// Writes a repacked copy of `source` to `dest`: unencrypted, or protected like
    /// `options.keep_protection` says.
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

    /// Writes `source` (unencrypted) to `dest` with every color picture turned gray. Unlike
    /// [`ImageOptimizer::optimize_images`] it changes a picture even when the result is larger.
    fn grayscale_images(&self, source: &Path, dest: &Path) -> Result<ImageReport, AppError>;
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

/// Turns image files into PDF pages.
pub trait ImageConverter: Send + Sync {
    /// Writes a PDF with the image filling one page of the image's own proportions.
    fn image_to_pdf(&self, image: &Path, dest: &Path) -> Result<(), AppError>;

    /// Writes one page per image, laid out by `layout`. Pictures in HEIC are converted through
    /// the Windows imaging component first, using `scratch` for the temporary file.
    fn images_to_pdf(
        &self,
        images: &[PathBuf],
        layout: &ImagesToPdfOptions,
        scratch: &Path,
        dest: &Path,
    ) -> Result<(), AppError>;

    /// Writes one page per frame of a TIFF file, sized from its resolution, and returns how many
    /// pages it wrote.
    fn tiff_to_pdf(&self, tiff: &Path, dest: &Path) -> Result<u32, AppError>;
}

/// How one page picture is written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PictureSpec {
    /// File format.
    pub format: PictureFormat,
    /// JPEG quality, 1 to 100.
    pub jpeg_quality: u8,
    /// Colors.
    pub color: PictureColor,
    /// Resolution written into the file.
    pub dpi: Dpi,
}

/// How a multi-page TIFF is written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TiffSpec {
    /// Compression of every page.
    pub compression: TiffCompression,
    /// Colors. CCITT Group 4 always writes black and white.
    pub color: PictureColor,
    /// Resolution written into the file.
    pub dpi: Dpi,
}

/// A TIFF file that grows one page at a time, so a long document never sits in memory.
pub trait TiffPages {
    /// Appends one page.
    fn add(&mut self, image: &RenderedImage) -> Result<(), AppError>;

    /// Completes the file.
    fn finish(self: Box<Self>) -> Result<(), AppError>;
}

/// Measures how far a scanned page is tilted.
pub trait SkewDetector: Send + Sync {
    /// The angle in degrees that turns the content counter-clockwise until its text lines run
    /// level, or `None` when the page has too little to go by. Tilts larger than
    /// `max_angle_deg` are not looked for.
    fn skew(&self, image: &RenderedImage, max_angle_deg: f32) -> Result<Option<f32>, AppError>;

    /// The picture turned counter-clockwise by `degrees` about its center, on the same canvas,
    /// with white where the picture no longer reaches.
    fn rotate(&self, image: &RenderedImage, degrees: f32) -> Result<RenderedImage, AppError>;
}

/// Turns pictures of pages into a PDF.
pub trait ScanPageWriter: Send + Sync {
    /// Cuts away the background, stands tilted pages upright, reduces the colors as asked and
    /// writes one page per picture to `dest`. Returns how many pages it wrote. The resolution in
    /// `options` sets the page size.
    fn scans_to_pdf(
        &self,
        images: &[PathBuf],
        options: &domain::tools::recognize::ScanToPdfOptions,
        scratch: &Path,
        dest: &Path,
        cancel: &crate::runtime::CancellationToken,
    ) -> Result<u32, AppError>;
}

/// Writes rendered pages as picture files.
pub trait PictureWriter: Send + Sync {
    /// Writes one page picture.
    fn write_picture(
        &self,
        image: &RenderedImage,
        spec: &PictureSpec,
        dest: &Path,
    ) -> Result<(), AppError>;

    /// Starts a multi-page TIFF at `dest`.
    fn start_tiff(&self, dest: &Path, spec: &TiffSpec) -> Result<Box<dyn TiffPages>, AppError>;

    /// Reopens a written file and checks that it decodes. `pages` is how many pages it must have.
    fn check_picture(&self, path: &Path, format: PictureFormat, pages: u32)
    -> Result<(), AppError>;
}

/// Container of a picture taken out of a PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractedKind {
    /// A JPEG file.
    Jpeg,
    /// A JPEG 2000 file.
    Jpeg2000,
    /// A PNG file.
    Png,
}

impl ExtractedKind {
    /// File extension without the dot.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Jpeg2000 => "jp2",
            Self::Png => "png",
        }
    }
}

/// One picture taken out of a PDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedImage {
    /// Page it appears on.
    pub page: PageIndex,
    /// Position among the pictures of that page, from 1.
    pub number: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// File format of `bytes`.
    pub kind: ExtractedKind,
    /// The file contents.
    pub bytes: Vec<u8>,
}

/// What an extraction left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExtractReport {
    /// Pictures found, including the ones left out.
    pub found: u32,
    /// Pictures smaller than the filter.
    pub too_small: u32,
    /// Pictures in a format that cannot be taken out (such as scans stored as CCITT or JBIG2).
    pub unsupported: u32,
}

/// Takes the pictures out of a PDF.
pub trait ImageExtractor: Send + Sync {
    /// Calls `sink` for every picture at least `min` (width, height) pixels, in page order.
    /// `source` must be unencrypted. With `keep_original` off every picture is written as PNG.
    fn extract(
        &self,
        source: &Path,
        keep_original: bool,
        min: (u32, u32),
        sink: &mut dyn FnMut(ExtractedImage) -> Result<(), AppError>,
    ) -> Result<ExtractReport, AppError>;
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

/// A file attached to a document, listed without its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentInfo {
    /// The name the file was attached under.
    pub name: String,
    /// Size in bytes, when known.
    pub size: Option<u64>,
    /// The description given when it was attached.
    pub description: Option<String>,
}

/// A file attached to a document, with its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// The name the file was attached under.
    pub name: String,
    /// The bytes.
    pub data: Vec<u8>,
}

/// Reads the files attached to a document.
pub trait AttachmentReader: Send + Sync {
    /// Lists the attachments.
    fn list(
        &self,
        path: &Path,
        password: Option<&Password>,
    ) -> Result<Vec<AttachmentInfo>, AppError>;

    /// Reads the attachments called `names`, or all of them when `names` is empty. An unknown
    /// name is an error.
    fn read(
        &self,
        path: &Path,
        password: Option<&Password>,
        names: &[String],
    ) -> Result<Vec<Attachment>, AppError>;
}

/// Reads the lines of text of a document with their sizes, to find headings.
pub trait HeadingFinder: Send + Sync {
    /// Every line of text on every page, in reading order.
    fn text_lines(
        &self,
        path: &Path,
        password: Option<&Password>,
    ) -> Result<Vec<TextLine>, AppError>;
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

    /// Returns the text of one page with its columns and gaps kept in place by spaces.
    fn page_text_layout(
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

/// A picture from a page, ready to be put in a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureBytes {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// True when `bytes` is a JPEG file, false for a PNG file.
    pub jpeg: bool,
    /// The encoded file.
    pub bytes: Vec<u8>,
}

/// Reads what is on a page for the tools that rebuild it as another document.
pub trait LayoutReader: Send + Sync {
    /// The text, the lines that may be table borders and the pictures of one page, in points on
    /// the page as displayed (origin top left, y down).
    fn page_content(
        &self,
        path: &Path,
        password: Option<&Password>,
        page: PageIndex,
    ) -> Result<domain::layout::PageContent, AppError>;

    /// The `index`-th picture of a page, as listed in its content.
    fn picture(
        &self,
        path: &Path,
        password: Option<&Password>,
        page: PageIndex,
        index: usize,
    ) -> Result<PictureBytes, AppError>;
}

/// Where a document writer gets a picture: the page number (from 1) and the picture's index.
pub type PictureSource<'a> = dyn FnMut(u32, usize) -> Result<PictureBytes, AppError> + 'a;

/// One sheet of a workbook: a name and rows of text. The writer turns text that is a number into a
/// number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetData {
    /// Sheet name; the writer makes it valid and unique.
    pub name: String,
    /// The rows; they need not be equally long.
    pub rows: Vec<Vec<String>>,
}

/// Writes the editable formats Word, PowerPoint and Excel open.
pub trait DocumentWriter: Send + Sync {
    /// A Word document. `exact` places every block in a frame where it was on the page; otherwise
    /// the text flows.
    fn write_docx(
        &self,
        pages: &[domain::layout::PageLayout],
        title: &str,
        exact: bool,
        pictures: &mut PictureSource<'_>,
        dest: &Path,
    ) -> Result<(), AppError>;

    /// A presentation with one slide per page: text boxes, tables and pictures.
    fn write_pptx(
        &self,
        pages: &[domain::layout::PageLayout],
        title: &str,
        pictures: &mut PictureSource<'_>,
        dest: &Path,
    ) -> Result<(), AppError>;

    /// A presentation with one picture per slide. `sizes` are the page sizes in points;
    /// `image` gives the picture of the page at that position.
    fn write_image_pptx(
        &self,
        sizes: &[(f32, f32)],
        title: &str,
        image: &mut dyn FnMut(usize) -> Result<RenderedImage, AppError>,
        dest: &Path,
    ) -> Result<(), AppError>;

    /// A workbook.
    fn write_xlsx(&self, sheets: &[SheetData], dest: &Path) -> Result<(), AppError>;

    /// Reopens a file the writer made and checks that it is a complete package with its main
    /// part. `parts` is the number of slides or sheets it must hold, or 0 when that does not apply.
    fn verify(&self, path: &Path, parts: u32) -> Result<(), AppError>;
}

/// A word read from a page picture, in pixels of that picture.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrWord {
    /// The text.
    pub text: String,
    /// Left edge.
    pub left: f32,
    /// Top edge.
    pub top: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
    /// Which line of the page the word is on; words with the same number share a line.
    pub line: u32,
    /// How sure the recognizer is, 0 to 100.
    pub confidence: f32,
}

/// Optical character recognition.
pub trait OcrEngine: Send + Sync {
    /// True when the recognizer is installed.
    fn is_available(&self) -> bool;

    /// Codes of the languages that are installed, such as `eng`.
    fn languages(&self) -> Result<Vec<String>, AppError>;

    /// The text of a page picture read at `dpi`. `scratch` is a folder for temporary files.
    fn recognize(
        &self,
        image: &RenderedImage,
        languages: &[String],
        dpi: Dpi,
        scratch: &Path,
        cancel: &crate::runtime::CancellationToken,
    ) -> Result<String, AppError>;

    /// The words of a page picture read at `dpi`, with their boxes.
    fn words(
        &self,
        image: &RenderedImage,
        languages: &[String],
        dpi: Dpi,
        scratch: &Path,
        cancel: &crate::runtime::CancellationToken,
    ) -> Result<Vec<OcrWord>, AppError>;

    /// Writes a one-page PDF at `dest` that holds only the invisible text of the page picture.
    /// Its page is as large as the picture is at `dpi`.
    fn text_layer(
        &self,
        image: &RenderedImage,
        languages: &[String],
        dpi: Dpi,
        scratch: &Path,
        dest: &Path,
        cancel: &crate::runtime::CancellationToken,
    ) -> Result<(), AppError>;
}

/// The kind of Office document a conversion starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeKind {
    /// A word processor document.
    Word,
    /// A presentation.
    Slides,
    /// A spreadsheet.
    Sheet,
}

/// What an Office conversion should do beyond turning the document into pages.
#[derive(Debug, Clone, PartialEq)]
pub struct OfficeRequest {
    /// What kind of document it is.
    pub kind: OfficeKind,
    /// For presentations: include hidden slides.
    pub include_hidden_slides: bool,
    /// For spreadsheets: the sheets to print, by position. `all` prints every visible sheet.
    pub sheets: PageRange,
    /// For spreadsheets: shrink sheets so every column fits the page width.
    pub fit_to_page_width: bool,
    /// For spreadsheets: page turn.
    pub orientation: SheetOrientation,
}

impl OfficeRequest {
    /// A request with no extra options for `kind`.
    pub fn plain(kind: OfficeKind) -> Self {
        Self {
            kind,
            include_hidden_slides: false,
            sheets: PageRange::all(),
            fit_to_page_width: false,
            orientation: SheetOrientation::Auto,
        }
    }
}

/// Converts Office documents to PDF with one program (Microsoft Office or LibreOffice).
pub trait OfficeConverter: Send + Sync {
    /// The program's name, for messages.
    fn name(&self) -> &'static str;

    /// True when the program can convert documents of `kind` on this computer right now.
    fn is_available(&self, kind: OfficeKind) -> bool;

    /// Converts `source` to a PDF at `dest`. `scratch` is a folder for temporary files. The
    /// conversion stops with `QW-JOB-CANCELLED` soon after `cancel` is set.
    fn convert(
        &self,
        source: &Path,
        request: &OfficeRequest,
        scratch: &Path,
        dest: &Path,
        cancel: &crate::runtime::CancellationToken,
    ) -> Result<(), AppError>;
}

/// The two ways to turn an Office document into a PDF.
#[derive(Clone)]
pub struct OfficeEngines {
    /// Microsoft Office on this computer.
    pub microsoft: std::sync::Arc<dyn OfficeConverter>,
    /// LibreOffice, from a component or an installed copy.
    pub libre: std::sync::Arc<dyn OfficeConverter>,
}

/// What a web page printout starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtmlSource {
    /// A file on this computer.
    File(PathBuf),
    /// A web address.
    Url(String),
}

/// Prints web pages to PDF with the web view that ships with Windows.
pub trait HtmlConverter: Send + Sync {
    /// True when the web view is installed.
    fn is_available(&self) -> bool;

    /// Loads `source` and prints it to a PDF at `dest` as `options` say. Pictures, styles and
    /// scripts that live on the internet are only fetched when `options.allow_network` is set.
    fn print(
        &self,
        source: &HtmlSource,
        options: &domain::tools::HtmlToPdfOptions,
        dest: &Path,
        cancel: &crate::runtime::CancellationToken,
    ) -> Result<(), AppError>;
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

    /// Copies the file at `from` to `to`, which is a temp path produced by [`FileStore::publish`].
    fn copy_file(&self, from: &Path, to: &Path) -> Result<(), AppError>;

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

    /// The folder of the newest installed version of a component, when there is one.
    fn locate(&self, id: &str) -> Option<PathBuf>;
}

/// Downloads a file over HTTPS.
pub trait Downloader: Send + Sync {
    /// Streams the body of `url` to `sink`. Fails when the body is larger than `max_bytes`.
    fn download(
        &self,
        url: &str,
        max_bytes: u64,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), AppError>,
    ) -> Result<(), AppError>;
}

/// Installs, imports and removes on-demand components. Everything it puts on disk has been
/// checked against the signed list first.
pub trait ComponentInstaller: Send + Sync {
    /// The components this build of Quirewise can install, from the signed component list. Needs
    /// the network, so callers ask the person first.
    fn available(&self) -> Result<Vec<ComponentEntry>, AppError>;

    /// Downloads, verifies and installs the newest version of `id` for this machine. `progress`
    /// counts downloaded bytes.
    fn install(
        &self,
        id: &str,
        progress: &dyn crate::runtime::ProgressSink,
    ) -> Result<ComponentEntry, AppError>;

    /// Installs a component from a `.qwpkg` file on disk, for machines without a network.
    fn import(&self, package: &Path) -> Result<ComponentEntry, AppError>;

    /// Removes every installed version of a component.
    fn remove(&self, id: &str) -> Result<(), AppError>;
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
