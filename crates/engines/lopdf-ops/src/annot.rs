//! What every annotation writer shares: moving points from the displayed page into user space,
//! text strings, colors, appearance streams and the page's annotation list.

use application::AppError;
use domain::stamp::Rgb;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};

use crate::pageres::num;
use crate::paint::Boxed;

/// A point of the displayed page (y up) in the page's user space.
pub fn to_user(frame: [f32; 6], x: f32, y: f32) -> (f32, f32) {
    (
        frame[0] * x + frame[2] * y + frame[4],
        frame[1] * x + frame[3] * y + frame[5],
    )
}

/// The user space rectangle `[x0, y0, x1, y1]` that holds a box of the displayed page.
pub fn user_rect(frame: [f32; 6], b: Boxed) -> [f32; 4] {
    let corners = [
        to_user(frame, b.x, b.y),
        to_user(frame, b.x + b.w, b.y),
        to_user(frame, b.x, b.y + b.h),
        to_user(frame, b.x + b.w, b.y + b.h),
    ];
    let xs = corners.map(|c| c.0);
    let ys = corners.map(|c| c.1);
    [
        xs.iter().copied().fold(f32::INFINITY, f32::min),
        ys.iter().copied().fold(f32::INFINITY, f32::min),
        xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),
    ]
}

/// A PDF text string: plain bytes when every character is ASCII, UTF-16 with a byte order mark
/// otherwise.
pub fn text_string(text: &str) -> Object {
    if text.is_ascii() {
        Object::String(text.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        Object::String(bytes, StringFormat::Hexadecimal)
    }
}

/// Reads a PDF text string back.
pub fn read_text(object: &Object) -> String {
    match object {
        Object::String(bytes, _) => {
            if bytes.starts_with(&[0xFE, 0xFF]) {
                let units: Vec<u16> = bytes[2..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| u16::from_be_bytes(*p))
                    .collect();
                String::from_utf16_lossy(&units)
            } else if bytes.starts_with(&[0xFF, 0xFE]) {
                let units: Vec<u16> = bytes[2..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| u16::from_le_bytes(*p))
                    .collect();
                String::from_utf16_lossy(&units)
            } else {
                // PDFDocEncoding agrees with Latin-1 for the characters people type.
                bytes.iter().map(|b| char::from(*b)).collect()
            }
        }
        Object::Name(name) => String::from_utf8_lossy(name).into_owned(),
        _ => String::new(),
    }
}

/// `[r g b]` as PDF reals.
pub fn color_array(color: &str) -> Result<Object, AppError> {
    let rgb = Rgb::parse(color).map_err(|_| {
        AppError::new(
            domain::ErrorCode::OptionInvalid,
            "A color is not written as #RRGGBB.",
        )
    })?;
    Ok(Object::Array(
        rgb.fractions().iter().map(|c| Object::Real(*c)).collect(),
    ))
}

/// The fill or stroke operator for a color, with a trailing newline.
pub fn color_op(color: &str, stroke: bool) -> Result<String, AppError> {
    let Object::Array(values) = color_array(color)? else {
        return Ok(String::new());
    };
    let parts: Vec<String> = values
        .iter()
        .map(|v| match v {
            Object::Real(r) => num(*r),
            _ => "0".to_owned(),
        })
        .collect();
    Ok(format!(
        "{} {}\n",
        parts.join(" "),
        if stroke { "RG" } else { "rg" }
    ))
}

/// A rectangle as a PDF array.
pub fn rect_array(r: [f32; 4]) -> Object {
    Object::Array(r.iter().map(|v| Object::Real(*v)).collect())
}

/// A form XObject, which is how an annotation's appearance is stored.
pub fn form_xobject(
    doc: &mut Document,
    bbox: [f32; 4],
    content: String,
    resources: Dictionary,
) -> ObjectId {
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Form".to_vec()));
    dict.set("BBox", rect_array(bbox));
    dict.set("Resources", Object::Dictionary(resources));
    doc.add_object(Stream::new(dict, content.into_bytes()))
}

/// Adds an annotation object to a page's `/Annots`, whether the list is direct, indirect or
/// missing.
pub fn add_to_page(doc: &mut Document, page: ObjectId, annot: ObjectId) -> Result<(), AppError> {
    let existing = doc
        .get_dictionary(page)
        .map_err(|_| AppError::internal("editing a page"))?
        .get(b"Annots")
        .ok()
        .cloned();
    match existing {
        Some(Object::Reference(list)) => {
            if let Ok(Object::Array(items)) = doc.get_object_mut(list) {
                items.push(Object::Reference(annot));
                return Ok(());
            }
        }
        Some(Object::Array(mut items)) => {
            items.push(Object::Reference(annot));
            doc.get_dictionary_mut(page)
                .map_err(|_| AppError::internal("editing a page"))?
                .set("Annots", Object::Array(items));
            return Ok(());
        }
        _ => {}
    }
    doc.get_dictionary_mut(page)
        .map_err(|_| AppError::internal("editing a page"))?
        .set("Annots", Object::Array(vec![Object::Reference(annot)]));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_follow_the_display_matrix() {
        // A page turned a quarter: displayed (x, y) lands on user (-y + w, x).
        let frame = [0.0, 1.0, -1.0, 0.0, 600.0, 0.0];
        assert_eq!(to_user(frame, 10.0, 20.0), (580.0, 10.0));
        let r = user_rect(
            frame,
            Boxed {
                x: 10.0,
                y: 20.0,
                w: 30.0,
                h: 5.0,
            },
        );
        assert_eq!(r, [575.0, 10.0, 580.0, 40.0]);
    }

    #[test]
    fn text_strings_round_trip_ascii_and_unicode() {
        for text in ["plain", "caf\u{e9}", "\u{4e2d}\u{6587} text", ""] {
            assert_eq!(read_text(&text_string(text)), text);
        }
        assert!(matches!(
            text_string("abc"),
            Object::String(_, StringFormat::Literal)
        ));
        let Object::String(bytes, _) = text_string("\u{4e2d}") else {
            panic!("not a string")
        };
        assert_eq!(&bytes[..2], [0xFE, 0xFF]);
    }

    #[test]
    fn annotations_join_direct_indirect_and_missing_lists() {
        let mut doc = Document::load_mem(&test_support::minimal_pdf(1)).unwrap();
        let page = doc.get_pages()[&1];
        let first = doc.add_object(Dictionary::new());
        add_to_page(&mut doc, page, first).unwrap();
        let second = doc.add_object(Dictionary::new());
        add_to_page(&mut doc, page, second).unwrap();
        // Move the list to an indirect array and add a third.
        let items = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .clone();
        let list = doc.add_object(items);
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Annots", Object::Reference(list));
        let third = doc.add_object(Dictionary::new());
        add_to_page(&mut doc, page, third).unwrap();
        let Ok(Object::Array(all)) = doc.get_object(list) else {
            panic!("list")
        };
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn colors_become_operators_and_arrays() {
        assert_eq!(color_op("#FF0000", false).unwrap(), "1 0 0 rg\n");
        assert_eq!(color_op("#000000", true).unwrap(), "0 0 0 RG\n");
        assert!(color_array("nope").is_err());
    }
}
