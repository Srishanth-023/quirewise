//! Document facts read through PDFium for the Inspect panel.

use application::inspect::{FontInfo, ImageSummary, PageInfo, PdfiumFacts};
use pdfium_render::prelude::*;
use std::collections::BTreeMap;

/// Pages whose objects are scanned for fonts and images. Larger documents are sampled from the
/// start so Inspect stays fast; `ImageSummary::scanned_pages` tells the user how many were read.
pub const OBJECT_SCAN_PAGES: usize = 500;

/// Pages checked for a text layer.
const TEXT_PROBE_PAGES: usize = 5;

/// All text on a page. `PdfPageText::all()` only looks inside the displayed page size, which misses
/// text on rotated pages and on pages whose box does not start at the origin, so this reads a
/// rectangle that covers any page.
pub fn all_text(text: &PdfPageText<'_>) -> String {
    text.inside_rect(PdfRect::new_from_values(-1.0e6, -1.0e6, 1.0e6, 1.0e6))
}

/// True when the document is encrypted. PDFium cannot name AES-256 handler revisions, which
/// surfaces as an unknown revision, so anything but "unprotected" counts as encrypted.
pub fn is_encrypted(doc: &PdfDocument<'_>) -> bool {
    !matches!(
        doc.permissions().security_handler_revision(),
        Ok(PdfSecurityHandlerRevision::Unprotected)
    )
}

fn version_text(version: PdfDocumentVersion) -> String {
    match version {
        PdfDocumentVersion::Unset => "unknown".to_owned(),
        PdfDocumentVersion::Pdf1_0 => "1.0".to_owned(),
        PdfDocumentVersion::Pdf1_1 => "1.1".to_owned(),
        PdfDocumentVersion::Pdf1_2 => "1.2".to_owned(),
        PdfDocumentVersion::Pdf1_3 => "1.3".to_owned(),
        PdfDocumentVersion::Pdf1_4 => "1.4".to_owned(),
        PdfDocumentVersion::Pdf1_5 => "1.5".to_owned(),
        PdfDocumentVersion::Pdf1_6 => "1.6".to_owned(),
        PdfDocumentVersion::Pdf1_7 => "1.7".to_owned(),
        PdfDocumentVersion::Pdf2_0 => "2.0".to_owned(),
        PdfDocumentVersion::Other(n) => format!("{}.{}", n / 10, n % 10),
    }
}

fn rotation_degrees(page: &PdfPage<'_>) -> u16 {
    match page.rotation() {
        Ok(PdfPageRenderRotation::Degrees90) => 90,
        Ok(PdfPageRenderRotation::Degrees180) => 180,
        Ok(PdfPageRenderRotation::Degrees270) => 270,
        _ => 0,
    }
}

/// Collects PDFium-derived facts. Individual failures degrade that fact instead of failing the
/// whole report, because the document itself opened successfully.
pub fn collect(doc: &PdfDocument<'_>) -> PdfiumFacts {
    let mut pages = Vec::new();
    let mut fonts: BTreeMap<String, bool> = BTreeMap::new();
    let mut images = ImageSummary::default();
    let mut has_text_layer = false;

    for (index, page) in doc.pages().iter().enumerate() {
        pages.push(PageInfo {
            number: u32::try_from(index + 1).unwrap_or(u32::MAX),
            width_pt: page.width().value,
            height_pt: page.height().value,
            rotation: rotation_degrees(&page),
        });
        if index < TEXT_PROBE_PAGES && !has_text_layer {
            has_text_layer = page
                .text()
                .map(|t| !all_text(&t).trim().is_empty())
                .unwrap_or(false);
        }
        if index < OBJECT_SCAN_PAGES {
            images.scanned_pages += 1;
            for object in page.objects().iter() {
                if let Some(text) = object.as_text_object() {
                    let font = text.font();
                    let embedded = font.is_embedded().unwrap_or(false);
                    let entry = fonts.entry(font.name()).or_insert(embedded);
                    *entry = *entry || embedded;
                } else if let Some(image) = object.as_image_object() {
                    images.count += 1;
                    if let Ok(dpi) = image.horizontal_dpi() {
                        images.min_dpi = Some(images.min_dpi.map_or(dpi, |m| m.min(dpi)));
                        images.max_dpi = Some(images.max_dpi.map_or(dpi, |m| m.max(dpi)));
                    }
                }
            }
        }
    }

    PdfiumFacts {
        version: version_text(doc.version()),
        pages,
        fonts: fonts
            .into_iter()
            .map(|(name, embedded)| FontInfo { name, embedded })
            .collect(),
        images,
        signature_count: u32::from(doc.signatures().len()),
        has_forms: doc
            .form()
            .is_some_and(|f| f.form_type() != PdfFormType::None),
        has_text_layer,
    }
}
