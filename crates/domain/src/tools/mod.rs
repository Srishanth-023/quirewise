//! Tool identifiers and per-tool option structs. One module per tool.

pub mod combine;
pub mod common;
pub mod convert;
pub mod document;
pub mod editor;
pub mod export;
pub mod layout;
pub mod office;
pub mod optimize;
pub mod pages;
pub mod recognize;
pub mod rotate;
pub mod secure;
pub mod stamp;
pub mod structure;

use core::fmt;
use core::str::FromStr;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
pub use combine::{AlternateMixOptions, MergeOptions, OrganizeOptions};
pub use convert::{
    ExtractImagesOptions, ImagesToPdfOptions, PdfToImagesOptions, PdfToTextOptions,
    PdfToTiffOptions, TiffToPdfOptions,
};
pub use document::{FlattenOptions, MetadataOptions, RemoveAnnotationsOptions};
pub use editor::{
    AnnotateOptions, CreateFormsOptions, EditPdfOptions, FillSignOptions, SignPdfOptions,
};
pub use export::{
    PdfToExcelOptions, PdfToHtmlOptions, PdfToMarkdownOptions, PdfToPowerPointOptions,
    PdfToWordOptions,
};
pub use layout::{CropOptions, FlipOptions, GrayscaleOptions, NUpOptions, ResizeOptions};
pub use office::{ExcelToPdfOptions, HtmlToPdfOptions, PowerPointToPdfOptions, WordToPdfOptions};
pub use optimize::{CompressOptions, LinearizeOptions, RepairOptions};
pub use pages::{
    ExtractPagesOptions, InsertPagesOptions, RemovePagesOptions, SplitByBookmarksOptions,
    SplitBySizeOptions, SplitByTextOptions, SplitEveryNOptions, SplitInHalfOptions,
    SplitRangesOptions,
};
pub use recognize::{DeskewOptions, OcrOptions, ScanToPdfOptions};
pub use rotate::RotateOptions;
pub use secure::{ProtectOptions, SanitizeOptions, UnlockOptions};
pub use stamp::{BatesOptions, HeaderFooterOptions, PageNumbersOptions, WatermarkOptions};
pub use structure::{
    AttachmentsOptions, BookmarksOptions, PageLabelsOptions, RenameByContentOptions,
};

/// Navigation group a tool belongs to (MASTER_PROMPT.md section 3.1).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ToolGroup {
    /// Merge, split, extract and reorder pages.
    Organize,
    /// Compress, repair, OCR and deskew.
    Optimize,
    /// Images, Office files and web pages to PDF.
    ConvertToPdf,
    /// PDF to images, Office files, text and PDF/A.
    ConvertFromPdf,
    /// Rotate, stamp, crop, annotate and edit.
    Edit,
    /// Fill, create and flatten forms.
    Forms,
    /// Protect, unlock, sign, redact and compare.
    Security,
    /// Optional local AI tools.
    Intelligence,
    /// PDF/A and accessibility validation.
    Validation,
}

/// Identifier of a tool, kebab-case on the wire (`split-by-size`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ToolId {
    /// Merge PDFs and images (Appendix A row 1).
    Merge,
    /// Alternate and mix pages of several files (row 2).
    AlternateMix,
    /// Organize pages visually (row 3).
    Organize,
    /// Split by ranges (row 4).
    SplitRanges,
    /// Split every N pages (row 5).
    SplitEveryN,
    /// Extract pages (row 6).
    ExtractPages,
    /// Remove pages (row 7).
    RemovePages,
    /// Split by bookmarks (row 8).
    SplitByBookmarks,
    /// Split pages in half (row 9).
    SplitInHalf,
    /// Split by file size (row 10).
    SplitBySize,
    /// Split by text in a region (row 11).
    SplitByText,
    /// Rotate pages (row 32).
    Rotate,
    /// Insert blank or duplicate pages (row 69).
    InsertPages,
    /// Compress (row 14).
    Compress,
    /// Repair (row 15).
    Repair,
    /// Remove annotations (row 44).
    RemoveAnnotations,
    /// Flatten (row 47).
    Flatten,
    /// Edit metadata (row 48).
    EditMetadata,
    /// Protect (row 51).
    Protect,
    /// Unlock (row 52).
    Unlock,
    /// Sanitize (row 63).
    Sanitize,
    /// Linearize (row 64).
    Linearize,
    /// Page numbers (row 33).
    PageNumbers,
    /// Header and footer (row 34).
    HeaderFooter,
    /// Bates numbering (row 35).
    Bates,
    /// Watermark (row 36).
    Watermark,
    /// Crop (row 37).
    Crop,
    /// Resize pages (row 38).
    Resize,
    /// Flip (row 39).
    Flip,
    /// N-up (row 40).
    NUp,
    /// Grayscale (row 41).
    Grayscale,
    /// Bookmarks editor (row 49).
    Bookmarks,
    /// Rename by content (row 50).
    RenameByContent,
    /// Attachments (row 65).
    Attachments,
    /// Page labels (row 66).
    PageLabels,
    /// Edit PDF (row 42).
    EditPdf,
    /// Annotate (row 43).
    Annotate,
    /// Fill and sign (row 45).
    FillSign,
    /// Sign PDF (row 53).
    SignPdf,
    /// Create forms (row 46).
    CreateForms,
    /// PDF to Word (row 25).
    PdfToWord,
    /// PDF to PowerPoint (row 26).
    PdfToPowerpoint,
    /// PDF to Excel (row 27).
    PdfToExcel,
    /// PDF to Markdown (row 29).
    PdfToMarkdown,
    /// PDF to HTML (row 67).
    PdfToHtml,
    /// OCR (row 16).
    Ocr,
    /// Deskew (row 17).
    Deskew,
    /// Scan to PDF (row 13).
    ScanToPdf,
    /// Word to PDF (row 19).
    WordToPdf,
    /// PowerPoint to PDF (row 20).
    PowerpointToPdf,
    /// Excel to PDF (row 21).
    ExcelToPdf,
    /// HTML to PDF (row 22).
    HtmlToPdf,
    /// Images to PDF (row 18).
    ImagesToPdf,
    /// PDF to images (row 23).
    PdfToImages,
    /// Extract images (row 24).
    ExtractImages,
    /// PDF to text (row 28).
    PdfToText,
    /// TIFF to PDF (row 68).
    TiffToPdf,
    /// PDF to TIFF (row 68).
    PdfToTiff,
}

impl ToolId {
    /// Every tool id the domain knows. The runtime tool list comes from the registry.
    pub const ALL: [ToolId; 58] = [
        ToolId::Merge,
        ToolId::AlternateMix,
        ToolId::Organize,
        ToolId::SplitRanges,
        ToolId::SplitEveryN,
        ToolId::ExtractPages,
        ToolId::RemovePages,
        ToolId::SplitByBookmarks,
        ToolId::SplitInHalf,
        ToolId::SplitBySize,
        ToolId::SplitByText,
        ToolId::Rotate,
        ToolId::InsertPages,
        ToolId::Compress,
        ToolId::Repair,
        ToolId::RemoveAnnotations,
        ToolId::Flatten,
        ToolId::EditMetadata,
        ToolId::Protect,
        ToolId::Unlock,
        ToolId::Sanitize,
        ToolId::Linearize,
        ToolId::PageNumbers,
        ToolId::HeaderFooter,
        ToolId::Bates,
        ToolId::Watermark,
        ToolId::Crop,
        ToolId::Resize,
        ToolId::Flip,
        ToolId::NUp,
        ToolId::Grayscale,
        ToolId::Bookmarks,
        ToolId::RenameByContent,
        ToolId::Attachments,
        ToolId::PageLabels,
        ToolId::EditPdf,
        ToolId::Annotate,
        ToolId::FillSign,
        ToolId::SignPdf,
        ToolId::CreateForms,
        ToolId::PdfToWord,
        ToolId::PdfToPowerpoint,
        ToolId::PdfToExcel,
        ToolId::PdfToMarkdown,
        ToolId::PdfToHtml,
        ToolId::Ocr,
        ToolId::Deskew,
        ToolId::ScanToPdf,
        ToolId::WordToPdf,
        ToolId::PowerpointToPdf,
        ToolId::ExcelToPdf,
        ToolId::HtmlToPdf,
        ToolId::ImagesToPdf,
        ToolId::PdfToImages,
        ToolId::ExtractImages,
        ToolId::PdfToText,
        ToolId::TiffToPdf,
        ToolId::PdfToTiff,
    ];

    /// The kebab-case identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::AlternateMix => "alternate-mix",
            Self::Organize => "organize",
            Self::SplitRanges => "split-ranges",
            Self::SplitEveryN => "split-every-n",
            Self::ExtractPages => "extract-pages",
            Self::RemovePages => "remove-pages",
            Self::SplitByBookmarks => "split-by-bookmarks",
            Self::SplitInHalf => "split-in-half",
            Self::SplitBySize => "split-by-size",
            Self::SplitByText => "split-by-text",
            Self::Rotate => "rotate",
            Self::InsertPages => "insert-pages",
            Self::Compress => "compress",
            Self::Repair => "repair",
            Self::RemoveAnnotations => "remove-annotations",
            Self::Flatten => "flatten",
            Self::EditMetadata => "edit-metadata",
            Self::Protect => "protect",
            Self::Unlock => "unlock",
            Self::Sanitize => "sanitize",
            Self::Linearize => "linearize",
            Self::PageNumbers => "page-numbers",
            Self::HeaderFooter => "header-footer",
            Self::Bates => "bates",
            Self::Watermark => "watermark",
            Self::Crop => "crop",
            Self::Resize => "resize",
            Self::Flip => "flip",
            Self::NUp => "n-up",
            Self::Grayscale => "grayscale",
            Self::Bookmarks => "bookmarks",
            Self::RenameByContent => "rename-by-content",
            Self::Attachments => "attachments",
            Self::PageLabels => "page-labels",
            Self::EditPdf => "edit-pdf",
            Self::Annotate => "annotate",
            Self::FillSign => "fill-sign",
            Self::SignPdf => "sign-pdf",
            Self::CreateForms => "create-forms",
            Self::PdfToWord => "pdf-to-word",
            Self::PdfToPowerpoint => "pdf-to-powerpoint",
            Self::PdfToExcel => "pdf-to-excel",
            Self::PdfToMarkdown => "pdf-to-markdown",
            Self::PdfToHtml => "pdf-to-html",
            Self::Ocr => "ocr",
            Self::Deskew => "deskew",
            Self::ScanToPdf => "scan-to-pdf",
            Self::WordToPdf => "word-to-pdf",
            Self::PowerpointToPdf => "powerpoint-to-pdf",
            Self::ExcelToPdf => "excel-to-pdf",
            Self::HtmlToPdf => "html-to-pdf",
            Self::ImagesToPdf => "images-to-pdf",
            Self::PdfToImages => "pdf-to-images",
            Self::ExtractImages => "extract-images",
            Self::PdfToText => "pdf-to-text",
            Self::TiffToPdf => "tiff-to-pdf",
            Self::PdfToTiff => "pdf-to-tiff",
        }
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ToolId {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|id| id.as_str() == s)
            .ok_or_else(|| DomainError::option("tool", format!("unknown tool \"{s}\"")))
    }
}

/// Options for any tool. The tag is the tool id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "tool", rename_all = "kebab-case")]
pub enum ToolOptions {
    /// Options for [`ToolId::Merge`].
    Merge(MergeOptions),
    /// Options for [`ToolId::AlternateMix`].
    AlternateMix(AlternateMixOptions),
    /// Options for [`ToolId::Organize`].
    Organize(OrganizeOptions),
    /// Options for [`ToolId::SplitRanges`].
    SplitRanges(SplitRangesOptions),
    /// Options for [`ToolId::SplitEveryN`].
    SplitEveryN(SplitEveryNOptions),
    /// Options for [`ToolId::ExtractPages`].
    ExtractPages(ExtractPagesOptions),
    /// Options for [`ToolId::RemovePages`].
    RemovePages(RemovePagesOptions),
    /// Options for [`ToolId::SplitByBookmarks`].
    SplitByBookmarks(SplitByBookmarksOptions),
    /// Options for [`ToolId::SplitInHalf`].
    SplitInHalf(SplitInHalfOptions),
    /// Options for [`ToolId::SplitBySize`].
    SplitBySize(SplitBySizeOptions),
    /// Options for [`ToolId::SplitByText`].
    SplitByText(SplitByTextOptions),
    /// Options for [`ToolId::Rotate`].
    Rotate(RotateOptions),
    /// Options for [`ToolId::InsertPages`].
    InsertPages(InsertPagesOptions),
    /// Options for [`ToolId::Compress`].
    Compress(CompressOptions),
    /// Options for [`ToolId::Repair`].
    Repair(RepairOptions),
    /// Options for [`ToolId::RemoveAnnotations`].
    RemoveAnnotations(RemoveAnnotationsOptions),
    /// Options for [`ToolId::Flatten`].
    Flatten(FlattenOptions),
    /// Options for [`ToolId::EditMetadata`].
    EditMetadata(MetadataOptions),
    /// Options for [`ToolId::Protect`].
    Protect(ProtectOptions),
    /// Options for [`ToolId::Unlock`].
    Unlock(UnlockOptions),
    /// Options for [`ToolId::Sanitize`].
    Sanitize(SanitizeOptions),
    /// Options for [`ToolId::Linearize`].
    Linearize(LinearizeOptions),
    /// Options for [`ToolId::PageNumbers`].
    PageNumbers(PageNumbersOptions),
    /// Options for [`ToolId::HeaderFooter`].
    HeaderFooter(HeaderFooterOptions),
    /// Options for [`ToolId::Bates`].
    Bates(BatesOptions),
    /// Options for [`ToolId::Watermark`].
    Watermark(WatermarkOptions),
    /// Options for [`ToolId::Crop`].
    Crop(CropOptions),
    /// Options for [`ToolId::Resize`].
    Resize(ResizeOptions),
    /// Options for [`ToolId::Flip`].
    Flip(FlipOptions),
    /// Options for [`ToolId::NUp`].
    NUp(NUpOptions),
    /// Options for [`ToolId::Grayscale`].
    Grayscale(GrayscaleOptions),
    /// Options for [`ToolId::Bookmarks`].
    Bookmarks(BookmarksOptions),
    /// Options for [`ToolId::RenameByContent`].
    RenameByContent(RenameByContentOptions),
    /// Options for [`ToolId::Attachments`].
    Attachments(AttachmentsOptions),
    /// Options for [`ToolId::PageLabels`].
    PageLabels(PageLabelsOptions),
    /// Options for [`ToolId::EditPdf`].
    EditPdf(EditPdfOptions),
    /// Options for [`ToolId::Annotate`].
    Annotate(AnnotateOptions),
    /// Options for [`ToolId::FillSign`].
    FillSign(FillSignOptions),
    /// Options for [`ToolId::SignPdf`].
    SignPdf(SignPdfOptions),
    /// Options for [`ToolId::CreateForms`].
    CreateForms(CreateFormsOptions),
    /// Options for [`ToolId::PdfToWord`].
    PdfToWord(PdfToWordOptions),
    /// Options for [`ToolId::PdfToPowerpoint`].
    PdfToPowerpoint(PdfToPowerPointOptions),
    /// Options for [`ToolId::PdfToExcel`].
    PdfToExcel(PdfToExcelOptions),
    /// Options for [`ToolId::PdfToMarkdown`].
    PdfToMarkdown(PdfToMarkdownOptions),
    /// Options for [`ToolId::PdfToHtml`].
    PdfToHtml(PdfToHtmlOptions),
    /// Options for [`ToolId::Ocr`].
    Ocr(OcrOptions),
    /// Options for [`ToolId::Deskew`].
    Deskew(DeskewOptions),
    /// Options for [`ToolId::ScanToPdf`].
    ScanToPdf(ScanToPdfOptions),
    /// Options for [`ToolId::WordToPdf`].
    WordToPdf(WordToPdfOptions),
    /// Options for [`ToolId::PowerpointToPdf`].
    PowerpointToPdf(PowerPointToPdfOptions),
    /// Options for [`ToolId::ExcelToPdf`].
    ExcelToPdf(ExcelToPdfOptions),
    /// Options for [`ToolId::HtmlToPdf`].
    HtmlToPdf(HtmlToPdfOptions),
    /// Options for [`ToolId::ImagesToPdf`].
    ImagesToPdf(ImagesToPdfOptions),
    /// Options for [`ToolId::PdfToImages`].
    PdfToImages(PdfToImagesOptions),
    /// Options for [`ToolId::ExtractImages`].
    ExtractImages(ExtractImagesOptions),
    /// Options for [`ToolId::PdfToText`].
    PdfToText(PdfToTextOptions),
    /// Options for [`ToolId::TiffToPdf`].
    TiffToPdf(TiffToPdfOptions),
    /// Options for [`ToolId::PdfToTiff`].
    PdfToTiff(PdfToTiffOptions),
}

impl ToolOptions {
    /// The tool these options belong to.
    pub const fn tool_id(&self) -> ToolId {
        match self {
            Self::Merge(_) => ToolId::Merge,
            Self::AlternateMix(_) => ToolId::AlternateMix,
            Self::Organize(_) => ToolId::Organize,
            Self::SplitRanges(_) => ToolId::SplitRanges,
            Self::SplitEveryN(_) => ToolId::SplitEveryN,
            Self::ExtractPages(_) => ToolId::ExtractPages,
            Self::RemovePages(_) => ToolId::RemovePages,
            Self::SplitByBookmarks(_) => ToolId::SplitByBookmarks,
            Self::SplitInHalf(_) => ToolId::SplitInHalf,
            Self::SplitBySize(_) => ToolId::SplitBySize,
            Self::SplitByText(_) => ToolId::SplitByText,
            Self::Rotate(_) => ToolId::Rotate,
            Self::InsertPages(_) => ToolId::InsertPages,
            Self::Compress(_) => ToolId::Compress,
            Self::Repair(_) => ToolId::Repair,
            Self::RemoveAnnotations(_) => ToolId::RemoveAnnotations,
            Self::Flatten(_) => ToolId::Flatten,
            Self::EditMetadata(_) => ToolId::EditMetadata,
            Self::Protect(_) => ToolId::Protect,
            Self::Unlock(_) => ToolId::Unlock,
            Self::Sanitize(_) => ToolId::Sanitize,
            Self::Linearize(_) => ToolId::Linearize,
            Self::PageNumbers(_) => ToolId::PageNumbers,
            Self::HeaderFooter(_) => ToolId::HeaderFooter,
            Self::Bates(_) => ToolId::Bates,
            Self::Watermark(_) => ToolId::Watermark,
            Self::Crop(_) => ToolId::Crop,
            Self::Resize(_) => ToolId::Resize,
            Self::Flip(_) => ToolId::Flip,
            Self::NUp(_) => ToolId::NUp,
            Self::Grayscale(_) => ToolId::Grayscale,
            Self::Bookmarks(_) => ToolId::Bookmarks,
            Self::RenameByContent(_) => ToolId::RenameByContent,
            Self::Attachments(_) => ToolId::Attachments,
            Self::PageLabels(_) => ToolId::PageLabels,
            Self::EditPdf(_) => ToolId::EditPdf,
            Self::Annotate(_) => ToolId::Annotate,
            Self::FillSign(_) => ToolId::FillSign,
            Self::SignPdf(_) => ToolId::SignPdf,
            Self::CreateForms(_) => ToolId::CreateForms,
            Self::PdfToWord(_) => ToolId::PdfToWord,
            Self::PdfToPowerpoint(_) => ToolId::PdfToPowerpoint,
            Self::PdfToExcel(_) => ToolId::PdfToExcel,
            Self::PdfToMarkdown(_) => ToolId::PdfToMarkdown,
            Self::PdfToHtml(_) => ToolId::PdfToHtml,
            Self::Ocr(_) => ToolId::Ocr,
            Self::Deskew(_) => ToolId::Deskew,
            Self::ScanToPdf(_) => ToolId::ScanToPdf,
            Self::WordToPdf(_) => ToolId::WordToPdf,
            Self::PowerpointToPdf(_) => ToolId::PowerpointToPdf,
            Self::ExcelToPdf(_) => ToolId::ExcelToPdf,
            Self::HtmlToPdf(_) => ToolId::HtmlToPdf,
            Self::ImagesToPdf(_) => ToolId::ImagesToPdf,
            Self::PdfToImages(_) => ToolId::PdfToImages,
            Self::ExtractImages(_) => ToolId::ExtractImages,
            Self::PdfToText(_) => ToolId::PdfToText,
            Self::TiffToPdf(_) => ToolId::TiffToPdf,
            Self::PdfToTiff(_) => ToolId::PdfToTiff,
        }
    }

    /// Default options for a tool, matching Appendix A.
    pub fn defaults_for(id: ToolId) -> Self {
        match id {
            ToolId::Merge => Self::Merge(MergeOptions::default()),
            ToolId::AlternateMix => Self::AlternateMix(AlternateMixOptions::default()),
            ToolId::Organize => Self::Organize(OrganizeOptions::default()),
            ToolId::SplitRanges => Self::SplitRanges(SplitRangesOptions::default()),
            ToolId::SplitEveryN => Self::SplitEveryN(SplitEveryNOptions::default()),
            ToolId::ExtractPages => Self::ExtractPages(ExtractPagesOptions::default()),
            ToolId::RemovePages => Self::RemovePages(RemovePagesOptions::default()),
            ToolId::SplitByBookmarks => Self::SplitByBookmarks(SplitByBookmarksOptions::default()),
            ToolId::SplitInHalf => Self::SplitInHalf(SplitInHalfOptions::default()),
            ToolId::SplitBySize => Self::SplitBySize(SplitBySizeOptions::default()),
            ToolId::SplitByText => Self::SplitByText(SplitByTextOptions::default()),
            ToolId::Rotate => Self::Rotate(RotateOptions::default()),
            ToolId::InsertPages => Self::InsertPages(InsertPagesOptions::default()),
            ToolId::Compress => Self::Compress(CompressOptions::default()),
            ToolId::Repair => Self::Repair(RepairOptions::default()),
            ToolId::RemoveAnnotations => {
                Self::RemoveAnnotations(RemoveAnnotationsOptions::default())
            }
            ToolId::Flatten => Self::Flatten(FlattenOptions::default()),
            ToolId::EditMetadata => Self::EditMetadata(MetadataOptions::default()),
            ToolId::Protect => Self::Protect(ProtectOptions::default()),
            ToolId::Unlock => Self::Unlock(UnlockOptions::default()),
            ToolId::Sanitize => Self::Sanitize(SanitizeOptions::default()),
            ToolId::Linearize => Self::Linearize(LinearizeOptions::default()),
            ToolId::PageNumbers => Self::PageNumbers(PageNumbersOptions::default()),
            ToolId::HeaderFooter => Self::HeaderFooter(HeaderFooterOptions::default()),
            ToolId::Bates => Self::Bates(BatesOptions::default()),
            ToolId::Watermark => Self::Watermark(WatermarkOptions::default()),
            ToolId::Crop => Self::Crop(CropOptions::default()),
            ToolId::Resize => Self::Resize(ResizeOptions::default()),
            ToolId::Flip => Self::Flip(FlipOptions::default()),
            ToolId::NUp => Self::NUp(NUpOptions::default()),
            ToolId::Grayscale => Self::Grayscale(GrayscaleOptions::default()),
            ToolId::Bookmarks => Self::Bookmarks(BookmarksOptions::default()),
            ToolId::RenameByContent => Self::RenameByContent(RenameByContentOptions::default()),
            ToolId::Attachments => Self::Attachments(AttachmentsOptions::default()),
            ToolId::PageLabels => Self::PageLabels(PageLabelsOptions::default()),
            ToolId::EditPdf => Self::EditPdf(EditPdfOptions::default()),
            ToolId::Annotate => Self::Annotate(AnnotateOptions::default()),
            ToolId::FillSign => Self::FillSign(FillSignOptions::default()),
            ToolId::SignPdf => Self::SignPdf(SignPdfOptions::default()),
            ToolId::CreateForms => Self::CreateForms(CreateFormsOptions::default()),
            ToolId::PdfToWord => Self::PdfToWord(PdfToWordOptions::default()),
            ToolId::PdfToPowerpoint => Self::PdfToPowerpoint(PdfToPowerPointOptions::default()),
            ToolId::PdfToExcel => Self::PdfToExcel(PdfToExcelOptions::default()),
            ToolId::PdfToMarkdown => Self::PdfToMarkdown(PdfToMarkdownOptions::default()),
            ToolId::PdfToHtml => Self::PdfToHtml(PdfToHtmlOptions::default()),
            ToolId::Ocr => Self::Ocr(OcrOptions::default()),
            ToolId::Deskew => Self::Deskew(DeskewOptions::default()),
            ToolId::ScanToPdf => Self::ScanToPdf(ScanToPdfOptions::default()),
            ToolId::WordToPdf => Self::WordToPdf(WordToPdfOptions::default()),
            ToolId::PowerpointToPdf => Self::PowerpointToPdf(PowerPointToPdfOptions::default()),
            ToolId::ExcelToPdf => Self::ExcelToPdf(ExcelToPdfOptions::default()),
            ToolId::HtmlToPdf => Self::HtmlToPdf(HtmlToPdfOptions::default()),
            ToolId::ImagesToPdf => Self::ImagesToPdf(ImagesToPdfOptions::default()),
            ToolId::PdfToImages => Self::PdfToImages(PdfToImagesOptions::default()),
            ToolId::ExtractImages => Self::ExtractImages(ExtractImagesOptions::default()),
            ToolId::PdfToText => Self::PdfToText(PdfToTextOptions::default()),
            ToolId::TiffToPdf => Self::TiffToPdf(TiffToPdfOptions::default()),
            ToolId::PdfToTiff => Self::PdfToTiff(PdfToTiffOptions::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_ids_are_kebab_case_and_parse_back() {
        for id in ToolId::ALL {
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(json, format!("\"{}\"", id.as_str()));
            assert_eq!(id.as_str().parse::<ToolId>().unwrap(), id);
            assert!(
                id.as_str()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-')
            );
        }
        assert!("nope".parse::<ToolId>().is_err());
    }

    #[test]
    fn options_are_tagged_by_tool_id() {
        let opts = ToolOptions::defaults_for(ToolId::Rotate);
        let json = serde_json::to_value(&opts).unwrap();
        assert_eq!(json["tool"], "rotate");
        let back: ToolOptions = serde_json::from_value(json).unwrap();
        assert_eq!(back, opts);
        assert_eq!(back.tool_id(), ToolId::Rotate);
    }
}
