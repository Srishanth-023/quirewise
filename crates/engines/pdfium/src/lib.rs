//! PDFium adapter: opening, document facts, text, and rendering.
//!
//! PDFium is a process-wide library, so it is bound once per process and shared. This crate is
//! the only place that calls into it (through `pdfium-render`) and it must run inside the worker.
#![forbid(unsafe_code)]

mod errors;
mod facts;

use application::AppError;
use application::Password;
use application::inspect::PdfiumFacts;
use application::ports::{
    DocumentInfo, PdfFacts, PdfReader, PngImage, RenderedImage, Renderer, TextExtractor,
};
use domain::{Dpi, ErrorCode, PageIndex};
use errors::map_error;
use image::ImageFormat;
use pdfium_render::prelude::*;
use std::io::Cursor;
use std::path::Path;
use std::sync::OnceLock;

static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

/// Handle to the process-wide PDFium library.
#[derive(Clone, Copy)]
pub struct PdfiumEngine {
    pdfium: &'static Pdfium,
}

impl PdfiumEngine {
    /// Loads `pdfium.dll` from `library_dir`. Later calls reuse the first successful load, since
    /// PDFium can be bound only once per process.
    pub fn load(library_dir: &Path) -> Result<Self, AppError> {
        let loaded = PDFIUM.get_or_init(|| {
            Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(library_dir))
                .map(Pdfium::new)
                .map_err(|e| e.to_string())
        });
        match loaded {
            Ok(pdfium) => Ok(Self { pdfium }),
            Err(_) => Err(AppError::new(
                ErrorCode::ComponentMissing,
                "The PDF engine (pdfium.dll) could not be loaded. Reinstall Quirewise or check that antivirus did not remove it.",
            )),
        }
    }

    pub(crate) fn open<'a>(
        &'a self,
        path: &Path,
        password: Option<&Password>,
    ) -> Result<PdfDocument<'a>, AppError> {
        self.pdfium
            .load_pdf_from_file(path, password.map(Password::expose))
            .map_err(|e| map_error(&e, password.is_some()))
    }
}

impl PdfReader for PdfiumEngine {
    fn open_info(
        &self,
        path: &Path,
        password: Option<&Password>,
    ) -> Result<DocumentInfo, AppError> {
        let doc = self.open(path, password)?;
        Ok(DocumentInfo {
            page_count: u32::try_from(doc.pages().len()).unwrap_or(0),
            encrypted: password.is_some() && facts::is_encrypted(&doc),
        })
    }
}

impl PdfFacts for PdfiumEngine {
    fn facts(&self, path: &Path, password: Option<&Password>) -> Result<PdfiumFacts, AppError> {
        let doc = self.open(path, password)?;
        Ok(facts::collect(&doc))
    }
}

impl TextExtractor for PdfiumEngine {
    fn page_text(
        &self,
        path: &Path,
        password: Option<&Password>,
        page: PageIndex,
    ) -> Result<String, AppError> {
        let doc = self.open(path, password)?;
        let page = doc
            .pages()
            .get(i32::try_from(page.get()).map_err(|_| out_of_range(page))?)
            .map_err(|e| map_error(&e, password.is_some()))?;
        let text = page.text().map_err(|e| map_error(&e, password.is_some()))?;
        Ok(text.all())
    }
}

fn out_of_range(page: PageIndex) -> AppError {
    domain::DomainError::RangeOutOfBounds {
        detail: format!("Page {} is outside this document.", page.get() + 1),
        valid: "the page count".to_owned(),
    }
    .into()
}

/// Pixel size of a page rendered so its longest edge is `max_edge`.
fn fit(width_pt: f32, height_pt: f32, max_edge: u32) -> (i32, i32) {
    let longest = width_pt.max(height_pt).max(1.0);
    let scale = max_edge as f32 / longest;
    let w = (width_pt * scale).round().max(1.0);
    let h = (height_pt * scale).round().max(1.0);
    (w as i32, h as i32)
}

impl Renderer for PdfiumEngine {
    fn render_page(
        &self,
        path: &Path,
        password: Option<&Password>,
        page: PageIndex,
        dpi: Dpi,
    ) -> Result<RenderedImage, AppError> {
        let doc = self.open(path, password)?;
        let index = i32::try_from(page.get()).map_err(|_| out_of_range(page))?;
        let page = doc
            .pages()
            .get(index)
            .map_err(|e| map_error(&e, password.is_some()))?;
        let scale = dpi.get() as f32 / 72.0;
        let (w, h) = (
            (page.width().value * scale).round().max(1.0) as i32,
            (page.height().value * scale).round().max(1.0) as i32,
        );
        let bitmap = page
            .render_with_config(&PdfRenderConfig::new().set_target_size(w, h))
            .map_err(|e| map_error(&e, password.is_some()))?;
        let rgba = bitmap
            .as_image()
            .map_err(|e| map_error(&e, password.is_some()))?
            .to_rgba8();
        Ok(RenderedImage {
            width: rgba.width(),
            height: rgba.height(),
            rgba: rgba.into_raw(),
        })
    }

    fn render_png(
        &self,
        path: &Path,
        password: Option<&Password>,
        page: PageIndex,
        max_edge: u32,
    ) -> Result<PngImage, AppError> {
        let doc = self.open(path, password)?;
        let index = i32::try_from(page.get()).map_err(|_| out_of_range(page))?;
        let page = doc
            .pages()
            .get(index)
            .map_err(|e| map_error(&e, password.is_some()))?;
        let (w, h) = fit(page.width().value, page.height().value, max_edge);
        let bitmap = page
            .render_with_config(&PdfRenderConfig::new().set_target_size(w, h))
            .map_err(|e| map_error(&e, password.is_some()))?;
        let image = bitmap
            .as_image()
            .map_err(|e| map_error(&e, password.is_some()))?;
        let mut png = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .map_err(|_| AppError::internal("encoding the page preview"))?;
        Ok(PngImage {
            width: image.width(),
            height: image.height(),
            png,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::{minimal_pdf, pdfium_dir};

    fn engine() -> PdfiumEngine {
        PdfiumEngine::load(&pdfium_dir()).expect("run scripts/fetch-pdfium.ps1 first")
    }

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn opens_a_pdf_and_counts_pages() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "a.pdf", &minimal_pdf(3));
        let info = engine().open_info(&path, None).unwrap();
        assert_eq!(
            info,
            DocumentInfo {
                page_count: 3,
                encrypted: false
            }
        );
    }

    #[test]
    fn damaged_and_non_pdf_files_report_damage() {
        let dir = tempfile::tempdir().unwrap();
        let truncated = write(&dir, "t.pdf", b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog");
        assert_eq!(
            engine().open_info(&truncated, None).unwrap_err().code,
            ErrorCode::PdfDamaged
        );
        let junk = write(&dir, "j.pdf", b"this is not a pdf at all");
        assert_eq!(
            engine().open_info(&junk, None).unwrap_err().code,
            ErrorCode::PdfDamaged
        );
        let missing = dir.path().join("missing.pdf");
        assert_eq!(
            engine().open_info(&missing, None).unwrap_err().code,
            ErrorCode::PathDenied
        );
    }

    #[test]
    fn extracts_text_per_page() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "a.pdf", &minimal_pdf(2));
        let text = engine().page_text(&path, None, PageIndex::new(1)).unwrap();
        assert!(text.contains("Page 2 of 2"), "got {text:?}");
        let err = engine()
            .page_text(&path, None, PageIndex::new(2))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::RangeOutOfBounds);
    }

    #[test]
    fn renders_a_png_within_the_edge_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "a.pdf", &minimal_pdf(1));
        let img = engine()
            .render_png(&path, None, PageIndex::new(0), 200)
            .unwrap();
        assert_eq!(&img.png[1..4], b"PNG");
        assert_eq!(
            img.height, 200,
            "Letter portrait: height is the longest edge"
        );
        assert!(img.width < 200 && img.width > 100);
        let decoded = image::load_from_memory(&img.png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (img.width, img.height));
    }

    #[test]
    fn rendered_pages_are_not_blank() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "a.pdf", &minimal_pdf(1));
        let img = engine()
            .render_page(&path, None, PageIndex::new(0), Dpi::new(72).unwrap())
            .unwrap();
        assert_eq!((img.width, img.height), (612, 792));
        let dark = img.rgba.chunks(4).filter(|p| p[0] < 128).count();
        assert!(
            dark > 50,
            "the page text should paint dark pixels, found {dark}"
        );
    }

    #[test]
    fn fit_keeps_the_aspect_ratio() {
        assert_eq!(fit(612.0, 792.0, 792), (612, 792));
        assert_eq!(fit(792.0, 612.0, 396), (396, 306));
        assert_eq!(fit(0.0, 0.0, 100), (1, 1));
    }

    #[test]
    fn facts_describe_pages_fonts_and_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(&dir, "a.pdf", &minimal_pdf(2));
        let f = engine().facts(&path, None).unwrap();
        assert_eq!(f.version, "1.7");
        assert_eq!(f.pages.len(), 2);
        assert_eq!(
            (
                f.pages[0].width_pt,
                f.pages[0].height_pt,
                f.pages[0].rotation
            ),
            (612.0, 792.0, 0)
        );
        assert!(f.has_text_layer);
        assert!(!f.has_forms);
        assert_eq!(f.signature_count, 0);
        assert_eq!(f.images.count, 0);
        assert_eq!(f.images.scanned_pages, 2);
        assert!(
            f.fonts
                .iter()
                .any(|x| x.name.contains("Helvetica") && !x.embedded),
            "fonts: {:?}",
            f.fonts
        );
    }

    #[test]
    fn password_protected_files_need_the_right_password() {
        let dir = tempfile::tempdir().unwrap();
        let plain = write(&dir, "a.pdf", &minimal_pdf(3));
        let locked = dir.path().join("locked.pdf");
        test_support::encrypt_pdf(&plain, &locked, "open");

        assert_eq!(
            engine().open_info(&locked, None).unwrap_err().code,
            ErrorCode::PdfEncrypted
        );
        let wrong = Password::new("nope");
        assert_eq!(
            engine().open_info(&locked, Some(&wrong)).unwrap_err().code,
            ErrorCode::PdfWrongPassword
        );
        let right = Password::new("open");
        let info = engine().open_info(&locked, Some(&right)).unwrap();
        assert_eq!(
            info,
            DocumentInfo {
                page_count: 3,
                encrypted: true
            }
        );
        let text = engine()
            .page_text(&locked, Some(&right), PageIndex::new(0))
            .unwrap();
        assert!(text.contains("Page 1 of 3"));
    }
}
