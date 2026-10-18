//! PDFium adapter: opening, document facts, text, and rendering.
//!
//! PDFium is a process-wide library, so it is bound once per process and shared. This crate is
//! the only place that calls into it (through `pdfium-render`) and it must run inside the worker.
#![forbid(unsafe_code)]

mod errors;
mod facts;
mod region;

use application::AppError;
use application::Password;
use application::inspect::PdfiumFacts;
use application::ports::{
    DocumentInfo, FormFlattener, OutlineItem, OutlineReader, PageSize, PdfFacts, PdfReader,
    PngImage, RenderedImage, Renderer, TextExtractor,
};
use domain::tools::common::Region;
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

    fn page_sizes(
        &self,
        path: &Path,
        password: Option<&Password>,
    ) -> Result<Vec<PageSize>, AppError> {
        let doc = self.open(path, password)?;
        Ok(doc
            .pages()
            .iter()
            .map(|page| PageSize {
                width_pt: page.width().value,
                height_pt: page.height().value,
            })
            .collect())
    }
}

impl OutlineReader for PdfiumEngine {
    fn outline(
        &self,
        path: &Path,
        password: Option<&Password>,
    ) -> Result<Vec<OutlineItem>, AppError> {
        let doc = self.open(path, password)?;
        let mut items = Vec::new();
        for bookmark in doc.bookmarks().iter() {
            let mut level = 1u32;
            let mut ancestor = bookmark.parent();
            // A cap keeps a malformed, cyclic parent chain from looping forever.
            while let Some(parent) = ancestor {
                level += 1;
                if level > 64 {
                    break;
                }
                ancestor = parent.parent();
            }
            let page = bookmark
                .destination()
                .and_then(|d| d.page_index().ok())
                .and_then(|i| u32::try_from(i).ok())
                .map(PageIndex::new);
            items.push(OutlineItem {
                title: bookmark.title().unwrap_or_default(),
                page,
                level,
            });
        }
        Ok(items)
    }
}

impl FormFlattener for PdfiumEngine {
    fn flatten_forms(
        &self,
        source: &Path,
        password: Option<&Password>,
        dest: &Path,
    ) -> Result<(), AppError> {
        let doc = self.open(source, password)?;
        for mut page in doc.pages().iter() {
            page.flatten()
                .map_err(|e| map_error(&e, password.is_some()))?;
        }
        doc.save_to_file(dest)
            .map_err(|e| map_error(&e, password.is_some()))
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
        Ok(facts::all_text(&text))
    }

    fn text_in_region(
        &self,
        path: &Path,
        password: Option<&Password>,
        page: PageIndex,
        region: &Region,
    ) -> Result<String, AppError> {
        let doc = self.open(path, password)?;
        let index = i32::try_from(page.get()).map_err(|_| out_of_range(page))?;
        let page = doc
            .pages()
            .get(index)
            .map_err(|e| map_error(&e, password.is_some()))?;
        let rotation = match page.rotation() {
            Ok(PdfPageRenderRotation::Degrees90) => 90,
            Ok(PdfPageRenderRotation::Degrees180) => 180,
            Ok(PdfPageRenderRotation::Degrees270) => 270,
            _ => 0,
        };
        // The unrotated box: text lives in user space, while page_size() is the displayed size.
        let bounds = page
            .boundaries()
            .crop()
            .or_else(|_| page.boundaries().media())
            .map(|b| b.bounds)
            .unwrap_or_else(|_| page.page_size());
        let user = region::to_user_space(
            region,
            rotation,
            region::Bounds {
                left: bounds.left().value,
                bottom: bounds.bottom().value,
                width: bounds.width().value,
                height: bounds.height().value,
            },
        );
        let rect = PdfRect::new_from_values(user.bottom, user.left, user.top, user.right);
        let text = page.text().map_err(|e| map_error(&e, password.is_some()))?;
        Ok(text.inside_rect(rect))
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

    #[test]
    fn page_sizes_follow_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = test_support::pdfs::pdf_with_geometry(&[(300, 400, 0), (300, 400, 90)]);
        let path = write(&dir, "g.pdf", &pdf);
        let sizes = engine().page_sizes(&path, None).unwrap();
        assert_eq!((sizes[0].width_pt, sizes[0].height_pt), (300.0, 400.0));
        assert_eq!(
            (sizes[1].width_pt, sizes[1].height_pt),
            (400.0, 300.0),
            "displayed size"
        );
    }

    #[test]
    fn outline_lists_bookmarks_with_levels_and_pages() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = test_support::pdfs::pdf_with_outline(
            5,
            &[("Intro", 0, 1), ("Background", 1, 2), ("Method", 3, 1)],
        );
        let path = write(&dir, "o.pdf", &pdf);
        let items = engine().outline(&path, None).unwrap();
        let rows: Vec<(String, Option<u32>, u32)> = items
            .into_iter()
            .map(|i| (i.title, i.page.map(|p| p.get()), i.level))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("Intro".to_owned(), Some(0), 1),
                ("Background".to_owned(), Some(1), 2),
                ("Method".to_owned(), Some(3), 1),
            ]
        );
        let none = write(&dir, "n.pdf", &minimal_pdf(2));
        assert!(engine().outline(&none, None).unwrap().is_empty());
    }

    #[test]
    fn text_in_region_reads_only_the_drawn_area() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = test_support::pdfs::pdf_with_texts(&["INV-1001", "INV-1002"]);
        let path = write(&dir, "t.pdf", &pdf);
        // The text is drawn near the top-left of a Letter page.
        let top_left = Region {
            x: 0.0,
            y: 0.0,
            width: 0.6,
            height: 0.15,
        };
        let text = engine()
            .text_in_region(&path, None, PageIndex::new(1), &top_left)
            .unwrap();
        assert!(text.contains("INV-1002"), "got {text:?}");
        let bottom = Region {
            x: 0.0,
            y: 0.8,
            width: 1.0,
            height: 0.2,
        };
        let none = engine()
            .text_in_region(&path, None, PageIndex::new(1), &bottom)
            .unwrap();
        assert!(none.trim().is_empty(), "got {none:?}");
    }

    #[test]
    fn text_regions_follow_page_rotation() {
        let dir = tempfile::tempdir().unwrap();
        // The page is stored upright with the label top-left, then rotated a quarter turn
        // clockwise, so on screen the label sits at the top-right.
        let pdf = test_support::pdfs::pdf_with_geometry(&[(612, 792, 90)]);
        let path = write(&dir, "r.pdf", &pdf);
        let top_right = Region {
            x: 0.7,
            y: 0.0,
            width: 0.3,
            height: 0.3,
        };
        let hit = engine()
            .text_in_region(&path, None, PageIndex::new(0), &top_right)
            .unwrap();
        assert!(hit.contains("Page 1"), "got {hit:?}");
        let top_left = Region {
            x: 0.0,
            y: 0.0,
            width: 0.3,
            height: 0.3,
        };
        let miss = engine()
            .text_in_region(&path, None, PageIndex::new(0), &top_left)
            .unwrap();
        assert!(!miss.contains("Page"), "got {miss:?}");
    }

    #[test]
    fn flattening_turns_form_fields_into_page_content() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = test_support::pdfs::pdf_with_form_field("name", "Ada Lovelace");
        let source = write(&dir, "f.pdf", &pdf);
        let dest = dir.path().join("flat.pdf");
        engine().flatten_forms(&source, None, &dest).unwrap();
        let before = engine().facts(&source, None).unwrap();
        let after = engine().facts(&dest, None).unwrap();
        assert!(before.has_forms, "the fixture has a form");
        assert!(!after.has_forms, "no interactive form is left");
        let text = engine().page_text(&dest, None, PageIndex::new(0)).unwrap();
        assert!(
            text.contains("Ada Lovelace"),
            "the value is now page text: {text:?}"
        );
    }

    #[test]
    fn regression_text_of_rotated_pages_is_extracted() {
        // PdfPageText::all() looked only inside the displayed page size and returned nothing for
        // pages turned a quarter, because their text lives in the unrotated user space.
        let dir = tempfile::tempdir().unwrap();
        let pdf = test_support::pdfs::pdf_with_geometry(&[
            (612, 792, 90),
            (612, 792, 180),
            (612, 792, 270),
        ]);
        let path = write(&dir, "rot.pdf", &pdf);
        for page in 0..3 {
            let text = engine()
                .page_text(&path, None, PageIndex::new(page))
                .unwrap();
            assert!(
                text.contains(&format!("Page {}", page + 1)),
                "page {page}: {text:?}"
            );
        }
        assert!(engine().facts(&path, None).unwrap().has_text_layer);
    }
}
