//! The 12 standard PDF fonts (Helvetica, Times, Courier with bold and italic): their names, the
//! WinAnsi text encoding and the glyph widths, so text can be measured and aligned without
//! loading a font file. Readers always have these fonts, so nothing needs embedding.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A font family offered for stamped text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FontFamily {
    /// A sans-serif face (default).
    #[default]
    Helvetica,
    /// A serif face.
    Times,
    /// A fixed-width face.
    Courier,
}

/// One of the 12 standard fonts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StandardFont {
    /// The family.
    pub family: FontFamily,
    /// Bold weight.
    pub bold: bool,
    /// Italic (oblique) style.
    pub italic: bool,
}

/// Width table rows: advance widths in 1/1000 em for the characters U+0020 to U+007E.
type Row = [u16; 95];

/// Advance widths of helvetica.
const HELVETICA: Row = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// Advance widths of helvetica-bold.
const HELVETICA_BOLD: Row = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667,
    611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556,
    278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// Advance widths of times.
const TIMES: Row = [
    250, 333, 408, 500, 500, 833, 778, 180, 333, 333, 500, 564, 250, 333, 250, 278, 500, 500, 500,
    500, 500, 500, 500, 500, 500, 500, 278, 278, 564, 564, 564, 444, 921, 722, 667, 667, 722, 611,
    556, 722, 722, 333, 389, 722, 611, 889, 722, 722, 556, 722, 667, 556, 611, 722, 722, 944, 722,
    722, 611, 333, 278, 333, 469, 500, 333, 444, 500, 444, 500, 444, 333, 500, 500, 278, 278, 500,
    278, 778, 500, 500, 500, 500, 333, 389, 278, 500, 500, 722, 500, 500, 444, 480, 200, 480, 541,
];

/// Advance widths of times-bold.
const TIMES_BOLD: Row = [
    250, 333, 555, 500, 500, 1000, 833, 278, 333, 333, 500, 570, 250, 333, 250, 278, 500, 500, 500,
    500, 500, 500, 500, 500, 500, 500, 333, 333, 570, 570, 570, 500, 930, 722, 667, 722, 722, 667,
    611, 778, 778, 389, 500, 778, 667, 944, 722, 778, 611, 778, 722, 556, 667, 722, 722, 1000, 722,
    722, 667, 333, 278, 333, 581, 500, 333, 500, 556, 444, 556, 444, 333, 500, 556, 278, 333, 556,
    278, 833, 556, 500, 556, 556, 444, 389, 333, 556, 500, 722, 500, 500, 444, 394, 220, 394, 520,
];

/// Advance widths of times-italic.
const TIMES_ITALIC: Row = [
    250, 333, 420, 500, 500, 833, 778, 214, 333, 333, 500, 675, 250, 333, 250, 278, 500, 500, 500,
    500, 500, 500, 500, 500, 500, 500, 333, 333, 675, 675, 675, 500, 920, 611, 611, 667, 722, 611,
    611, 722, 722, 333, 444, 667, 556, 833, 667, 722, 611, 722, 611, 500, 556, 722, 611, 833, 611,
    556, 556, 389, 278, 389, 422, 500, 333, 500, 500, 444, 500, 444, 278, 500, 500, 278, 278, 444,
    278, 722, 500, 500, 500, 500, 389, 389, 278, 500, 444, 667, 444, 444, 389, 400, 275, 400, 541,
];

/// Advance widths of times-bolditalic.
const TIMES_BOLD_ITALIC: Row = [
    250, 389, 555, 500, 500, 833, 778, 278, 333, 333, 500, 570, 250, 333, 250, 278, 500, 500, 500,
    500, 500, 500, 500, 500, 500, 500, 333, 333, 570, 570, 570, 500, 832, 667, 667, 667, 722, 667,
    667, 722, 778, 389, 500, 667, 611, 889, 722, 722, 611, 722, 667, 556, 611, 722, 667, 889, 667,
    611, 611, 333, 278, 333, 570, 500, 333, 500, 500, 444, 500, 444, 333, 500, 556, 278, 278, 500,
    278, 778, 556, 500, 500, 500, 389, 389, 278, 556, 444, 667, 500, 444, 389, 348, 220, 348, 570,
];

/// The cp1252 characters of the WinAnsi range 0x80 to 0x9F, with their widths for the serif and
/// sans-serif families (fixed-width fonts use 600 for everything).
const CP1252: [(char, u8, u16, u16); 27] = [
    ('\u{20AC}', 0x80, 556, 500),
    ('\u{201A}', 0x82, 222, 333),
    ('\u{0192}', 0x83, 556, 500),
    ('\u{201E}', 0x84, 333, 444),
    ('\u{2026}', 0x85, 1000, 1000),
    ('\u{2020}', 0x86, 556, 500),
    ('\u{2021}', 0x87, 556, 500),
    ('\u{02C6}', 0x88, 333, 333),
    ('\u{2030}', 0x89, 1000, 1000),
    ('\u{0160}', 0x8A, 667, 556),
    ('\u{2039}', 0x8B, 333, 333),
    ('\u{0152}', 0x8C, 1000, 889),
    ('\u{017D}', 0x8E, 611, 611),
    ('\u{2018}', 0x91, 222, 333),
    ('\u{2019}', 0x92, 222, 333),
    ('\u{201C}', 0x93, 333, 444),
    ('\u{201D}', 0x94, 333, 444),
    ('\u{2022}', 0x95, 350, 350),
    ('\u{2013}', 0x96, 556, 500),
    ('\u{2014}', 0x97, 1000, 1000),
    ('\u{02DC}', 0x98, 333, 333),
    ('\u{2122}', 0x99, 1000, 980),
    ('\u{0161}', 0x9A, 500, 389),
    ('\u{203A}', 0x9B, 333, 333),
    ('\u{0153}', 0x9C, 944, 722),
    ('\u{017E}', 0x9E, 500, 444),
    ('\u{0178}', 0x9F, 667, 722),
];

/// The plain letter an accented Latin-1 letter is built on, for measuring.
fn base_letter(c: char) -> Option<char> {
    Some(match c {
        '\u{C0}'..='\u{C5}' => 'A',
        '\u{C7}' => 'C',
        '\u{C8}'..='\u{CB}' => 'E',
        '\u{CC}'..='\u{CF}' => 'I',
        '\u{D1}' => 'N',
        '\u{D2}'..='\u{D6}' | '\u{D8}' => 'O',
        '\u{D9}'..='\u{DC}' => 'U',
        '\u{DD}' => 'Y',
        '\u{E0}'..='\u{E5}' => 'a',
        '\u{E7}' => 'c',
        '\u{E8}'..='\u{EB}' => 'e',
        '\u{EC}'..='\u{EF}' => 'i',
        '\u{F1}' => 'n',
        '\u{F2}'..='\u{F6}' | '\u{F8}' => 'o',
        '\u{F9}'..='\u{FC}' => 'u',
        '\u{FD}' | '\u{FF}' => 'y',
        _ => return None,
    })
}

impl StandardFont {
    /// Builds a font from a family and its style flags.
    pub const fn new(family: FontFamily, bold: bool, italic: bool) -> Self {
        Self {
            family,
            bold,
            italic,
        }
    }

    /// The PDF `BaseFont` name, such as `Helvetica-BoldOblique`.
    pub const fn pdf_name(self) -> &'static str {
        match (self.family, self.bold, self.italic) {
            (FontFamily::Helvetica, false, false) => "Helvetica",
            (FontFamily::Helvetica, true, false) => "Helvetica-Bold",
            (FontFamily::Helvetica, false, true) => "Helvetica-Oblique",
            (FontFamily::Helvetica, true, true) => "Helvetica-BoldOblique",
            (FontFamily::Times, false, false) => "Times-Roman",
            (FontFamily::Times, true, false) => "Times-Bold",
            (FontFamily::Times, false, true) => "Times-Italic",
            (FontFamily::Times, true, true) => "Times-BoldItalic",
            (FontFamily::Courier, false, false) => "Courier",
            (FontFamily::Courier, true, false) => "Courier-Bold",
            (FontFamily::Courier, false, true) => "Courier-Oblique",
            (FontFamily::Courier, true, true) => "Courier-BoldOblique",
        }
    }

    fn row(self) -> Option<&'static Row> {
        match (self.family, self.bold, self.italic) {
            (FontFamily::Helvetica, false, _) => Some(&HELVETICA),
            (FontFamily::Helvetica, true, _) => Some(&HELVETICA_BOLD),
            (FontFamily::Times, false, false) => Some(&TIMES),
            (FontFamily::Times, true, false) => Some(&TIMES_BOLD),
            (FontFamily::Times, false, true) => Some(&TIMES_ITALIC),
            (FontFamily::Times, true, true) => Some(&TIMES_BOLD_ITALIC),
            (FontFamily::Courier, _, _) => None,
        }
    }

    /// The WinAnsi byte of a character, or `None` when the font cannot draw it.
    pub fn encode_char(c: char) -> Option<u8> {
        let code = u32::from(c);
        match code {
            0x20..=0x7E | 0xA0..=0xFF => u8::try_from(code).ok(),
            _ => CP1252
                .iter()
                .find(|(ch, ..)| *ch == c)
                .map(|(_, byte, ..)| *byte),
        }
    }

    /// Encodes text as WinAnsi bytes. On failure returns the first character the font cannot
    /// draw.
    pub fn encode(text: &str) -> Result<Vec<u8>, char> {
        text.chars()
            .map(|c| Self::encode_char(c).ok_or(c))
            .collect()
    }

    /// The advance width of a character in 1/1000 em. Characters the font cannot draw count as
    /// the question mark.
    pub fn char_width(self, c: char) -> u16 {
        let Some(row) = self.row() else {
            return 600;
        };
        let serif = self.family == FontFamily::Times;
        let ascii = |ch: char| -> u16 {
            let index = (u32::from(ch) as usize).wrapping_sub(0x20);
            row.get(index).copied().unwrap_or(0)
        };
        match u32::from(c) {
            0x20..=0x7E => ascii(c),
            0xA0 => ascii(' '),
            _ => {
                if let Some(base) = base_letter(c) {
                    return ascii(base);
                }
                if let Some((_, _, sans, times)) = CP1252.iter().find(|(ch, ..)| *ch == c) {
                    return if serif { *times } else { *sans };
                }
                match Self::encode_char(c) {
                    // Other Latin-1 symbols and ligature-like letters: about one lowercase letter.
                    Some(_) => ascii('o'),
                    None => ascii('?'),
                }
            }
        }
    }

    /// The width of a text in points at the given font size.
    pub fn text_width(self, text: &str, size_pt: f32) -> f32 {
        let units: u32 = text.chars().map(|c| u32::from(self.char_width(c))).sum();
        units as f32 * size_pt / 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAMILIES: [FontFamily; 3] = [
        FontFamily::Helvetica,
        FontFamily::Times,
        FontFamily::Courier,
    ];

    #[test]
    fn names_follow_the_standard_14() {
        assert_eq!(
            StandardFont::new(FontFamily::Helvetica, true, true).pdf_name(),
            "Helvetica-BoldOblique"
        );
        assert_eq!(
            StandardFont::new(FontFamily::Times, false, false).pdf_name(),
            "Times-Roman"
        );
        assert_eq!(
            StandardFont::new(FontFamily::Courier, false, true).pdf_name(),
            "Courier-Oblique"
        );
    }

    #[test]
    fn widths_match_the_published_metrics() {
        let helvetica = StandardFont::new(FontFamily::Helvetica, false, false);
        assert_eq!(helvetica.char_width('A'), 667);
        assert_eq!(helvetica.char_width('i'), 222);
        assert_eq!(helvetica.char_width(' '), 278);
        assert_eq!(helvetica.char_width('@'), 1015);
        let times = StandardFont::new(FontFamily::Times, false, false);
        assert_eq!(times.char_width('W'), 944);
        let bold = StandardFont::new(FontFamily::Times, true, false);
        assert_eq!(bold.char_width('a'), 500);
        assert_eq!(
            StandardFont::new(FontFamily::Courier, true, true).char_width('W'),
            600
        );
    }

    #[test]
    fn text_width_scales_with_the_size() {
        let font = StandardFont::new(FontFamily::Helvetica, false, false);
        assert!((font.text_width("Page 1", 10.0) - 31.69).abs() < 0.01);
        assert_eq!(font.text_width("", 12.0), 0.0);
        let mono = StandardFont::new(FontFamily::Courier, false, false);
        assert_eq!(mono.text_width("abcd", 10.0), 24.0);
    }

    #[test]
    fn accented_letters_measure_like_their_base_and_every_font_has_every_width() {
        for family in FAMILIES {
            for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                let font = StandardFont::new(family, bold, italic);
                assert_eq!(font.char_width('\u{E9}'), font.char_width('e'));
                assert_eq!(font.char_width('\u{D6}'), font.char_width('O'));
                for code in 0x20u32..=0x7E {
                    let c = char::from_u32(code).unwrap_or(' ');
                    assert!(font.char_width(c) > 0, "{} {c:?}", font.pdf_name());
                }
            }
        }
    }

    #[test]
    fn winansi_covers_latin_text_and_rejects_the_rest() {
        assert_eq!(
            StandardFont::encode("Caf\u{E9} \u{20AC}5").unwrap(),
            b"Caf\xE9 \x805"
        );
        assert_eq!(
            StandardFont::encode("\u{201C}ok\u{201D}").unwrap(),
            b"\x93ok\x94"
        );
        assert_eq!(StandardFont::encode("na\u{EF}ve \u{4E2D}"), Err('\u{4E2D}'));
        assert_eq!(StandardFont::encode_char('\u{7F}'), None);
    }
}
