//! Drawing text and pictures on pages: the shared stamper behind Page numbers, Header and
//! footer, Bates numbering and Watermark.
//!
//! Stamps are laid out in the page as displayed (origin at the bottom left, y up) and moved into
//! the page's user space with one `cm`, so rotated pages and crop boxes need no special cases.
//! Text uses the standard fonts, which every reader has, so nothing is embedded.

use application::AppError;
use domain::ErrorCode;
use domain::fonts::StandardFont;
use domain::stamp::{Layer, PageStamps, Stamp, StampContent, TextSpec};
use image::imageops::FilterType;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::boxes::{display_matrix, displayed_size};
use crate::pageres::{
    add_named, cm, effective_resources, num, page_object, set_resources, wrap_content,
};

/// Height of a line of capitals relative to the font size, used to place text vertically.
const CAP_HEIGHT: f32 = 0.72;
/// Pictures larger than this on their longer side are reduced before they are embedded.
const MAX_PICTURE_EDGE: u32 = 1600;
/// Largest picture file read.
const MAX_PICTURE_BYTES: u64 = 50 * 1024 * 1024;
/// Upper bound of copies when a stamp is tiled across a page.
const MAX_TILES: usize = 2000;

/// A picture ready to draw: its XObject and size in pixels.
#[derive(Clone, Copy)]
pub(crate) struct Picture {
    pub(crate) id: ObjectId,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

fn bad_picture() -> AppError {
    AppError::new(
        ErrorCode::OptionInvalid,
        "The picture could not be read. Use a PNG or JPEG file that opens in an image viewer.",
    )
}

pub(crate) fn load_picture(doc: &mut Document, path: &Path) -> Result<Picture, AppError> {
    let size = std::fs::metadata(path)
        .map_err(|e| AppError::from_io("read the picture", &e))?
        .len();
    if size > MAX_PICTURE_BYTES {
        return Err(AppError::new(
            ErrorCode::OptionInvalid,
            "The picture file is larger than 50 MB. Use a smaller picture.",
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| AppError::from_io("read the picture", &e))?;
    load_picture_bytes(doc, &bytes)
}

/// Writes a PNG or JPEG file's content into the document as an image object.
pub(crate) fn load_picture_bytes(doc: &mut Document, bytes: &[u8]) -> Result<Picture, AppError> {
    let decoded = image::load_from_memory(bytes).map_err(|_| bad_picture())?;
    let decoded = if decoded.width().max(decoded.height()) > MAX_PICTURE_EDGE {
        decoded.resize(MAX_PICTURE_EDGE, MAX_PICTURE_EDGE, FilterType::Triangle)
    } else {
        decoded
    };
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return Err(bad_picture());
    }
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    let mut alpha = Vec::with_capacity(width as usize * height as usize);
    for px in rgba.pixels() {
        rgb.extend_from_slice(&px.0[..3]);
        alpha.push(px.0[3]);
    }
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Image".to_vec()));
    dict.set("Width", Object::Integer(i64::from(width)));
    dict.set("Height", Object::Integer(i64::from(height)));
    dict.set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
    dict.set("BitsPerComponent", Object::Integer(8));
    if alpha.iter().any(|a| *a != 255) {
        let mut mask = Dictionary::new();
        mask.set("Type", Object::Name(b"XObject".to_vec()));
        mask.set("Subtype", Object::Name(b"Image".to_vec()));
        mask.set("Width", Object::Integer(i64::from(width)));
        mask.set("Height", Object::Integer(i64::from(height)));
        mask.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
        mask.set("BitsPerComponent", Object::Integer(8));
        let mask_id = doc.add_object(Stream::new(mask, alpha));
        dict.set("SMask", Object::Reference(mask_id));
    }
    let id = doc.add_object(Stream::new(dict, rgb));
    Ok(Picture { id, width, height })
}

/// A PDF string literal for WinAnsi bytes.
pub(crate) fn literal(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('(');
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(char::from(b));
            }
            0x20..=0x7E => out.push(char::from(b)),
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out.push(')');
    out
}

/// The size of the thing a stamp draws, in points.
fn content_size(content: &StampContent, picture: Option<Picture>, page_w: f32) -> (f32, f32) {
    match content {
        StampContent::Text(t) => (
            t.font.text_width(&t.text, t.size_pt),
            t.size_pt * CAP_HEIGHT,
        ),
        StampContent::Image { width_fraction, .. } => {
            let (pw, ph) = picture.map_or((1, 1), |p| (p.width, p.height));
            let w = page_w * width_fraction;
            (w, w * ph as f32 / pw as f32)
        }
    }
}

/// Centers of the boxes a stamp is drawn at: one, or a grid over the page when tiled.
fn centers(stamp: &Stamp, page: (f32, f32), size: (f32, f32)) -> Vec<(f32, f32)> {
    let (page_w, page_h) = page;
    let (w, h) = size;
    let (hx, vy) = (stamp.position.horizontal(), stamp.position.vertical());
    let x = match hx {
        -1 => stamp.margin_pt,
        0 => (page_w - w) / 2.0,
        _ => page_w - stamp.margin_pt - w,
    } + stamp.offset_x_pt;
    let y = match vy {
        -1 => stamp.margin_pt,
        0 => (page_h - h) / 2.0,
        _ => page_h - stamp.margin_pt - h,
    } + stamp.offset_y_pt;
    let first = (x + w / 2.0, y + h / 2.0);
    if !stamp.tile || w <= 0.0 || h <= 0.0 {
        return vec![first];
    }
    let is_text = matches!(stamp.content, StampContent::Text(_));
    let step_x = if is_text { w * 1.5 + h } else { w * 1.25 };
    let step_y = if is_text { h * 3.0 } else { h * 1.25 };
    let reach = (page_w.hypot(page_h) / 2.0) + w.max(h);
    let nx = (reach / step_x).ceil() as i32 + 1;
    let ny = (reach / step_y).ceil() as i32 + 1;
    let mut out = Vec::new();
    for j in -ny..=ny {
        for i in -nx..=nx {
            let c = (first.0 + i as f32 * step_x, first.1 + j as f32 * step_y);
            let inside = c.0 > -(w + h)
                && c.0 < page_w + (w + h)
                && c.1 > -(w + h)
                && c.1 < page_h + (w + h);
            if inside && out.len() < MAX_TILES {
                out.push(c);
            }
        }
    }
    out
}

/// The resources a stamp's content needs on one page.
struct PageParts<'a> {
    fonts: HashMap<StandardFont, String>,
    opacities: HashMap<u32, String>,
    pictures: HashMap<&'a Path, String>,
}

fn font_object(
    doc: &mut Document,
    cache: &mut HashMap<StandardFont, ObjectId>,
    f: StandardFont,
) -> ObjectId {
    *cache.entry(f).or_insert_with(|| {
        let mut dict = Dictionary::new();
        dict.set("Type", Object::Name(b"Font".to_vec()));
        dict.set("Subtype", Object::Name(b"Type1".to_vec()));
        dict.set("BaseFont", Object::Name(f.pdf_name().as_bytes().to_vec()));
        dict.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
        doc.add_object(Object::Dictionary(dict))
    })
}

fn text_commands(
    out: &mut String,
    spec: &TextSpec,
    font_name: &str,
    size: (f32, f32),
    c: (f32, f32),
    angle: f32,
) -> Result<(), AppError> {
    let bytes = StandardFont::encode(&spec.text).map_err(|ch| {
        AppError::new(
            ErrorCode::OptionInvalid,
            format!(
                "The character \"{ch}\" cannot be drawn with the {} font.",
                spec.font.pdf_name()
            ),
        )
    })?;
    let (sin, cos) = angle.to_radians().sin_cos();
    let [r, g, b] = spec.color.fractions();
    out.push_str("q\n");
    out.push_str(&cm([cos, sin, -sin, cos, c.0, c.1]));
    out.push_str(&format!("{} {} {} rg\nBT\n", num(r), num(g), num(b)));
    out.push_str(&format!("/{font_name} {} Tf\n", num(spec.size_pt)));
    out.push_str(&format!(
        "{} {} Td\n",
        num(-size.0 / 2.0),
        num(-size.1 / 2.0)
    ));
    out.push_str(&literal(&bytes));
    out.push_str(" Tj\nET\nQ\n");
    Ok(())
}

fn picture_commands(out: &mut String, name: &str, size: (f32, f32), c: (f32, f32), angle: f32) {
    let (sin, cos) = angle.to_radians().sin_cos();
    out.push_str("q\n");
    out.push_str(&cm([cos, sin, -sin, cos, c.0, c.1]));
    out.push_str(&cm([
        size.0,
        0.0,
        0.0,
        size.1,
        -size.0 / 2.0,
        -size.1 / 2.0,
    ]));
    out.push_str(&format!("/{name} Do\nQ\n"));
}

/// Draws the stamps of the listed pages.
pub fn apply_stamps(doc: &mut Document, pages: &[PageStamps]) -> Result<(), AppError> {
    let mut fonts: HashMap<StandardFont, ObjectId> = HashMap::new();
    let mut pictures: HashMap<PathBuf, Picture> = HashMap::new();
    for page in pages {
        // Pictures are decoded once per file even when many pages use them.
        for stamp in &page.stamps {
            if let StampContent::Image { path, .. } = &stamp.content
                && !pictures.contains_key(path)
            {
                let picture = load_picture(doc, path)?;
                pictures.insert(path.clone(), picture);
            }
        }
    }

    for page in pages {
        if page.stamps.is_empty() {
            continue;
        }
        let id = page_object(doc, page.page.get())?;
        let (page_w, page_h) = displayed_size(doc, id);
        let frame = display_matrix(doc, id);
        let mut resources = effective_resources(doc, id);
        let mut parts = PageParts {
            fonts: HashMap::new(),
            opacities: HashMap::new(),
            pictures: HashMap::new(),
        };
        let mut below = String::new();
        let mut above = String::new();

        for stamp in &page.stamps {
            let picture = match &stamp.content {
                StampContent::Image { path, .. } => pictures.get(path).copied(),
                StampContent::Text(_) => None,
            };
            let size = content_size(&stamp.content, picture, page_w);
            let out = match stamp.layer {
                Layer::Above => &mut above,
                Layer::Below => &mut below,
            };
            out.push_str("q\n");
            out.push_str(&cm(frame));
            if stamp.opacity < 0.999 {
                let key = (stamp.opacity.clamp(0.0, 1.0) * 1000.0).round() as u32;
                parts.opacities.entry(key).or_insert_with(|| {
                    let mut gs = Dictionary::new();
                    gs.set("Type", Object::Name(b"ExtGState".to_vec()));
                    let alpha = Object::Real(key as f32 / 1000.0);
                    gs.set("ca", alpha.clone());
                    gs.set("CA", alpha);
                    add_named(&mut resources, "ExtGState", "QwG", Object::Dictionary(gs))
                });
                if let Some(name) = parts.opacities.get(&key) {
                    out.push_str(&format!("/{name} gs\n"));
                }
            }
            match &stamp.content {
                StampContent::Text(spec) => {
                    parts.fonts.entry(spec.font).or_insert_with(|| {
                        let object = font_object(doc, &mut fonts, spec.font);
                        add_named(&mut resources, "Font", "QwF", Object::Reference(object))
                    });
                    let font_name = parts.fonts.get(&spec.font).cloned().unwrap_or_default();
                    for c in centers(stamp, (page_w, page_h), size) {
                        text_commands(out, spec, &font_name, size, c, stamp.rotation_deg)?;
                    }
                }
                StampContent::Image { path, .. } => {
                    let Some(picture) = picture else {
                        return Err(AppError::internal("loading the picture"));
                    };
                    let key: &Path = path;
                    if !parts.pictures.contains_key(key) {
                        let name = add_named(
                            &mut resources,
                            "XObject",
                            "QwI",
                            Object::Reference(picture.id),
                        );
                        parts.pictures.insert(key, name);
                    }
                    let name = parts.pictures.get(key).cloned().unwrap_or_default();
                    for c in centers(stamp, (page_w, page_h), size) {
                        picture_commands(out, &name, size, c, stamp.rotation_deg);
                    }
                }
            }
            out.push_str("Q\n");
        }
        set_resources(doc, id, resources)?;
        wrap_content(
            doc,
            id,
            (!below.is_empty()).then(|| below.into_bytes()),
            (!above.is_empty()).then(|| above.into_bytes()),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::PageIndex;
    use domain::fonts::FontFamily;
    use domain::stamp::{Position, Rgb};
    use test_support::{minimal_pdf, pdfs};

    fn text_stamp(text: &str, position: Position) -> Stamp {
        Stamp {
            content: StampContent::Text(TextSpec {
                text: text.to_owned(),
                font: StandardFont::new(FontFamily::Helvetica, false, false),
                size_pt: 10.0,
                color: Rgb::BLACK,
            }),
            position,
            offset_x_pt: 0.0,
            offset_y_pt: 0.0,
            rotation_deg: 0.0,
            opacity: 1.0,
            layer: Layer::Above,
            tile: false,
            margin_pt: 36.0,
        }
    }

    fn on(page: u32, stamps: Vec<Stamp>) -> PageStamps {
        PageStamps {
            page: PageIndex::new(page),
            stamps,
        }
    }

    fn content_of(doc: &Document, page: u32) -> String {
        let id = doc.get_pages()[&(page + 1)];
        let bytes = doc.get_page_content(id);
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[test]
    fn literals_escape_parentheses_backslashes_and_high_bytes() {
        assert_eq!(literal(b"a(b)c\\"), "(a\\(b\\)c\\\\)");
        assert_eq!(literal(&[0x41, 0xE9, 0x80]), "(A\\351\\200)");
    }

    #[test]
    fn a_page_number_is_drawn_centered_at_the_bottom_above_the_old_content() {
        let mut doc = Document::load_mem(&minimal_pdf(2)).unwrap();
        let before = content_of(&doc, 0);
        apply_stamps(
            &mut doc,
            &[on(0, vec![text_stamp("1", Position::BottomCenter)])],
        )
        .unwrap();
        let after = content_of(&doc, 0);
        assert!(after.contains("(1) Tj"), "{after}");
        assert!(after.contains("/QwF0 10 Tf"));
        assert!(
            after.find(&before).unwrap() < after.find("(1) Tj").unwrap(),
            "above"
        );
        assert!(after.starts_with("q\n"), "wrapped: {after}");
        // The other page is untouched.
        assert_eq!(
            content_of(&doc, 1),
            content_of(&Document::load_mem(&minimal_pdf(2)).unwrap(), 1)
        );
        // The font resource exists on the page and names a standard font.
        let id = doc.get_pages()[&1];
        let fonts = effective_resources(&doc, id);
        let table = fonts.get(b"Font").unwrap().as_dict().unwrap();
        let font_ref = table.get(b"QwF0").unwrap().as_reference().unwrap();
        let font = doc.get_dictionary(font_ref).unwrap();
        assert_eq!(
            font.get(b"BaseFont").unwrap().as_name().unwrap(),
            b"Helvetica"
        );
    }

    #[test]
    fn the_anchor_decides_where_the_text_sits() {
        // Letter page, 10 pt text "AB": width (667 + 667) * 10 / 1000 = 13.34, height 7.2.
        let at = |position| {
            let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
            apply_stamps(&mut doc, &[on(0, vec![text_stamp("AB", position)])]).unwrap();
            let c = content_of(&doc, 0);
            // The centre of the box is the translation of the second matrix.
            let m: Vec<f32> = c
                .lines()
                .filter(|l| l.ends_with(" cm"))
                .nth(1)
                .unwrap()
                .split(' ')
                .filter_map(|v| v.parse().ok())
                .collect();
            (m[4], m[5])
        };
        let (x, y) = at(Position::BottomLeft);
        assert!((x - (36.0 + 13.34 / 2.0)).abs() < 0.01, "{x}");
        assert!((y - (36.0 + 3.6)).abs() < 0.01, "{y}");
        let (x, y) = at(Position::TopRight);
        assert!((x - (612.0 - 36.0 - 13.34 / 2.0)).abs() < 0.01, "{x}");
        assert!((y - (792.0 - 36.0 - 3.6)).abs() < 0.01, "{y}");
        let (x, y) = at(Position::Center);
        assert!(
            (x - 306.0).abs() < 0.01 && (y - 396.0).abs() < 0.01,
            "{x} {y}"
        );
    }

    #[test]
    fn stamps_on_rotated_pages_use_the_displayed_frame() {
        let mut doc = Document::load_mem(&pdfs::pdf_with_geometry(&[(300, 400, 90)])).unwrap();
        apply_stamps(
            &mut doc,
            &[on(0, vec![text_stamp("x", Position::BottomLeft)])],
        )
        .unwrap();
        let c = content_of(&doc, 0);
        // A page turned a quarter clockwise maps the displayed frame with this matrix.
        assert!(c.contains("0 1 -1 0 300 0 cm"), "{c}");
    }

    #[test]
    fn below_stamps_come_first_and_opacity_adds_a_graphics_state() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        let old = content_of(&doc, 0);
        let mut stamp = text_stamp("DRAFT", Position::Center);
        stamp.layer = Layer::Below;
        stamp.opacity = 0.25;
        stamp.rotation_deg = 45.0;
        apply_stamps(&mut doc, &[on(0, vec![stamp])]).unwrap();
        let c = content_of(&doc, 0);
        assert!(c.find("(DRAFT) Tj").unwrap() < c.find(&old).unwrap(), "{c}");
        assert!(c.contains("/QwG0 gs"));
        assert!(
            c.contains("0.7071 0.7071 -0.7071 0.7071"),
            "rotation matrix: {c}"
        );
        let id = doc.get_pages()[&1];
        let res = effective_resources(&doc, id);
        let gs = res.get(b"ExtGState").unwrap().as_dict().unwrap();
        let state = gs.get(b"QwG0").unwrap().as_dict().unwrap();
        assert!((state.get(b"ca").unwrap().as_float().unwrap() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn tiling_repeats_the_text_across_the_page() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        let mut stamp = text_stamp("COPY", Position::Center);
        stamp.tile = true;
        apply_stamps(&mut doc, &[on(0, vec![stamp])]).unwrap();
        let count = content_of(&doc, 0).matches("(COPY) Tj").count();
        assert!(count > 10 && count <= MAX_TILES, "{count}");
    }

    #[test]
    fn stamping_twice_keeps_both_stamps_and_their_resources() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        apply_stamps(
            &mut doc,
            &[on(0, vec![text_stamp("first", Position::TopLeft)])],
        )
        .unwrap();
        apply_stamps(
            &mut doc,
            &[on(0, vec![text_stamp("second", Position::TopRight)])],
        )
        .unwrap();
        let c = content_of(&doc, 0);
        assert!(c.contains("(first) Tj") && c.contains("(second) Tj"));
        let id = doc.get_pages()[&1];
        let res = effective_resources(&doc, id);
        let fonts = res.get(b"Font").unwrap().as_dict().unwrap();
        assert!(
            fonts.get(b"QwF0").is_ok() && fonts.get(b"QwF1").is_ok(),
            "two names"
        );
        assert!(c.contains("/QwF0 10 Tf") && c.contains("/QwF1 10 Tf"));
    }

    #[test]
    fn a_picture_is_embedded_once_and_drawn_at_the_requested_width() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logo.png");
        let mut img = image::RgbaImage::new(40, 20);
        for (x, _, px) in img.enumerate_pixels_mut() {
            *px = image::Rgba([200, 30, 30, if x < 20 { 255 } else { 0 }]);
        }
        img.save(&path).unwrap();
        let mut doc = Document::load_mem(&minimal_pdf(2)).unwrap();
        let stamp = Stamp {
            content: StampContent::Image {
                path: path.clone(),
                width_fraction: 0.5,
            },
            ..text_stamp("", Position::Center)
        };
        apply_stamps(&mut doc, &[on(0, vec![stamp.clone()]), on(1, vec![stamp])]).unwrap();
        let images = doc
            .objects
            .values()
            .filter(|o| matches!(o, Object::Stream(s) if s.dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|n| n == b"Image")))
            .count();
        assert_eq!(
            images, 2,
            "one picture and its transparency mask, shared by both pages"
        );
        // 50% of 612 is 306 wide; the 2:1 picture is 153 high.
        assert!(
            content_of(&doc, 0).contains("306 0 0 153 -153 -76.5 cm"),
            "{}",
            content_of(&doc, 0)
        );
        assert!(content_of(&doc, 1).contains("/QwI0 Do"));
    }

    #[test]
    fn an_unreadable_picture_is_reported_as_an_option_problem() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-an-image.png");
        std::fs::write(&path, b"nope").unwrap();
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        let stamp = Stamp {
            content: StampContent::Image {
                path,
                width_fraction: 0.5,
            },
            ..text_stamp("", Position::Center)
        };
        let err = apply_stamps(&mut doc, &[on(0, vec![stamp])]).unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
        let missing = Stamp {
            content: StampContent::Image {
                path: dir.path().join("missing.png"),
                width_fraction: 0.5,
            },
            ..text_stamp("", Position::Center)
        };
        assert_eq!(
            apply_stamps(&mut doc, &[on(0, vec![missing])])
                .unwrap_err()
                .code,
            ErrorCode::PathDenied
        );
    }
}
