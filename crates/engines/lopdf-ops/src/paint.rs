//! Drawing the things the editor adds to a page: text, pictures, shapes, patches and strokes.
//!
//! Everything is laid out in the page as displayed (origin bottom left, y up), from boxes given
//! as fractions of that page measured from the top left. One `cm` moves the drawing into the
//! page's user space, so rotated pages and crop boxes need no special cases.

use application::AppError;
use domain::ErrorCode;
use domain::stamp::Rgb;
use domain::tools::common::Region;
use domain::tools::editor::{Point, ShapeItem, ShapeKind, TextAlign, TextStyle};
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::boxes::{display_matrix, displayed_size};
use crate::fonts::Face;
use crate::pageres::{
    add_named, cm, effective_resources, num, page_object, set_resources, wrap_content,
};
use crate::stamp::{Picture, literal, load_picture, load_picture_bytes};

/// Space between lines, as a multiple of the font size.
const LINE_HEIGHT: f32 = 1.2;
/// The control distance that makes a Bezier curve follow a quarter of a circle.
const KAPPA: f32 = 0.552_284_7;

/// What several pages of one document share, so a picture or font is written once.
#[derive(Default)]
pub struct Shared {
    pictures: HashMap<PathBuf, Picture>,
    fonts: HashMap<String, ObjectId>,
}

/// A box in points on the displayed page: left, bottom, width, height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Boxed {
    /// Left edge.
    pub x: f32,
    /// Bottom edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

/// Content being added to one page.
pub struct Painter {
    id: ObjectId,
    /// Width and height of the displayed page.
    pub size: (f32, f32),
    frame: [f32; 6],
    resources: Dictionary,
    out: String,
    fonts: HashMap<String, String>,
    pictures: HashMap<PathBuf, String>,
}

fn cannot_draw(c: char, face: &Face) -> AppError {
    AppError::new(
        ErrorCode::OptionInvalid,
        format!(
            "The character \"{c}\" cannot be drawn with the {} font. Choose a font that has it, or remove the character.",
            face.pdf_name()
        ),
    )
}

/// Breaks text into lines no wider than `max`: at line breaks, then between words.
pub fn wrap(face: &Face, text: &str, size: f32, max: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let candidate = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if line.is_empty() || face.width(&candidate, size) <= max {
                line = candidate;
            } else {
                lines.push(std::mem::take(&mut line));
                line = word.to_owned();
            }
        }
        lines.push(line);
    }
    lines
}

fn color_ops(color: &str, stroke: bool) -> Result<String, AppError> {
    let rgb = Rgb::parse(color).map_err(|_| {
        AppError::new(
            ErrorCode::OptionInvalid,
            "A color is not written as #RRGGBB.",
        )
    })?;
    let [r, g, b] = rgb.fractions();
    Ok(format!(
        "{} {} {} {}\n",
        num(r),
        num(g),
        num(b),
        if stroke { "RG" } else { "rg" }
    ))
}

impl Painter {
    /// Starts content for page `index` (from 0).
    pub fn new(doc: &Document, index: u32) -> Result<Self, AppError> {
        let id = page_object(doc, index)?;
        Ok(Self {
            id,
            size: displayed_size(doc, id),
            frame: display_matrix(doc, id),
            resources: effective_resources(doc, id),
            out: String::new(),
            fonts: HashMap::new(),
            pictures: HashMap::new(),
        })
    }

    /// The page object this painter draws on.
    pub fn page(&self) -> ObjectId {
        self.id
    }

    /// The display matrix, for placing annotations.
    pub fn frame(&self) -> [f32; 6] {
        self.frame
    }

    /// A region as a box in points on the displayed page.
    pub fn boxed(&self, r: &Region) -> Boxed {
        let (pw, ph) = self.size;
        Boxed {
            x: r.x * pw,
            y: ph - (r.y + r.height) * ph,
            w: r.width * pw,
            h: r.height * ph,
        }
    }

    /// A point as displayed-page points (y up).
    pub fn at(&self, p: &Point) -> (f32, f32) {
        (p.x * self.size.0, self.size.1 - p.y * self.size.1)
    }

    fn open(&mut self) {
        self.out.push_str("q\n");
        self.out.push_str(&cm(self.frame));
    }

    fn close(&mut self) {
        self.out.push_str("Q\n");
    }

    /// A solid patch.
    pub fn patch(&mut self, r: &Region, color: &str) -> Result<(), AppError> {
        let b = self.boxed(r);
        self.open();
        self.out.push_str(&color_ops(color, false)?);
        self.out.push_str(&format!(
            "{} {} {} {} re f\n",
            num(b.x),
            num(b.y),
            num(b.w),
            num(b.h)
        ));
        self.close();
        Ok(())
    }

    /// A shape.
    pub fn shape(&mut self, item: &ShapeItem) -> Result<(), AppError> {
        let b = self.boxed(&item.region);
        let width = item.stroke_width_pt;
        self.open();
        self.out.push_str(&color_ops(&item.stroke_color, true)?);
        if let Some(fill) = &item.fill_color {
            self.out.push_str(&color_ops(fill, false)?);
        }
        self.out.push_str(&format!("{} w\n", num(width)));
        let paint = if item.fill_color.is_some() { "B" } else { "S" };
        match item.shape {
            ShapeKind::Rectangle => {
                // Keep the outline inside the box.
                let (x, y) = (b.x + width / 2.0, b.y + width / 2.0);
                let (w, h) = ((b.w - width).max(0.1), (b.h - width).max(0.1));
                self.out.push_str(&format!(
                    "{} {} {} {} re {paint}\n",
                    num(x),
                    num(y),
                    num(w),
                    num(h)
                ));
            }
            ShapeKind::Ellipse => {
                let (cx, cy) = (b.x + b.w / 2.0, b.y + b.h / 2.0);
                let (rx, ry) = ((b.w - width).max(0.1) / 2.0, (b.h - width).max(0.1) / 2.0);
                let (kx, ky) = (rx * KAPPA, ry * KAPPA);
                let p = |x: f32, y: f32| format!("{} {}", num(x), num(y));
                self.out.push_str(&format!("{} m\n", p(cx + rx, cy)));
                self.out.push_str(&format!(
                    "{} {} {} c\n",
                    p(cx + rx, cy + ky),
                    p(cx + kx, cy + ry),
                    p(cx, cy + ry)
                ));
                self.out.push_str(&format!(
                    "{} {} {} c\n",
                    p(cx - kx, cy + ry),
                    p(cx - rx, cy + ky),
                    p(cx - rx, cy)
                ));
                self.out.push_str(&format!(
                    "{} {} {} c\n",
                    p(cx - rx, cy - ky),
                    p(cx - kx, cy - ry),
                    p(cx, cy - ry)
                ));
                self.out.push_str(&format!(
                    "{} {} {} c\n",
                    p(cx + kx, cy - ry),
                    p(cx + rx, cy - ky),
                    p(cx + rx, cy)
                ));
                self.out.push_str(&format!("h {paint}\n"));
            }
            ShapeKind::Line | ShapeKind::Arrow => {
                let (from, to) = if item.flip {
                    ((b.x, b.y), (b.x + b.w, b.y + b.h))
                } else {
                    ((b.x, b.y + b.h), (b.x + b.w, b.y))
                };
                self.out.push_str("1 J\n");
                self.out.push_str(&format!(
                    "{} {} m {} {} l S\n",
                    num(from.0),
                    num(from.1),
                    num(to.0),
                    num(to.1)
                ));
                if item.shape == ShapeKind::Arrow {
                    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
                    let len = dx.hypot(dy).max(0.001);
                    let (ux, uy) = (dx / len, dy / len);
                    let head = (width * 4.0).max(8.0).min(len);
                    let half = head * 0.4;
                    let base = (to.0 - ux * head, to.1 - uy * head);
                    // The head is filled with the outline color.
                    self.out.push_str(&color_ops(&item.stroke_color, false)?);
                    self.out.push_str(&format!(
                        "{} {} m {} {} l {} {} l h f\n",
                        num(to.0),
                        num(to.1),
                        num(base.0 - uy * half),
                        num(base.1 + ux * half),
                        num(base.0 + uy * half),
                        num(base.1 - ux * half)
                    ));
                }
            }
        }
        self.close();
        Ok(())
    }

    fn font_name(&mut self, doc: &mut Document, shared: &mut Shared, face: &Face) -> String {
        let key = face.key();
        if let Some(name) = self.fonts.get(&key) {
            return name.clone();
        }
        let id = *shared
            .fonts
            .entry(key.clone())
            .or_insert_with(|| face.write(doc));
        let name = add_named(&mut self.resources, "Font", "QwF", Object::Reference(id));
        self.fonts.insert(key, name.clone());
        name
    }

    /// Lines of text starting at the top of a box. `fit` shrinks the size until the longest line
    /// fits the width.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        doc: &mut Document,
        shared: &mut Shared,
        region: &Region,
        text: &str,
        style: &TextStyle,
        align: TextAlign,
    ) -> Result<(), AppError> {
        let face = Face::resolve(style)?;
        self.text_face(
            doc,
            shared,
            region,
            text,
            &face,
            style.size_pt,
            &style.color,
            align,
        )
    }

    /// The size, at most `size`, at which `text` wraps inside the box without running out of its
    /// height, but not below `floor`.
    pub fn fit_size(&self, region: &Region, text: &str, face: &Face, size: f32, floor: f32) -> f32 {
        let b = self.boxed(region);
        let mut size = size.max(floor);
        while size > floor {
            let lines = wrap(face, text, size, b.w).len() as f32;
            if lines * size * LINE_HEIGHT <= b.h {
                break;
            }
            size = (size * 0.94).max(floor);
        }
        size
    }

    /// Lines of text in a given font and color, starting at the top of a box.
    #[allow(clippy::too_many_arguments)]
    pub fn text_face(
        &mut self,
        doc: &mut Document,
        shared: &mut Shared,
        region: &Region,
        text: &str,
        face: &Face,
        size_pt: f32,
        color: &str,
        align: TextAlign,
    ) -> Result<(), AppError> {
        let b = self.boxed(region);
        let lines = wrap(face, text, size_pt, b.w);
        let font = self.font_name(doc, shared, face);
        self.open();
        self.out.push_str(&color_ops(color, false)?);
        let top = b.y + b.h;
        for (i, line) in lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let bytes = face.encode(line).map_err(|c| cannot_draw(c, face))?;
            let width = face.width(line, size_pt);
            let x = match align {
                TextAlign::Left => b.x,
                TextAlign::Center => b.x + (b.w - width) / 2.0,
                TextAlign::Right => b.x + b.w - width,
            };
            let y = top - face.ascent() * size_pt - i as f32 * size_pt * LINE_HEIGHT;
            self.out.push_str(&format!(
                "BT\n/{font} {} Tf\n{} {} Td\n{} Tj\nET\n",
                num(size_pt),
                num(x),
                num(y),
                literal(&bytes)
            ));
        }
        self.close();
        Ok(())
    }

    /// One line of text centered in a box, made smaller when it would not fit the width.
    pub fn text_centered(
        &mut self,
        doc: &mut Document,
        shared: &mut Shared,
        region: &Region,
        text: &str,
        style: &TextStyle,
    ) -> Result<(), AppError> {
        let face = Face::resolve(style)?;
        let b = self.boxed(region);
        let text = text.replace(['\r', '\n'], " ");
        let natural = face.width(&text, style.size_pt);
        let size = if natural > b.w && natural > 0.0 {
            style.size_pt * b.w / natural
        } else {
            style.size_pt
        };
        let width = face.width(&text, size);
        let bytes = face.encode(&text).map_err(|c| cannot_draw(c, &face))?;
        let font = self.font_name(doc, shared, &face);
        self.open();
        self.out.push_str(&color_ops(&style.color, false)?);
        self.out.push_str(&format!(
            "BT\n/{font} {} Tf\n{} {} Td\n{} Tj\nET\n",
            num(size),
            num(b.x + (b.w - width) / 2.0),
            num(b.y + (b.h - face.cap_height() * size) / 2.0),
            literal(&bytes)
        ));
        self.close();
        Ok(())
    }

    /// A picture file, kept in proportion and centered in the box.
    pub fn image(
        &mut self,
        doc: &mut Document,
        shared: &mut Shared,
        region: &Region,
        path: &Path,
    ) -> Result<(), AppError> {
        if !shared.pictures.contains_key(path) {
            let picture = load_picture(doc, path)?;
            shared.pictures.insert(path.to_path_buf(), picture);
        }
        self.place_picture(shared, region, path)
    }

    /// A picture given as bytes, under a key that tells equal pictures apart.
    pub fn image_bytes(
        &mut self,
        doc: &mut Document,
        shared: &mut Shared,
        region: &Region,
        key: &str,
        bytes: &[u8],
    ) -> Result<(), AppError> {
        let key_path = PathBuf::from(format!("bytes:{key}"));
        if !shared.pictures.contains_key(&key_path) {
            let picture = load_picture_bytes(doc, bytes)?;
            shared.pictures.insert(key_path.clone(), picture);
        }
        self.place_picture(shared, region, &key_path)
    }

    fn place_picture(
        &mut self,
        shared: &Shared,
        region: &Region,
        path: &Path,
    ) -> Result<(), AppError> {
        let Some(picture) = shared.pictures.get(path).copied() else {
            return Err(AppError::internal("loading the picture"));
        };
        let name = match self.pictures.get(path) {
            Some(name) => name.clone(),
            None => {
                let name = add_named(
                    &mut self.resources,
                    "XObject",
                    "QwI",
                    Object::Reference(picture.id),
                );
                self.pictures.insert(path.to_path_buf(), name.clone());
                name
            }
        };
        let b = self.boxed(region);
        let aspect = picture.width as f32 / picture.height.max(1) as f32;
        let (w, h) = if b.w / b.h.max(0.001) > aspect {
            (b.h * aspect, b.h)
        } else {
            (b.w, b.w / aspect)
        };
        self.open();
        self.out.push_str(&cm([
            w,
            0.0,
            0.0,
            h,
            b.x + (b.w - w) / 2.0,
            b.y + (b.h - h) / 2.0,
        ]));
        self.out.push_str(&format!("/{name} Do\n"));
        self.close();
        Ok(())
    }

    /// Freehand strokes whose points are fractions of the box, drawn with round ends.
    pub fn strokes(
        &mut self,
        region: &Region,
        strokes: &[Vec<Point>],
        color: &str,
        width: f32,
    ) -> Result<(), AppError> {
        let b = self.boxed(region);
        self.open();
        self.out.push_str(&color_ops(color, true)?);
        self.out.push_str(&format!("{} w\n1 J 1 j\n", num(width)));
        for stroke in strokes {
            let mut points = stroke.iter().map(|p| {
                (
                    num(b.x + p.x.clamp(0.0, 1.0) * b.w),
                    num(b.y + b.h - p.y.clamp(0.0, 1.0) * b.h),
                )
            });
            let Some(first) = points.next() else { continue };
            self.out.push_str(&format!("{} {} m\n", first.0, first.1));
            let mut count = 0;
            for p in points {
                self.out.push_str(&format!("{} {} l\n", p.0, p.1));
                count += 1;
            }
            if count == 0 {
                // A dot: a zero-length line with round ends.
                self.out.push_str(&format!("{} {} l\n", first.0, first.1));
            }
            self.out.push_str("S\n");
        }
        self.close();
        Ok(())
    }

    /// Writes the drawing above the page's own content.
    pub fn finish(self, doc: &mut Document) -> Result<(), AppError> {
        set_resources(doc, self.id, self.resources)?;
        if self.out.is_empty() {
            return Ok(());
        }
        wrap_content(doc, self.id, None, Some(self.out.into_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::fonts::{FontFamily, StandardFont};

    fn face() -> Face {
        Face::Standard(StandardFont::new(FontFamily::Helvetica, false, false))
    }

    #[test]
    fn text_wraps_at_the_width_and_keeps_line_breaks() {
        let lines = wrap(&face(), "one two three four", 10.0, 40.0);
        assert!(lines.len() >= 2, "{lines:?}");
        assert!(
            lines
                .iter()
                .all(|l| face().width(l, 10.0) <= 40.0 || !l.contains(' '))
        );
        assert_eq!(lines.join(" "), "one two three four");
        assert_eq!(wrap(&face(), "a\n\nb", 10.0, 500.0), ["a", "", "b"]);
        // A word wider than the box stays whole on its own line.
        assert_eq!(
            wrap(&face(), "extraordinarily short", 10.0, 20.0),
            ["extraordinarily", "short"]
        );
        assert_eq!(wrap(&face(), "", 10.0, 20.0), [""]);
    }

    #[test]
    fn regions_become_boxes_in_points_with_y_up() {
        let doc = Document::load_mem(&test_support::minimal_pdf(1)).unwrap();
        let painter = Painter::new(&doc, 0).unwrap();
        let b = painter.boxed(&Region {
            x: 0.1,
            y: 0.2,
            width: 0.5,
            height: 0.25,
        });
        let (w, h) = painter.size;
        assert!((b.x - 0.1 * w).abs() < 0.01 && (b.w - 0.5 * w).abs() < 0.01);
        assert!((b.y - (h - 0.45 * h)).abs() < 0.01 && (b.h - 0.25 * h).abs() < 0.01);
        let (px, py) = painter.at(&Point { x: 0.0, y: 0.0 });
        assert_eq!((px, py), (0.0, h));
    }
}
