//! Helpers every page edit shares: a page's own resources, new content streams around the old
//! ones, and the number formats content streams are written in.

use application::AppError;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::boxes::inherited;
use crate::edit::content_ids;

/// A decimal for a content stream: no exponent, trailing zeros trimmed.
pub fn num(v: f32) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let text = format!("{v:.4}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" || trimmed == "-0" {
        "0".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// `a b c d e f cm`.
pub fn cm(m: [f32; 6]) -> String {
    format!(
        "{} {} {} {} {} {} cm\n",
        num(m[0]),
        num(m[1]),
        num(m[2]),
        num(m[3]),
        num(m[4]),
        num(m[5])
    )
}

/// The matrix `a` applied first, then `b`.
pub fn multiply(a: [f32; 6], b: [f32; 6]) -> [f32; 6] {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

/// A page's resources as the page sees them (inherited ones included), as a dictionary of its
/// own whose categories are direct dictionaries. Changing the copy never touches shared objects.
pub fn effective_resources(doc: &Document, page: ObjectId) -> Dictionary {
    let resolve = |object: &Object| -> Option<Dictionary> {
        match object {
            Object::Dictionary(d) => Some(d.clone()),
            Object::Reference(id) => doc.get_dictionary(*id).ok().cloned(),
            _ => None,
        }
    };
    let mut resources = inherited(doc, page, b"Resources")
        .and_then(resolve)
        .unwrap_or_default();
    let keys: Vec<Vec<u8>> = resources.iter().map(|(k, _)| k.clone()).collect();
    for key in keys {
        if let Ok(value) = resources.get(&key)
            && let Object::Reference(id) = value
            && let Ok(Object::Dictionary(d)) = doc.get_object(*id)
        {
            let direct = d.clone();
            resources.set(key, Object::Dictionary(direct));
        }
    }
    resources
}

/// Puts `resources` on the page as its own direct dictionary.
pub fn set_resources(
    doc: &mut Document,
    page: ObjectId,
    resources: Dictionary,
) -> Result<(), AppError> {
    doc.get_dictionary_mut(page)
        .map_err(|_| AppError::internal("editing a page"))?
        .set("Resources", Object::Dictionary(resources));
    Ok(())
}

/// Adds `value` to a category of `resources` (`Font`, `ExtGState`, `XObject`) under a name that
/// starts with `prefix` and is not taken yet. Returns the name.
pub fn add_named(
    resources: &mut Dictionary,
    category: &str,
    prefix: &str,
    value: Object,
) -> String {
    let mut table = match resources.get(category.as_bytes()) {
        Ok(Object::Dictionary(d)) => d.clone(),
        _ => Dictionary::new(),
    };
    let mut n = 0u32;
    let name = loop {
        let candidate = format!("{prefix}{n}");
        if table.get(candidate.as_bytes()).is_err() {
            break candidate;
        }
        n += 1;
    };
    table.set(name.clone(), value);
    resources.set(category, Object::Dictionary(table));
    name
}

/// Wraps the page's content in `q ... Q` and adds streams before and after it. The wrap makes
/// the added content start from the default graphics state even when the page content leaves
/// the state unbalanced.
pub fn wrap_content(
    doc: &mut Document,
    page: ObjectId,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
) -> Result<(), AppError> {
    let existing = content_ids(doc, page);
    let mut streams: Vec<Object> = Vec::new();
    if let Some(bytes) = before {
        streams.push(Object::Reference(
            doc.add_object(Stream::new(Dictionary::new(), bytes)),
        ));
    }
    streams.push(Object::Reference(
        doc.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec())),
    ));
    streams.extend(existing);
    streams.push(Object::Reference(
        doc.add_object(Stream::new(Dictionary::new(), b"\nQ\n".to_vec())),
    ));
    if let Some(bytes) = after {
        streams.push(Object::Reference(
            doc.add_object(Stream::new(Dictionary::new(), bytes)),
        ));
    }
    doc.get_dictionary_mut(page)
        .map_err(|_| AppError::internal("editing a page"))?
        .set("Contents", Object::Array(streams));
    Ok(())
}

/// The object id of page `index` (zero-based).
pub fn page_object(doc: &Document, index: u32) -> Result<ObjectId, AppError> {
    doc.get_pages()
        .get(&(index + 1))
        .copied()
        .ok_or_else(|| AppError::internal("finding a page to edit"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_short_and_never_use_exponents() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(0.5), "0.5");
        assert_eq!(num(-2.25), "-2.25");
        assert_eq!(num(0.0), "0");
        assert_eq!(num(-0.00001), "0");
        assert_eq!(num(1e-9), "0");
        assert_eq!(num(123_456.79), "123456.7891");
        assert_eq!(num(f32::NAN), "0");
        assert_eq!(num(f32::INFINITY), "0");
    }

    #[test]
    fn matrices_multiply_in_content_stream_order() {
        // Scale by 2, then move by (10, 20): the point (1, 1) lands on (12, 22).
        let m = multiply(
            [2.0, 0.0, 0.0, 2.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 1.0, 10.0, 20.0],
        );
        assert_eq!(m, [2.0, 0.0, 0.0, 2.0, 10.0, 20.0]);
        assert_eq!(cm(m), "2 0 0 2 10 20 cm\n");
    }

    #[test]
    fn added_names_never_collide_with_existing_ones() {
        let mut resources = Dictionary::new();
        let first = add_named(&mut resources, "Font", "QwF", Object::Null);
        let second = add_named(&mut resources, "Font", "QwF", Object::Null);
        assert_eq!((first.as_str(), second.as_str()), ("QwF0", "QwF1"));
        let Ok(Object::Dictionary(fonts)) = resources.get(b"Font") else {
            panic!("no font table");
        };
        assert_eq!(fonts.len(), 2);
    }
}
