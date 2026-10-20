//! Options of the tools that turn a PDF into an editable document: PDF to Word (row 25), PDF to
//! PowerPoint (row 26), PDF to Excel (row 27), PDF to Markdown (row 29) and PDF to HTML (row 67).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::layout::Detection;
use crate::page_range::PageRange;
use crate::tools::ai::AiOptions;
use crate::tools::recognize::{OCR_DPI_RANGE, check_languages};
use crate::units::Dpi;

/// Resolution scanned pages are read at for these tools.
pub const EXPORT_OCR_DPI: u32 = 300;

/// Whether pages without text are read with OCR first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ExportOcr {
    /// Read the pages that have no text, leave the others (default).
    #[default]
    Auto,
    /// Never read pages; a page without text becomes a picture or stays empty.
    Off,
    /// Read every page, ignoring the text it has.
    Always,
}

fn default_languages() -> Vec<String> {
    vec!["eng".to_owned()]
}

fn check_ocr(ocr: ExportOcr, languages: &[String]) -> Result<(), DomainError> {
    if ocr == ExportOcr::Off {
        Ok(())
    } else {
        check_languages(languages)
    }
}

/// How the Word document is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum WordLayout {
    /// Headings, paragraphs, lists, tables and pictures that flow and can be edited freely (default).
    #[default]
    Flowing,
    /// Every block is placed where it was on the page, in a frame.
    Exact,
}

/// Options for PDF to Word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PdfToWordOptions {
    /// Flowing text or exact positions. Default flowing.
    pub layout: WordLayout,
    /// Read pages that have no text. Default auto.
    pub ocr: ExportOcr,
    /// Languages for OCR. Default `eng`.
    pub languages: Vec<String>,
    /// Pages to convert. Default `all`.
    pub pages: PageRange,
}

impl Default for PdfToWordOptions {
    fn default() -> Self {
        Self {
            layout: WordLayout::Flowing,
            ocr: ExportOcr::Auto,
            languages: default_languages(),
            pages: PageRange::all(),
        }
    }
}

impl PdfToWordOptions {
    /// Checks the OCR languages.
    pub fn validate(&self) -> Result<(), DomainError> {
        check_ocr(self.ocr, &self.languages)
    }
}

/// What a slide holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SlideMode {
    /// Text boxes, tables and pictures that can be edited (default).
    #[default]
    EditableText,
    /// The page as one picture; looks exact, cannot be edited.
    ImageSlides,
}

/// Options for PDF to PowerPoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PdfToPowerPointOptions {
    /// Editable text boxes or one picture per slide. Default editable text.
    pub mode: SlideMode,
    /// Resolution of the pictures for image slides, 100 to 600. Default 150.
    pub dpi: Dpi,
    /// Read pages that have no text, for editable text. Default auto.
    pub ocr: ExportOcr,
    /// Languages for OCR. Default `eng`.
    pub languages: Vec<String>,
    /// Pages to convert. Default `all`.
    pub pages: PageRange,
}

impl Default for PdfToPowerPointOptions {
    fn default() -> Self {
        Self {
            mode: SlideMode::EditableText,
            dpi: Dpi::new(150).unwrap_or_default(),
            ocr: ExportOcr::Auto,
            languages: default_languages(),
            pages: PageRange::all(),
        }
    }
}

impl PdfToPowerPointOptions {
    /// Checks the resolution and the OCR languages.
    pub fn validate(&self) -> Result<(), DomainError> {
        let (low, high) = OCR_DPI_RANGE;
        if !(low..=high).contains(&self.dpi.get()) {
            return Err(DomainError::option(
                "dpi",
                format!("must be between {low} and {high}"),
            ));
        }
        if self.mode == SlideMode::EditableText {
            check_ocr(self.ocr, &self.languages)?;
        }
        Ok(())
    }
}

/// What PDF to Excel writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SheetFormat {
    /// One workbook (default).
    #[default]
    Xlsx,
    /// One CSV file per table.
    Csv,
}

/// How tables are spread over sheets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SheetGrouping {
    /// One sheet per table (default).
    #[default]
    PerTable,
    /// One sheet per page, its tables one under the other.
    PerPage,
}

/// Options for PDF to Excel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PdfToExcelOptions {
    /// A workbook or CSV files. Default workbook.
    pub format: SheetFormat,
    /// Sheets per table or per page. Default per table.
    pub grouping: SheetGrouping,
    /// How tables are found. Default auto.
    pub detection: Detection,
    /// Read pages that have no text. Default auto.
    pub ocr: ExportOcr,
    /// Languages for OCR. Default `eng`.
    pub languages: Vec<String>,
    /// Pages to search for tables. Default `all`.
    pub pages: PageRange,
}

impl Default for PdfToExcelOptions {
    fn default() -> Self {
        Self {
            format: SheetFormat::Xlsx,
            grouping: SheetGrouping::PerTable,
            detection: Detection::Auto,
            ocr: ExportOcr::Auto,
            languages: default_languages(),
            pages: PageRange::all(),
        }
    }
}

impl PdfToExcelOptions {
    /// Checks the OCR languages.
    pub fn validate(&self) -> Result<(), DomainError> {
        check_ocr(self.ocr, &self.languages)
    }
}

/// Options for PDF to Markdown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PdfToMarkdownOptions {
    /// Save the pictures next to the file and link them. Default on.
    pub images: bool,
    /// Read pages that have no text. Default auto.
    pub ocr: ExportOcr,
    /// Languages for OCR. Default `eng`.
    pub languages: Vec<String>,
    /// Pages to convert. Default `all`.
    pub pages: PageRange,
    /// Let the local model tidy the text: join lines that were broken by the page, mend
    /// hyphenated words and fix the heading levels. Words are never added or removed. Default off.
    pub ai_cleanup: bool,
    /// The model for `ai_cleanup`.
    pub ai: AiOptions,
}

impl Default for PdfToMarkdownOptions {
    fn default() -> Self {
        Self {
            images: true,
            ocr: ExportOcr::Auto,
            languages: default_languages(),
            pages: PageRange::all(),
            ai_cleanup: false,
            ai: AiOptions::default(),
        }
    }
}

impl PdfToMarkdownOptions {
    /// Checks the OCR languages and, when the model is used, the model.
    pub fn validate(&self) -> Result<(), DomainError> {
        check_ocr(self.ocr, &self.languages)?;
        if self.ai_cleanup {
            self.ai.validate()?;
        }
        Ok(())
    }
}

/// Options for PDF to HTML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PdfToHtmlOptions {
    /// Put the pictures inside the page, so it is one file. Default on.
    pub inline_images: bool,
    /// Read pages that have no text. Default auto.
    pub ocr: ExportOcr,
    /// Languages for OCR. Default `eng`.
    pub languages: Vec<String>,
    /// Pages to convert. Default `all`.
    pub pages: PageRange,
}

impl Default for PdfToHtmlOptions {
    fn default() -> Self {
        Self {
            inline_images: true,
            ocr: ExportOcr::Auto,
            languages: default_languages(),
            pages: PageRange::all(),
        }
    }
}

impl PdfToHtmlOptions {
    /// Checks the OCR languages.
    pub fn validate(&self) -> Result<(), DomainError> {
        check_ocr(self.ocr, &self.languages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_json_names_are_kebab_case() {
        assert!(PdfToWordOptions::default().validate().is_ok());
        assert!(PdfToPowerPointOptions::default().validate().is_ok());
        assert!(PdfToExcelOptions::default().validate().is_ok());
        assert!(PdfToMarkdownOptions::default().validate().is_ok());
        assert!(PdfToHtmlOptions::default().validate().is_ok());
        let o: PdfToPowerPointOptions =
            serde_json::from_str(r#"{"mode":"image-slides","ocr":"off","dpi":200}"#).unwrap();
        assert_eq!(o.mode, SlideMode::ImageSlides);
        assert_eq!(o.ocr, ExportOcr::Off);
        let e: PdfToExcelOptions =
            serde_json::from_str(r#"{"format":"csv","grouping":"per-page","detection":"lattice"}"#)
                .unwrap();
        assert_eq!(e.format, SheetFormat::Csv);
        assert_eq!(e.grouping, SheetGrouping::PerPage);
        assert_eq!(e.detection, Detection::Lattice);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(serde_json::from_str::<PdfToWordOptions>(r#"{"colour":1}"#).is_err());
    }

    #[test]
    fn languages_are_checked_unless_ocr_is_off() {
        let bad = PdfToWordOptions {
            languages: vec!["EN".to_owned()],
            ..PdfToWordOptions::default()
        };
        assert!(bad.validate().is_err());
        let off = PdfToWordOptions {
            ocr: ExportOcr::Off,
            ..bad
        };
        assert!(off.validate().is_ok());
    }

    #[test]
    fn the_slide_resolution_is_bounded() {
        let low = PdfToPowerPointOptions {
            dpi: Dpi::new(50).unwrap_or_default(),
            ..PdfToPowerPointOptions::default()
        };
        assert!(low.validate().is_err());
    }
}
