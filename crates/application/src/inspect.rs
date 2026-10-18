//! The `InspectDocument` use case (Appendix A row 71): facts about a PDF, shown in the
//! Inspect and Properties panel. It combines PDFium facts with raw structure facts and still
//! produces a useful report for password-protected files.

use domain::ErrorCode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

use crate::error::AppError;
use crate::input::validate_input;
use crate::ports::{FileStore, PdfFacts, StructureFacts};
use crate::runtime::Password;
use crate::tool::InputKind;
use domain::limits::SafetyLimits;

/// Pages listed individually in a report. Larger documents are summarized by count only.
pub const MAX_REPORTED_PAGES: usize = 5000;

/// Size and orientation of one page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PageInfo {
    /// One-based page number.
    pub number: u32,
    /// Width in PDF points (1/72 inch), before rotation.
    pub width_pt: f32,
    /// Height in PDF points, before rotation.
    pub height_pt: f32,
    /// Page rotation in degrees: 0, 90, 180 or 270.
    pub rotation: u16,
}

/// A font used on some page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FontInfo {
    /// Font name as written in the file.
    pub name: String,
    /// True when the font program is embedded.
    pub embedded: bool,
}

/// Image counts and effective resolution.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImageSummary {
    /// Number of image objects on pages.
    pub count: u32,
    /// Lowest effective resolution in dots per inch.
    pub min_dpi: Option<f32>,
    /// Highest effective resolution in dots per inch.
    pub max_dpi: Option<f32>,
    /// Pages that were scanned. Large documents are sampled from the start.
    pub scanned_pages: u32,
}

/// What an encrypted document allows (as granted to the opened view).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Permissions {
    /// Printing allowed at any quality.
    pub print: bool,
    /// Copying text and graphics allowed.
    pub copy: bool,
    /// Modifying content allowed.
    pub modify: bool,
    /// Adding or changing annotations allowed.
    pub annotate: bool,
    /// Filling form fields allowed.
    pub fill_forms: bool,
    /// Assembling (inserting, deleting, rotating pages) allowed.
    pub assemble: bool,
}

/// How a document is encrypted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EncryptionInfo {
    /// Cipher, for example `AES-256`, `AES-128` or `RC4`.
    pub method: String,
    /// Key length in bits when known.
    pub key_bits: Option<u32>,
    /// Security handler revision when known.
    pub revision: Option<u32>,
}

/// Where the bytes of the file go.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SizeBreakdown {
    /// Image streams.
    pub images: u64,
    /// Embedded font programs.
    pub fonts: u64,
    /// Page content streams.
    pub content: u64,
    /// Everything else (structure, metadata, attachments).
    pub other: u64,
}

/// A tool the app can suggest from what it learned about the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Suggestion {
    /// The file is encrypted: suggest Unlock.
    Unlock,
    /// The file has pictures of pages but no text: suggest OCR.
    Ocr,
    /// Several PDFs were dropped: suggest Merge.
    Merge,
    /// Images were dropped: suggest Images to PDF.
    ImagesToPdf,
}

/// Facts that PDFium provides.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PdfiumFacts {
    /// PDF version such as `1.7`.
    pub version: String,
    /// Pages, in order.
    pub pages: Vec<PageInfo>,
    /// Distinct fonts.
    pub fonts: Vec<FontInfo>,
    /// Image summary.
    pub images: ImageSummary,
    /// Number of signature fields.
    pub signature_count: u32,
    /// True when the document has an interactive form.
    pub has_forms: bool,
    /// True when any of the first pages has a text layer.
    pub has_text_layer: bool,
}

/// Facts read from the raw file structure.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StructureReport {
    /// Encryption details, when encrypted.
    pub encryption: Option<EncryptionInfo>,
    /// What the encryption allows, from the Encrypt dictionary (works without the password).
    pub permissions: Option<Permissions>,
    /// True when JavaScript or an automatic action is present.
    pub has_javascript: bool,
    /// PDF/A claim from the XMP metadata, for example `PDF/A-2b`.
    pub pdfa_claim: Option<String>,
    /// Byte usage by kind.
    pub size_breakdown: SizeBreakdown,
    /// Number of embedded files.
    pub embedded_files: u32,
}

/// Everything the Inspect panel shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DocumentReport {
    /// File size in bytes.
    pub file_size: u64,
    /// True when a password is needed to read the contents.
    pub locked: bool,
    /// PDF version such as `1.7`, unknown while locked.
    pub pdf_version: Option<String>,
    /// Page count, zero while locked.
    pub page_count: u32,
    /// Page sizes (capped at 5,000 entries).
    pub pages: Vec<PageInfo>,
    /// Fonts.
    pub fonts: Vec<FontInfo>,
    /// Images.
    pub images: ImageSummary,
    /// Encryption, when present.
    pub encryption: Option<EncryptionInfo>,
    /// Permissions, when encrypted and readable.
    pub permissions: Option<Permissions>,
    /// Signature fields.
    pub signature_count: u32,
    /// Interactive form present.
    pub has_forms: bool,
    /// JavaScript present (never executed by Quirewise).
    pub has_javascript: bool,
    /// PDF/A claim.
    pub pdfa_claim: Option<String>,
    /// True when pages carry text.
    pub has_text_layer: bool,
    /// Embedded files.
    pub embedded_files: u32,
    /// Where the bytes go.
    pub size_breakdown: SizeBreakdown,
    /// Suggested tools.
    pub suggestions: Vec<Suggestion>,
}

/// Suggestions for a batch of dropped files, from their kinds alone.
pub fn suggest_for_batch(kinds: &[InputKind]) -> Vec<Suggestion> {
    let pdfs = kinds.iter().filter(|k| **k == InputKind::Pdf).count();
    let images = kinds.len() - pdfs;
    let mut out = Vec::new();
    if images > 0 && pdfs == 0 {
        out.push(Suggestion::ImagesToPdf);
    }
    if pdfs >= 2 {
        out.push(Suggestion::Merge);
    }
    out
}

/// The `InspectDocument` use case.
pub struct InspectDocument {
    files: Arc<dyn FileStore>,
    facts: Arc<dyn PdfFacts>,
    structure: Arc<dyn StructureFacts>,
    limits: SafetyLimits,
}

impl InspectDocument {
    /// Wires the use case to its ports.
    pub fn new(
        files: Arc<dyn FileStore>,
        facts: Arc<dyn PdfFacts>,
        structure: Arc<dyn StructureFacts>,
        limits: SafetyLimits,
    ) -> Self {
        Self {
            files,
            facts,
            structure,
            limits,
        }
    }

    /// Inspects a PDF. A missing password on an encrypted file yields a `locked` report, not an
    /// error, so the UI can offer Unlock. A wrong password is an error.
    pub fn run(
        &self,
        path: &Path,
        password: Option<&Password>,
    ) -> Result<DocumentReport, AppError> {
        let (doc, _) = validate_input(self.files.as_ref(), path, &[InputKind::Pdf], &self.limits)?;
        let file_size = self.files.len(&doc.path)?;

        match self.facts.facts(&doc.path, password) {
            Ok(facts) => {
                // Structure facts are a bonus; PDFium already proved the file opens.
                let structure = self
                    .structure
                    .structure(&doc.path, password)
                    .unwrap_or_default();
                Ok(build_report(file_size, facts, structure))
            }
            Err(e) if e.code == ErrorCode::PdfEncrypted => {
                let structure = self
                    .structure
                    .structure(&doc.path, None)
                    .unwrap_or_default();
                Ok(locked_report(file_size, structure))
            }
            Err(e) => Err(e),
        }
    }
}

fn build_report(file_size: u64, facts: PdfiumFacts, structure: StructureReport) -> DocumentReport {
    let page_count = u32::try_from(facts.pages.len()).unwrap_or(u32::MAX);
    let mut suggestions = Vec::new();
    if page_count > 0 && !facts.has_text_layer && facts.images.count > 0 {
        suggestions.push(Suggestion::Ocr);
    }
    let mut pages = facts.pages;
    pages.truncate(MAX_REPORTED_PAGES);
    DocumentReport {
        file_size,
        locked: false,
        pdf_version: Some(facts.version),
        page_count,
        pages,
        fonts: facts.fonts,
        images: facts.images,
        encryption: structure.encryption,
        permissions: structure.permissions,
        signature_count: facts.signature_count,
        has_forms: facts.has_forms,
        has_javascript: structure.has_javascript,
        pdfa_claim: structure.pdfa_claim,
        has_text_layer: facts.has_text_layer,
        embedded_files: structure.embedded_files,
        size_breakdown: structure.size_breakdown,
        suggestions,
    }
}

fn locked_report(file_size: u64, structure: StructureReport) -> DocumentReport {
    DocumentReport {
        file_size,
        locked: true,
        pdf_version: None,
        page_count: 0,
        pages: Vec::new(),
        fonts: Vec::new(),
        images: ImageSummary::default(),
        encryption: structure.encryption,
        permissions: structure.permissions,
        signature_count: 0,
        has_forms: false,
        has_javascript: false,
        pdfa_claim: None,
        has_text_layer: false,
        embedded_files: 0,
        size_breakdown: structure.size_breakdown,
        suggestions: vec![Suggestion::Unlock],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MemoryFiles;

    struct Facts(Result<PdfiumFacts, AppError>);
    impl PdfFacts for Facts {
        fn facts(&self, _p: &Path, _pw: Option<&Password>) -> Result<PdfiumFacts, AppError> {
            self.0.clone()
        }
    }

    struct Structure(Result<StructureReport, AppError>);
    impl StructureFacts for Structure {
        fn structure(
            &self,
            _p: &Path,
            _pw: Option<&Password>,
        ) -> Result<StructureReport, AppError> {
            self.0.clone()
        }
    }

    fn files() -> Arc<MemoryFiles> {
        let f = Arc::new(MemoryFiles::default());
        f.put("C:\\a.pdf", b"%PDF-1.7 content");
        f
    }

    fn facts(text: bool, images: u32) -> PdfiumFacts {
        PdfiumFacts {
            version: "1.7".to_owned(),
            pages: vec![PageInfo {
                number: 1,
                width_pt: 612.0,
                height_pt: 792.0,
                rotation: 0,
            }],
            has_text_layer: text,
            images: ImageSummary {
                count: images,
                min_dpi: Some(72.0),
                max_dpi: Some(300.0),
                scanned_pages: 1,
            },
            ..PdfiumFacts::default()
        }
    }

    fn run(
        f: Result<PdfiumFacts, AppError>,
        s: Result<StructureReport, AppError>,
        pw: Option<&Password>,
    ) -> Result<DocumentReport, AppError> {
        InspectDocument::new(
            files(),
            Arc::new(Facts(f)),
            Arc::new(Structure(s)),
            SafetyLimits::default(),
        )
        .run(Path::new("C:\\a.pdf"), pw)
    }

    #[test]
    fn combines_pdfium_and_structure_facts() {
        let s = StructureReport {
            has_javascript: true,
            pdfa_claim: Some("PDF/A-2b".into()),
            ..StructureReport::default()
        };
        let r = run(Ok(facts(true, 0)), Ok(s), None).unwrap();
        assert!(!r.locked);
        assert_eq!(r.page_count, 1);
        assert_eq!(r.pdf_version.as_deref(), Some("1.7"));
        assert!(r.has_javascript);
        assert_eq!(r.pdfa_claim.as_deref(), Some("PDF/A-2b"));
        assert_eq!(r.file_size, 16);
        assert!(r.suggestions.is_empty());
    }

    #[test]
    fn scanned_pages_suggest_ocr() {
        let r = run(Ok(facts(false, 1)), Ok(StructureReport::default()), None).unwrap();
        assert_eq!(r.suggestions, vec![Suggestion::Ocr]);
    }

    #[test]
    fn missing_password_gives_a_locked_report_with_unlock_suggestion() {
        let enc = AppError::new(ErrorCode::PdfEncrypted, "locked");
        let s = StructureReport {
            encryption: Some(EncryptionInfo {
                method: "AES-256".into(),
                key_bits: Some(256),
                revision: Some(6),
            }),
            ..StructureReport::default()
        };
        let r = run(Err(enc), Ok(s), None).unwrap();
        assert!(r.locked);
        assert_eq!(r.page_count, 0);
        assert_eq!(r.suggestions, vec![Suggestion::Unlock]);
        assert_eq!(r.encryption.map(|e| e.method), Some("AES-256".to_owned()));
    }

    #[test]
    fn wrong_password_and_damage_are_errors() {
        for code in [ErrorCode::PdfWrongPassword, ErrorCode::PdfDamaged] {
            let e = run(
                Err(AppError::new(code, "x")),
                Ok(StructureReport::default()),
                None,
            )
            .unwrap_err();
            assert_eq!(e.code, code);
        }
    }

    #[test]
    fn structure_failures_degrade_instead_of_failing() {
        let r = run(
            Ok(facts(true, 0)),
            Err(AppError::new(ErrorCode::PdfDamaged, "x")),
            None,
        )
        .unwrap();
        assert!(!r.has_javascript);
    }

    #[test]
    fn non_pdf_input_is_rejected_before_any_parser() {
        let f = Arc::new(MemoryFiles::default());
        f.put("C:\\a.zip", b"PK\x03\x04");
        let uc = InspectDocument::new(
            f,
            Arc::new(Facts(Ok(facts(true, 0)))),
            Arc::new(Structure(Ok(StructureReport::default()))),
            SafetyLimits::default(),
        );
        assert_eq!(
            uc.run(Path::new("C:\\a.zip"), None).unwrap_err().code,
            ErrorCode::InputNotPdf
        );
    }

    #[test]
    fn batch_suggestions_follow_input_kinds() {
        use InputKind::{Image, Pdf};
        assert_eq!(
            suggest_for_batch(&[Image, Image]),
            vec![Suggestion::ImagesToPdf]
        );
        assert_eq!(suggest_for_batch(&[Pdf, Pdf]), vec![Suggestion::Merge]);
        assert!(suggest_for_batch(&[Pdf]).is_empty());
        assert!(suggest_for_batch(&[Pdf, Image]).is_empty());
    }
}
