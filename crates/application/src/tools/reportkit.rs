//! What the report tools share: laying text out on blank Letter pages as items for the editor
//! engine, so no second PDF writer is needed.

use domain::fonts::{FontFamily, StandardFont};
use domain::tools::common::Region;
use domain::tools::editor::{EditItem, TextAlign, TextItem, TextStyle, WhiteoutItem};

/// Size of a report page in points: Letter.
pub const PAGE: (f32, f32) = (612.0, 792.0);
/// Space around the content.
pub const MARGIN: f32 = 40.0;

pub const INK: &str = "#1F2328";
pub const MUTED: &str = "#5A5F66";

/// The finished layout.
pub struct Report {
    /// Pages of the report.
    pub pages: u32,
    /// What to draw on them.
    pub items: Vec<EditItem>,
}

/// What can be drawn in the standard fonts: other characters become a question mark and control
/// characters go.
pub fn printable(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\t' => Some(' '),
            c if c.is_control() => None,
            c if StandardFont::encode_char(c).is_some() => Some(c),
            _ => Some('?'),
        })
        .collect()
}

/// Breaks text into lines of at most `width` characters, at spaces where it can.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let text = printable(text);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split(' ') {
        let mut word = word.to_owned();
        while word.chars().count() > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let head: String = word.chars().take(width).collect();
            word = word.chars().skip(width).collect();
            lines.push(head);
        }
        let joined = if current.is_empty() {
            word.clone()
        } else {
            format!("{current} {word}")
        };
        if joined.chars().count() > width {
            lines.push(std::mem::take(&mut current));
            current = word;
        } else {
            current = joined;
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

/// How a line of text looks.
#[derive(Clone, Copy)]
pub struct Face<'a> {
    /// Size in points.
    pub size: f32,
    /// Bold.
    pub bold: bool,
    /// Color as `#RRGGBB`.
    pub color: &'a str,
}

pub struct Builder {
    /// What to draw so far.
    pub items: Vec<EditItem>,
    /// The page being filled, counted from 1.
    pub page: u32,
    /// Distance of the next line from the top of the page, in points.
    pub y: f32,
}

impl Builder {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            page: 1,
            y: MARGIN,
        }
    }

    pub fn region(x: f32, y: f32, width: f32, height: f32) -> Region {
        Region {
            x: x / PAGE.0,
            y: y / PAGE.1,
            width: width / PAGE.0,
            height: height / PAGE.1,
        }
    }

    pub fn new_page(&mut self) {
        self.page += 1;
        self.y = MARGIN;
    }

    pub fn room(&self, height: f32) -> bool {
        self.y + height <= PAGE.1 - MARGIN
    }

    /// Text at `(x, width, y)` in the look `face` says.
    pub fn text(&mut self, place: (f32, f32, f32), text: &str, face: Face<'_>) {
        let (x, width, y) = place;
        let Face { size, bold, color } = face;
        let text = printable(text);
        if text.trim().is_empty() {
            return;
        }
        self.items.push(EditItem::Text(TextItem {
            page: self.page,
            region: Self::region(x, y, width, size * 1.4),
            text,
            style: TextStyle {
                font: FontFamily::Helvetica,
                installed_font: None,
                bold,
                italic: false,
                size_pt: size,
                color: color.to_owned(),
            },
            align: TextAlign::Left,
        }));
    }

    pub fn fill(&mut self, x: f32, y: f32, width: f32, height: f32, color: &str) {
        self.items.push(EditItem::Whiteout(WhiteoutItem {
            page: self.page,
            region: Self::region(x, y, width, height),
            color: color.to_owned(),
        }));
    }

    /// A line of text at the cursor, then the cursor moves down.
    pub fn line(&mut self, text: &str, size: f32, bold: bool, color: &str) {
        let height = size * 1.5;
        if !self.room(height) {
            self.new_page();
        }
        let y = self.y;
        self.text(
            (MARGIN, PAGE.0 - 2.0 * MARGIN, y),
            text,
            Face { size, bold, color },
        );
        self.y += height;
    }

    pub fn gap(&mut self, height: f32) {
        self.y += height;
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}
