//! What is on a page, for the tools that rebuild it as another document: characters with their
//! style, the lines that may be table borders, and the pictures, all in points on the page as it
//! is displayed (origin top left, y down).

use application::AppError;
use application::Password;
use application::ports::{LayoutReader, PictureBytes};
use domain::PageIndex;
use domain::layout::{CharBox, PageContent, PictureBox, Rect, Rule, words_from_chars};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ExtendedColorType, ImageEncoder};
use pdfium_render::prelude::*;
use std::path::Path;

use crate::PdfiumEngine;
use crate::errors::map_error;
use crate::out_of_range;

/// Lines thicker than this many points are not table borders but shapes.
const THIN: f32 = 2.5;
/// Lines shorter than this many points are not table borders.
const MIN_LINE: f32 = 6.0;
/// Pictures smaller than this in either direction are decoration.
const MIN_PICTURE: f32 = 4.0;
/// Longest edge of a picture handed to a document, in pixels.
const MAX_EDGE: u32 = 2400;
/// Opaque pictures with at least this many pixels are stored as JPEG.
const JPEG_FROM_PIXELS: u64 = 160_000;

/// Where the page's user space sits on the displayed page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    /// Left edge of the unrotated box.
    pub left: f32,
    /// Bottom edge of the unrotated box.
    pub bottom: f32,
    /// Width of the unrotated box.
    pub width: f32,
    /// Height of the unrotated box.
    pub height: f32,
    /// Clockwise turn: 0, 90, 180 or 270.
    pub rotation: u16,
}

impl Frame {
    /// A point in user space as a point on the displayed page.
    pub(crate) fn point(&self, x: f32, y: f32) -> (f32, f32) {
        let (ux, uy) = (x - self.left, y - self.bottom);
        match self.rotation {
            90 => (uy, ux),
            180 => (self.width - ux, uy),
            270 => (self.height - uy, self.width - ux),
            _ => (ux, self.height - uy),
        }
    }

    /// A box in user space as a box on the displayed page.
    pub(crate) fn rect(&self, left: f32, bottom: f32, right: f32, top: f32) -> Rect {
        let (ax, ay) = self.point(left, bottom);
        let (bx, by) = self.point(right, top);
        Rect::new(ax, ay, bx, by)
    }

    /// Width and height of the displayed page.
    pub(crate) fn displayed(&self) -> (f32, f32) {
        if self.rotation == 90 || self.rotation == 270 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }
}

fn frame_of(page: &PdfPage<'_>) -> Frame {
    let rotation = match page.rotation() {
        Ok(PdfPageRenderRotation::Degrees90) => 90,
        Ok(PdfPageRenderRotation::Degrees180) => 180,
        Ok(PdfPageRenderRotation::Degrees270) => 270,
        _ => 0,
    };
    let bounds = page
        .boundaries()
        .crop()
        .or_else(|_| page.boundaries().media())
        .map(|b| b.bounds)
        .unwrap_or_else(|_| page.page_size());
    Frame {
        left: bounds.left().value,
        bottom: bounds.bottom().value,
        width: bounds.width().value,
        height: bounds.height().value,
        rotation,
    }
}

fn is_bold(weight: Option<PdfFontWeight>, name: &str) -> bool {
    let by_weight = match weight {
        Some(
            PdfFontWeight::Weight600
            | PdfFontWeight::Weight700Bold
            | PdfFontWeight::Weight800
            | PdfFontWeight::Weight900,
        ) => true,
        Some(PdfFontWeight::Custom(w)) => w >= 600,
        _ => false,
    };
    let lower = name.to_ascii_lowercase();
    by_weight
        || ["bold", "black", "heavy", "semibold"]
            .iter()
            .any(|k| lower.contains(k))
}

fn is_italic(flag: bool, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    flag || lower.contains("italic") || lower.contains("oblique")
}

/// A line as a thin box. Zero-thickness lines get a little body so touching tests work.
fn thin_rule(rect: Rect) -> Rule {
    let mut r = rect;
    if r.height() < 0.5 {
        let mid = r.mid_y();
        r.y0 = mid - 0.25;
        r.y1 = mid + 0.25;
    }
    if r.width() < 0.5 {
        let mid = r.mid_x();
        r.x0 = mid - 0.25;
        r.x1 = mid + 0.25;
    }
    Rule { rect: r }
}

/// The horizontal and vertical lines a path draws.
fn path_rules(path: &PdfPagePathObject<'_>, frame: &Frame, out: &mut Vec<Rule>) {
    let Ok(bounds) = path.bounds() else {
        return;
    };
    let (w, h) = (bounds.width().value, bounds.height().value);
    if w.min(h) <= THIN {
        // One line or one bar.
        if w.max(h) >= MIN_LINE {
            out.push(thin_rule(frame.rect(
                bounds.left().value,
                bounds.bottom().value,
                bounds.right().value,
                bounds.top().value,
            )));
        }
        return;
    }
    // A large shape is a border only when it is stroked; a plain fill is shading.
    if !path.is_stroked().unwrap_or(false) {
        return;
    }
    let matrix = path.matrix().unwrap_or(PdfMatrix::IDENTITY);
    let segments = path.segments().transform(matrix);
    let mut current: Option<(f32, f32)> = None;
    let mut start: Option<(f32, f32)> = None;
    let mut line = |from: (f32, f32), to: (f32, f32)| {
        let rect = frame.rect(from.0, from.1, to.0, to.1);
        let flat = rect.height() <= 0.5 || rect.width() <= 0.5;
        if flat && rect.width().max(rect.height()) >= MIN_LINE {
            out.push(thin_rule(rect));
        }
    };
    for i in 0..segments.len() {
        let Ok(segment) = segments.get(i) else {
            continue;
        };
        let (x, y) = segment.point();
        let point = (x.value, y.value);
        match segment.segment_type() {
            PdfPathSegmentType::MoveTo => {
                current = Some(point);
                start = Some(point);
            }
            PdfPathSegmentType::LineTo => {
                if let Some(from) = current {
                    line(from, point);
                }
                current = Some(point);
            }
            PdfPathSegmentType::BezierTo => current = Some(point),
            PdfPathSegmentType::Unknown => {}
        }
        if segment.is_close()
            && let (Some(from), Some(to)) = (current, start)
        {
            line(from, to);
            current = Some(to);
        }
    }
}

fn encode(image: &DynamicImage) -> Result<PictureBytes, AppError> {
    let fail = || AppError::internal("encoding a picture of the page");
    let image = if image.width().max(image.height()) > MAX_EDGE {
        image.resize(MAX_EDGE, MAX_EDGE, FilterType::Triangle)
    } else {
        image.clone()
    };
    let (width, height) = (image.width(), image.height());
    let opaque = !image.color().has_alpha();
    let mut bytes = Vec::new();
    let jpeg = opaque && u64::from(width) * u64::from(height) >= JPEG_FROM_PIXELS;
    if jpeg {
        let rgb = image.to_rgb8();
        JpegEncoder::new_with_quality(&mut bytes, 85)
            .write_image(&rgb, width, height, ExtendedColorType::Rgb8)
            .map_err(|_| fail())?;
    } else {
        let rgba = image.to_rgba8();
        PngEncoder::new(&mut bytes)
            .write_image(&rgba, width, height, ExtendedColorType::Rgba8)
            .map_err(|_| fail())?;
    }
    Ok(PictureBytes {
        width,
        height,
        jpeg,
        bytes,
    })
}

impl LayoutReader for PdfiumEngine {
    fn page_content(
        &self,
        path: &Path,
        password: Option<&Password>,
        page: PageIndex,
    ) -> Result<PageContent, AppError> {
        let doc = self.open(path, password)?;
        let index = i32::try_from(page.get()).map_err(|_| out_of_range(page))?;
        let pdf_page = doc
            .pages()
            .get(index)
            .map_err(|e| map_error(&e, password.is_some()))?;
        let frame = frame_of(&pdf_page);
        let (width, height) = frame.displayed();

        let mut chars: Vec<CharBox> = Vec::new();
        let text = pdf_page
            .text()
            .map_err(|e| map_error(&e, password.is_some()))?;
        for c in text.chars().iter() {
            let (Some(ch), Ok(bounds)) = (c.unicode_char(), c.loose_bounds()) else {
                continue;
            };
            let name = c.font_name();
            chars.push(CharBox {
                ch,
                rect: frame.rect(
                    bounds.left().value,
                    bounds.bottom().value,
                    bounds.right().value,
                    bounds.top().value,
                ),
                size: c.scaled_font_size().value,
                bold: is_bold(c.font_weight(), &name),
                italic: is_italic(c.font_is_italic(), &name),
            });
        }

        let mut rules: Vec<Rule> = Vec::new();
        let mut pictures: Vec<PictureBox> = Vec::new();
        let mut picture_index = 0usize;
        for object in pdf_page.objects().iter() {
            if let Some(path_object) = object.as_path_object() {
                path_rules(path_object, &frame, &mut rules);
            } else if object.as_image_object().is_some() {
                if let Ok(bounds) = object.bounds() {
                    let rect = frame.rect(
                        bounds.left().value,
                        bounds.bottom().value,
                        bounds.right().value,
                        bounds.top().value,
                    );
                    if rect.width() >= MIN_PICTURE && rect.height() >= MIN_PICTURE {
                        pictures.push(PictureBox {
                            rect,
                            index: picture_index,
                        });
                    }
                }
                picture_index += 1;
            }
        }

        Ok(PageContent {
            number: page.get() + 1,
            width,
            height,
            words: words_from_chars(&chars),
            rules,
            pictures,
        })
    }

    fn picture(
        &self,
        path: &Path,
        password: Option<&Password>,
        page: PageIndex,
        index: usize,
    ) -> Result<PictureBytes, AppError> {
        let doc = self.open(path, password)?;
        let at = i32::try_from(page.get()).map_err(|_| out_of_range(page))?;
        let pdf_page = doc
            .pages()
            .get(at)
            .map_err(|e| map_error(&e, password.is_some()))?;
        let object = pdf_page
            .objects()
            .iter()
            .filter(|o| o.as_image_object().is_some())
            .nth(index)
            .ok_or_else(|| AppError::internal("finding a picture of the page"))?;
        let Some(image_object) = object.as_image_object() else {
            return Err(AppError::internal("finding a picture of the page"));
        };
        let image = image_object
            .get_processed_image(&doc)
            .or_else(|_| image_object.get_raw_image())
            .map_err(|e| map_error(&e, password.is_some()))?;
        encode(&image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::{Bounds, to_user_space};
    use domain::tools::common::Region;

    #[test]
    fn displayed_points_round_trip_through_every_rotation() {
        let page = Bounds {
            left: 0.0,
            bottom: 0.0,
            width: 600.0,
            height: 800.0,
        };
        for rotation in [0u16, 90, 180, 270] {
            let frame = Frame {
                left: 0.0,
                bottom: 0.0,
                width: 600.0,
                height: 800.0,
                rotation,
            };
            let (dw, dh) = frame.displayed();
            // A tiny region at a displayed point, mapped to user space and back.
            let (x, y) = (dw * 0.2, dh * 0.7);
            let region = Region {
                x: x / dw,
                y: y / dh,
                width: 0.0001,
                height: 0.0001,
            };
            let user = to_user_space(&region, rotation, page);
            let (bx, by) = frame.point(user.left, user.bottom);
            let (cx, cy) = frame.point(user.right, user.top);
            let (mx, my) = ((bx + cx) / 2.0, (by + cy) / 2.0);
            assert!(
                (mx - x).abs() < 0.5 && (my - y).abs() < 0.5,
                "rotation {rotation}: ({mx}, {my}) instead of ({x}, {y})"
            );
        }
    }

    fn engine() -> PdfiumEngine {
        PdfiumEngine::load(&test_support::pdfium_dir()).unwrap()
    }

    fn content_of(pdf: &[u8], rotate: u32) -> PageContent {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("page.pdf");
        std::fs::write(&path, pdf).unwrap();
        let _ = rotate;
        engine()
            .page_content(&path, None, PageIndex::new(0))
            .unwrap()
    }

    #[test]
    fn words_come_with_boxes_sizes_and_styles() {
        let pdf = test_support::pdfs::pdf_from_content(
            "BT /F1 12 Tf 50 700 Td (Plain words) Tj /F2 12 Tf ( bold) Tj /F3 12 Tf ( slanted) Tj ET\nBT /F1 24 Tf 50 650 Td (Big) Tj ET",
            0,
        );
        let content = content_of(&pdf, 0);
        assert_eq!((content.width, content.height), (612.0, 792.0));
        let by = |t: &str| content.words.iter().find(|w| w.text == t).cloned().unwrap();
        let plain = by("Plain");
        // 700 from the bottom of a 792 high page is about 92 from the top, minus the ascent.
        assert!(plain.rect.y0 > 70.0 && plain.rect.y1 < 110.0, "{plain:?}");
        assert!((plain.rect.x0 - 50.0).abs() < 1.0);
        assert!(!plain.bold && !plain.italic);
        assert!(by("bold").bold);
        assert!(by("slanted").italic);
        assert!((by("Big").size - 24.0).abs() < 0.5);
    }

    #[test]
    fn lines_bars_and_outlines_become_rules_but_fills_do_not() {
        let content_stream = "0.5 w 50 600 m 250 600 l S\n\
                              50 500 m 50 600 l S\n\
                              0 0 1 rg 300 500 100 50 re f\n\
                              0 g 100 300 150 2 re f\n\
                              0 G 300 300 100 80 re S";
        let pdf = test_support::pdfs::pdf_from_content(content_stream, 0);
        let content = content_of(&pdf, 0);
        let horizontal = content
            .rules
            .iter()
            .filter(|r| r.rect.width() > r.rect.height())
            .count();
        let vertical = content
            .rules
            .iter()
            .filter(|r| r.rect.height() > r.rect.width())
            .count();
        // Lines: one horizontal, one vertical, one thin bar, and the four sides of the outline.
        assert_eq!(horizontal, 4, "{:?}", content.rules);
        assert_eq!(vertical, 3, "{:?}", content.rules);
        // The blue fill is shading and gives nothing: no rule is as big as the fill.
        assert!(
            content
                .rules
                .iter()
                .all(|r| r.rect.height() < 3.0 || r.rect.width() < 3.0)
        );
    }

    #[test]
    fn a_picture_has_its_box_and_can_be_read_back() {
        let pdf = test_support::pdfs::pdf_from_content("q 100 0 0 100 200 400 cm /Im1 Do Q", 0);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pic.pdf");
        std::fs::write(&path, &pdf).unwrap();
        let content = engine()
            .page_content(&path, None, PageIndex::new(0))
            .unwrap();
        assert_eq!(content.pictures.len(), 1);
        let picture = content.pictures[0];
        assert!(
            (picture.rect.width() - 100.0).abs() < 0.5
                && (picture.rect.height() - 100.0).abs() < 0.5
        );
        assert!((picture.rect.x0 - 200.0).abs() < 0.5 && (picture.rect.y0 - 292.0).abs() < 0.5);
        let bytes = engine()
            .picture(&path, None, PageIndex::new(0), picture.index)
            .unwrap();
        assert_eq!((bytes.width, bytes.height), (2, 2));
        assert!(!bytes.jpeg && bytes.bytes.starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(engine().picture(&path, None, PageIndex::new(0), 5).is_err());
    }

    #[test]
    fn a_rotated_page_is_read_as_it_is_displayed() {
        // The page is turned a quarter clockwise, and its text is drawn turned the other way, as
        // a landscape scan is: on screen the word reads left to right at (50, 100) from the top left.
        let pdf = test_support::pdfs::pdf_from_content(
            "BT /F1 12 Tf 0 1 -1 0 100 50 Tm (Corner) Tj ET",
            90,
        );
        let content = content_of(&pdf, 90);
        assert_eq!((content.width, content.height), (792.0, 612.0));
        let word = content.words.iter().find(|w| w.text == "Corner").unwrap();
        assert!((word.rect.x0 - 50.0).abs() < 1.5, "{word:?}");
        assert!(word.rect.y0 > 80.0 && word.rect.y1 < 112.0, "{word:?}");
    }
}
