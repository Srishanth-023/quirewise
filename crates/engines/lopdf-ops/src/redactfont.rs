//! What redaction needs to know about a font: how wide each character code is, how tall the
//! glyphs are, and whether codes are one byte or two. With these the places of the characters
//! that a text operator draws can be worked out from the content stream alone.

use domain::fonts::{FontFamily, StandardFont};
use lopdf::{Dictionary, Document, Object};
use std::collections::HashMap;

use crate::fonts::winansi_chars;

/// Glyph heights assumed when the font does not say, as fractions of the font size.
const DEFAULT_ASCENT: f32 = 0.8;
const DEFAULT_DESCENT: f32 = -0.2;

#[derive(Debug, Clone)]
enum Widths {
    /// `Widths` of a simple font: the list starts at `first`.
    Table {
        first: u32,
        list: Vec<f32>,
        missing: f32,
    },
    /// One of the 12 standard fonts, which have no width list of their own.
    Standard(StandardFont),
    /// `DW` and `W` of a CID font.
    Cid {
        default: f32,
        map: HashMap<u32, f32>,
    },
}

/// The measurements of one font resource.
#[derive(Debug, Clone)]
pub struct FontInfo {
    /// Codes are two bytes (an Identity-encoded composite font).
    pub two_byte: bool,
    /// False when advances cannot be worked out (other code maps, vertical writing, simple fonts
    /// without widths). Text in such a font cannot be located exactly.
    pub usable: bool,
    /// Scale from glyph space to text space: 0.001 for most fonts.
    unit: f32,
    widths: Widths,
    /// Top and bottom of the glyphs, as fractions of the font size.
    pub ascent: f32,
    pub descent: f32,
}

fn resolve<'a>(doc: &'a Document, object: &'a Object) -> &'a Object {
    match object {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(object),
        other => other,
    }
}

fn number(doc: &Document, object: &Object) -> Option<f32> {
    resolve(doc, object).as_float().ok()
}

fn dict<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    match resolve(doc, object) {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

/// The standard font a base font name stands for, when it names one.
fn standard_of(name: &[u8]) -> Option<StandardFont> {
    let text = String::from_utf8_lossy(name).to_ascii_lowercase();
    let text = text.rsplit('+').next().unwrap_or(&text).to_owned();
    let family = if text.contains("courier") {
        FontFamily::Courier
    } else if text.contains("times") {
        FontFamily::Times
    } else if text.contains("helvetica") || text.contains("arial") {
        FontFamily::Helvetica
    } else {
        return None;
    };
    Some(StandardFont::new(
        family,
        text.contains("bold"),
        text.contains("italic") || text.contains("oblique"),
    ))
}

fn height_of(doc: &Document, descriptor: Option<&Dictionary>) -> (f32, f32) {
    let get = |key: &[u8]| {
        descriptor
            .and_then(|d| d.get(key).ok())
            .and_then(|o| number(doc, o))
    };
    let ascent = get(b"Ascent")
        .map(|a| (a / 1000.0).clamp(0.5, 1.2))
        .unwrap_or(DEFAULT_ASCENT);
    let descent = get(b"Descent")
        .map(|d| (d / 1000.0).clamp(-0.5, 0.0))
        .unwrap_or(DEFAULT_DESCENT);
    (ascent, descent)
}

/// `W` of a CID font: `c [w1 w2 ...]` and `first last w` entries.
fn cid_widths(doc: &Document, array: &[Object]) -> HashMap<u32, f32> {
    let mut map = HashMap::new();
    let mut i = 0;
    while i < array.len() {
        let Some(first) = number(doc, &array[i]) else {
            break;
        };
        let first = first as u32;
        match array.get(i + 1).map(|o| resolve(doc, o)) {
            Some(Object::Array(list)) => {
                for (k, w) in list.iter().enumerate() {
                    if let Some(w) = number(doc, w) {
                        map.insert(first + k as u32, w);
                    }
                }
                i += 2;
            }
            Some(_) => {
                let last = array.get(i + 1).and_then(|o| number(doc, o));
                let width = array.get(i + 2).and_then(|o| number(doc, o));
                if let (Some(last), Some(width)) = (last, width) {
                    let last = (last as u32).min(first.saturating_add(70_000));
                    for code in first..=last {
                        map.insert(code, width);
                    }
                }
                i += 3;
            }
            None => break,
        }
    }
    map
}

impl FontInfo {
    /// Reads a font dictionary. Never fails: what cannot be understood makes the font unusable.
    pub fn load(doc: &Document, font: &Dictionary) -> Self {
        let subtype = font
            .get(b"Subtype")
            .ok()
            .and_then(|o| resolve(doc, o).as_name().ok())
            .unwrap_or(b"");
        let unusable = |widths: Widths| Self {
            two_byte: false,
            usable: false,
            unit: 0.001,
            widths,
            ascent: DEFAULT_ASCENT,
            descent: DEFAULT_DESCENT,
        };
        if subtype == b"Type0" {
            return Self::composite(doc, font).unwrap_or_else(|| {
                unusable(Widths::Cid {
                    default: 1000.0,
                    map: HashMap::new(),
                })
            });
        }
        let base = font
            .get(b"BaseFont")
            .ok()
            .and_then(|o| resolve(doc, o).as_name().ok())
            .unwrap_or(b"");
        let descriptor = font.get(b"FontDescriptor").ok().and_then(|o| dict(doc, o));
        let (ascent, descent) = height_of(doc, descriptor);
        let unit = if subtype == b"Type3" {
            font.get(b"FontMatrix")
                .ok()
                .and_then(|o| match resolve(doc, o) {
                    Object::Array(m) => m.first().and_then(|v| number(doc, v)),
                    _ => None,
                })
                .filter(|u| *u > 0.0)
                .unwrap_or(0.001)
        } else {
            0.001
        };
        let list: Option<Vec<f32>> = font
            .get(b"Widths")
            .ok()
            .and_then(|o| match resolve(doc, o) {
                Object::Array(items) => items.iter().map(|v| number(doc, v)).collect(),
                _ => None,
            });
        let first = font
            .get(b"FirstChar")
            .ok()
            .and_then(|o| number(doc, o))
            .map_or(0, |f| f.max(0.0) as u32);
        let missing = descriptor
            .and_then(|d| d.get(b"MissingWidth").ok())
            .and_then(|o| number(doc, o))
            .unwrap_or(0.0);
        match (list, standard_of(base)) {
            (Some(list), _) => Self {
                two_byte: false,
                usable: true,
                unit,
                widths: Widths::Table {
                    first,
                    list,
                    missing,
                },
                ascent,
                descent,
            },
            (None, Some(standard)) => Self {
                two_byte: false,
                usable: true,
                unit,
                widths: Widths::Standard(standard),
                ascent: if descriptor.is_some() { ascent } else { 0.72 },
                descent: if descriptor.is_some() { descent } else { -0.21 },
            },
            (None, None) => unusable(Widths::Table {
                first: 0,
                list: Vec::new(),
                missing: 0.0,
            }),
        }
    }

    fn composite(doc: &Document, font: &Dictionary) -> Option<Self> {
        let encoding = resolve(doc, font.get(b"Encoding").ok()?).as_name().ok()?;
        // Identity-V is vertical writing, and other code maps have codes of mixed length.
        if encoding != b"Identity-H" {
            return None;
        }
        let descendants = match resolve(doc, font.get(b"DescendantFonts").ok()?) {
            Object::Array(items) => items,
            _ => return None,
        };
        let cid = dict(doc, descendants.first()?)?;
        let default = cid
            .get(b"DW")
            .ok()
            .and_then(|o| number(doc, o))
            .unwrap_or(1000.0);
        let map = match cid.get(b"W").ok().map(|o| resolve(doc, o)) {
            Some(Object::Array(items)) => cid_widths(doc, items),
            _ => HashMap::new(),
        };
        let descriptor = cid.get(b"FontDescriptor").ok().and_then(|o| dict(doc, o));
        let (ascent, descent) = height_of(doc, descriptor);
        Some(Self {
            two_byte: true,
            usable: true,
            unit: 0.001,
            widths: Widths::Cid { default, map },
            ascent,
            descent,
        })
    }

    /// The advance of a character code, as a fraction of the font size.
    pub fn advance(&self, code: u32) -> f32 {
        let glyph = match &self.widths {
            Widths::Table {
                first,
                list,
                missing,
            } => code
                .checked_sub(*first)
                .and_then(|i| list.get(i as usize))
                .copied()
                .unwrap_or(*missing),
            Widths::Standard(font) => {
                let ch = if code >= 32 {
                    winansi_chars()
                        .get(code as usize - 32)
                        .copied()
                        .filter(|c| *c != '\0')
                        .unwrap_or(' ')
                } else {
                    ' '
                };
                f32::from(font.char_width(ch))
            }
            Widths::Cid { default, map } => map.get(&code).copied().unwrap_or(*default),
        };
        glyph * self.unit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn font(extra: Dictionary) -> Dictionary {
        let mut d = dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"};
        for (k, v) in extra.iter() {
            d.set(k.clone(), v.clone());
        }
        d
    }

    #[test]
    fn a_standard_font_without_widths_uses_the_built_in_table() {
        let doc = Document::new();
        let info = FontInfo::load(&doc, &font(Dictionary::new()));
        assert!(info.usable && !info.two_byte);
        // Helvetica "A" is 667 units wide, a space 278.
        assert!((info.advance(u32::from(b'A')) - 0.667).abs() < 1e-4);
        assert!((info.advance(32) - 0.278).abs() < 1e-4);
    }

    #[test]
    fn a_width_list_wins_and_codes_outside_it_use_the_missing_width() {
        let doc = Document::new();
        let f = font(dictionary! {
            "FirstChar" => 65,
            "Widths" => vec![Object::Integer(700), Object::Integer(710)],
        });
        let info = FontInfo::load(&doc, &f);
        assert!((info.advance(65) - 0.7).abs() < 1e-4);
        assert!((info.advance(66) - 0.71).abs() < 1e-4);
        assert!(info.advance(67).abs() < 1e-4);
        assert!(info.advance(10).abs() < 1e-4);
    }

    #[test]
    fn a_font_with_no_widths_and_no_known_name_cannot_be_measured() {
        let doc = Document::new();
        let f =
            dictionary! {"Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "ABCDEF+Custom"};
        assert!(!FontInfo::load(&doc, &f).usable);
    }

    #[test]
    fn subset_prefixes_and_style_words_pick_the_standard_font() {
        let font = standard_of(b"ABCDEF+Arial-BoldMT").unwrap();
        assert_eq!(font, StandardFont::new(FontFamily::Helvetica, true, false));
        assert_eq!(
            standard_of(b"Times-BoldItalic").unwrap(),
            StandardFont::new(FontFamily::Times, true, true)
        );
        assert!(standard_of(b"Garamond").is_none());
    }

    #[test]
    fn identity_composite_fonts_read_dw_and_w_in_both_forms() {
        let mut doc = Document::new();
        let cid = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "CIDFontType2",
            "DW" => 800,
            "W" => vec![
                Object::Integer(1),
                Object::Array(vec![Object::Integer(500), Object::Integer(600)]),
                Object::Integer(10),
                Object::Integer(12),
                Object::Integer(300),
            ],
        });
        let f = dictionary! {
            "Type" => "Font",
            "Subtype" => "Type0",
            "Encoding" => "Identity-H",
            "DescendantFonts" => vec![Object::Reference(cid)],
        };
        let info = FontInfo::load(&doc, &f);
        assert!(info.usable && info.two_byte);
        assert!((info.advance(1) - 0.5).abs() < 1e-4);
        assert!((info.advance(2) - 0.6).abs() < 1e-4);
        assert!((info.advance(11) - 0.3).abs() < 1e-4);
        assert!((info.advance(99) - 0.8).abs() < 1e-4);
    }

    #[test]
    fn other_code_maps_and_vertical_writing_are_not_measured() {
        let doc = Document::new();
        for encoding in ["UniJIS-UCS2-H", "Identity-V"] {
            let f = dictionary! {
                "Type" => "Font",
                "Subtype" => "Type0",
                "Encoding" => encoding,
                "DescendantFonts" => Vec::<Object>::new(),
            };
            assert!(!FontInfo::load(&doc, &f).usable, "{encoding}");
        }
    }
}
