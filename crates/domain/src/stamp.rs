//! What is drawn on a page by the stamping tools (page numbers, headers and footers, Bates
//! numbers, watermarks): positions, colors, number styles and the `{page}` style tokens.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::PageIndex;
use crate::error::DomainError;
use crate::fonts::StandardFont;

/// Distance from the page edge for stamps anchored to an edge or corner, in points (0.5 inch).
pub const EDGE_MARGIN_PT: f32 = 36.0;

/// An RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    /// Red, 0 to 255.
    pub r: u8,
    /// Green, 0 to 255.
    pub g: u8,
    /// Blue, 0 to 255.
    pub b: u8,
}

impl Rgb {
    /// Black.
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0 };

    /// Parses `#RRGGBB` or `RRGGBB`.
    pub fn parse(text: &str) -> Result<Self, DomainError> {
        let hex = text.trim().trim_start_matches('#');
        let bad = || DomainError::option("color", "use a color such as #1F2937");
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(bad());
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| bad());
        Ok(Self {
            r: channel(0)?,
            g: channel(2)?,
            b: channel(4)?,
        })
    }

    /// Each channel as a fraction from 0 to 1.
    pub fn fractions(self) -> [f32; 3] {
        [self.r, self.g, self.b].map(|c| f32::from(c) / 255.0)
    }
}

/// Where a stamp sits on the page: a 3 by 3 grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Position {
    /// Top left corner.
    TopLeft,
    /// Top edge, centered.
    TopCenter,
    /// Top right corner.
    TopRight,
    /// Left edge, centered vertically.
    MiddleLeft,
    /// Center of the page.
    Center,
    /// Right edge, centered vertically.
    MiddleRight,
    /// Bottom left corner.
    BottomLeft,
    /// Bottom edge, centered.
    #[default]
    BottomCenter,
    /// Bottom right corner.
    BottomRight,
}

impl Position {
    /// Horizontal place: -1 left, 0 center, 1 right.
    pub const fn horizontal(self) -> i8 {
        match self {
            Self::TopLeft | Self::MiddleLeft | Self::BottomLeft => -1,
            Self::TopCenter | Self::Center | Self::BottomCenter => 0,
            Self::TopRight | Self::MiddleRight | Self::BottomRight => 1,
        }
    }

    /// Vertical place: -1 bottom, 0 middle, 1 top.
    pub const fn vertical(self) -> i8 {
        match self {
            Self::BottomLeft | Self::BottomCenter | Self::BottomRight => -1,
            Self::MiddleLeft | Self::Center | Self::MiddleRight => 0,
            Self::TopLeft | Self::TopCenter | Self::TopRight => 1,
        }
    }

    /// The same place seen in a mirror: left and right swap.
    pub const fn mirrored(self) -> Self {
        match self {
            Self::TopLeft => Self::TopRight,
            Self::TopRight => Self::TopLeft,
            Self::MiddleLeft => Self::MiddleRight,
            Self::MiddleRight => Self::MiddleLeft,
            Self::BottomLeft => Self::BottomRight,
            Self::BottomRight => Self::BottomLeft,
            other => other,
        }
    }
}

/// Whether a stamp lies over the page content or beneath it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Layer {
    /// Over the content (default).
    #[default]
    Above,
    /// Behind the content, so text stays on top.
    Below,
}

/// How a page number is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum NumberStyle {
    /// 1, 2, 3 (default).
    #[default]
    Arabic,
    /// i, ii, iii.
    RomanLower,
    /// I, II, III.
    RomanUpper,
    /// a, b, c, then aa, bb, cc.
    LettersLower,
    /// A, B, C, then AA, BB, CC.
    LettersUpper,
}

impl NumberStyle {
    /// The `/S` value of a PDF page label range, or `None` for numbers without a style.
    pub const fn label_code(self) -> &'static str {
        match self {
            Self::Arabic => "D",
            Self::RomanLower => "r",
            Self::RomanUpper => "R",
            Self::LettersLower => "a",
            Self::LettersUpper => "A",
        }
    }

    /// Writes `n` in this style. Roman numerals and letters start at 1; 0 and numbers past what
    /// the style can write fall back to Arabic digits.
    pub fn format(self, n: u32) -> String {
        match self {
            Self::Arabic => n.to_string(),
            Self::RomanLower => roman(n).map_or_else(|| n.to_string(), |r| r.to_lowercase()),
            Self::RomanUpper => roman(n).unwrap_or_else(|| n.to_string()),
            Self::LettersLower => letters(n).map_or_else(|| n.to_string(), |l| l.to_lowercase()),
            Self::LettersUpper => letters(n).unwrap_or_else(|| n.to_string()),
        }
    }
}

fn roman(n: u32) -> Option<String> {
    if n == 0 || n >= 4000 {
        return None;
    }
    const TABLE: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut rest = n;
    let mut out = String::new();
    for (value, text) in TABLE {
        while rest >= value {
            out.push_str(text);
            rest -= value;
        }
    }
    Some(out)
}

/// PDF's lettering: A to Z, then AA to ZZ, then AAA, and so on.
fn letters(n: u32) -> Option<String> {
    if n == 0 {
        return None;
    }
    let index = (n - 1) % 26;
    let repeat = (n - 1) / 26 + 1;
    let letter = char::from(b'A' + u8::try_from(index).ok()?);
    Some(std::iter::repeat_n(letter, repeat as usize).collect())
}

/// The values the style tokens stand for.
#[derive(Debug, Clone, Copy)]
pub struct TokenValues<'a> {
    /// `{page}`.
    pub page: &'a str,
    /// `{pages}`.
    pub pages: &'a str,
    /// `{date}`, as `2026-10-19`.
    pub date: &'a str,
    /// `{filename}`, without the extension.
    pub filename: &'a str,
}

const TOKENS: [&str; 4] = ["page", "pages", "date", "filename"];

/// Checks a template: only the known tokens, with `{{` and `}}` for literal braces.
pub fn validate_template(template: &str) -> Result<(), DomainError> {
    walk(template, |_| ())
}

fn walk(template: &str, mut each: impl FnMut(&str)) -> Result<(), DomainError> {
    let mut chars = template.chars().peekable();
    let mut name = String::new();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
            }
            '{' => {
                name.clear();
                let mut closed = false;
                for next in chars.by_ref() {
                    if next == '}' {
                        closed = true;
                        break;
                    }
                    name.push(next);
                }
                if !closed || !TOKENS.contains(&name.as_str()) {
                    return Err(DomainError::option(
                        "template",
                        format!(
                            "\"{{{name}\" is not a token; use {{page}}, {{pages}}, {{date}} or {{filename}}"
                        ),
                    ));
                }
                each(&name);
            }
            '}' => {
                return Err(DomainError::option(
                    "template",
                    "a lone } is not allowed; write }} for a literal brace",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Replaces the tokens of a validated template. Unknown tokens are kept as typed.
pub fn expand_template(template: &str, values: &TokenValues<'_>) -> String {
    let mut out = String::with_capacity(template.len() + 8);
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut name = String::new();
                let mut closed = false;
                for next in chars.by_ref() {
                    if next == '}' {
                        closed = true;
                        break;
                    }
                    name.push(next);
                }
                match (closed, name.as_str()) {
                    (true, "page") => out.push_str(values.page),
                    (true, "pages") => out.push_str(values.pages),
                    (true, "date") => out.push_str(values.date),
                    (true, "filename") => out.push_str(values.filename),
                    _ => {
                        out.push('{');
                        out.push_str(&name);
                        if closed {
                            out.push('}');
                        }
                    }
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// A line of text to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct TextSpec {
    /// The text. Only characters the font can draw (see [`StandardFont::encode`]).
    pub text: String,
    /// The font.
    pub font: StandardFont,
    /// Size in points.
    pub size_pt: f32,
    /// Fill color.
    pub color: Rgb,
}

/// What a stamp shows.
#[derive(Debug, Clone, PartialEq)]
pub enum StampContent {
    /// A line of text.
    Text(TextSpec),
    /// A PNG or JPEG file, drawn at a fraction of the page width.
    Image {
        /// The picture file.
        path: PathBuf,
        /// Width as a fraction of the visible page width, above 0 and at most 1.
        width_fraction: f32,
    },
}

/// One thing drawn on one page.
#[derive(Debug, Clone, PartialEq)]
pub struct Stamp {
    /// What to draw.
    pub content: StampContent,
    /// Anchor on the page grid.
    pub position: Position,
    /// Move right by this many points (negative moves left).
    pub offset_x_pt: f32,
    /// Move up by this many points (negative moves down).
    pub offset_y_pt: f32,
    /// Counter-clockwise rotation about the stamp's center, in degrees.
    pub rotation_deg: f32,
    /// 1 is solid, 0 is invisible.
    pub opacity: f32,
    /// Over or under the page content.
    pub layer: Layer,
    /// Repeat the stamp across the whole page.
    pub tile: bool,
    /// Distance from the edges for edge and corner anchors, in points.
    pub margin_pt: f32,
}

/// The stamps of one page.
#[derive(Debug, Clone, PartialEq)]
pub struct PageStamps {
    /// The page.
    pub page: PageIndex,
    /// What to draw on it, in order.
    pub stamps: Vec<Stamp>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_parse_with_or_without_a_hash_and_reject_the_rest() {
        assert_eq!(
            Rgb::parse("#FF8000").unwrap(),
            Rgb {
                r: 255,
                g: 128,
                b: 0
            }
        );
        assert_eq!(Rgb::parse("00ff00").unwrap().g, 255);
        for bad in ["", "#12345", "red", "#GG0000", "#1234567"] {
            assert!(Rgb::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(Rgb::BLACK.fractions(), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn mirroring_swaps_left_and_right_only() {
        assert_eq!(Position::BottomRight.mirrored(), Position::BottomLeft);
        assert_eq!(Position::MiddleLeft.mirrored(), Position::MiddleRight);
        assert_eq!(Position::BottomCenter.mirrored(), Position::BottomCenter);
        assert_eq!(Position::Center.mirrored(), Position::Center);
        assert_eq!(Position::TopLeft.horizontal(), -1);
        assert_eq!(Position::TopLeft.vertical(), 1);
        assert_eq!(Position::BottomCenter.vertical(), -1);
    }

    #[test]
    fn numbers_are_written_in_every_style() {
        assert_eq!(NumberStyle::Arabic.format(12), "12");
        assert_eq!(NumberStyle::RomanLower.format(4), "iv");
        assert_eq!(NumberStyle::RomanUpper.format(1994), "MCMXCIV");
        assert_eq!(NumberStyle::LettersLower.format(1), "a");
        assert_eq!(NumberStyle::LettersUpper.format(26), "Z");
        assert_eq!(NumberStyle::LettersUpper.format(27), "AA");
        assert_eq!(NumberStyle::LettersLower.format(53), "aaa");
        // Out of range for the style: digits are better than nothing.
        assert_eq!(NumberStyle::RomanLower.format(0), "0");
        assert_eq!(NumberStyle::RomanUpper.format(4000), "4000");
        assert_eq!(NumberStyle::LettersLower.format(0), "0");
    }

    #[test]
    fn templates_expand_tokens_and_escape_braces() {
        let values = TokenValues {
            page: "3",
            pages: "10",
            date: "2026-10-19",
            filename: "report",
        };
        assert_eq!(
            expand_template("Page {page} of {pages}", &values),
            "Page 3 of 10"
        );
        assert_eq!(
            expand_template("{filename} {date} {{x}}", &values),
            "report 2026-10-19 {x}"
        );
        assert_eq!(expand_template("plain", &values), "plain");
    }

    #[test]
    fn templates_reject_unknown_tokens_and_stray_braces() {
        assert!(validate_template("Page {page} of {pages}").is_ok());
        assert!(validate_template("{{literal}}").is_ok());
        assert!(validate_template("").is_ok());
        for bad in ["{pagee}", "{page", "oops}", "{}"] {
            let err = validate_template(bad).unwrap_err();
            assert_eq!(err.code(), crate::ErrorCode::OptionInvalid, "{bad}");
        }
    }
}
