//! Whole-document scan: JavaScript, embedded files, PDF/A claim and byte usage by kind.

use application::inspect::{SizeBreakdown, StructureReport};
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::BTreeSet;

use crate::encryption;

fn name_is(dict: &Dictionary, key: &[u8], value: &[u8]) -> bool {
    matches!(dict.get(key), Ok(Object::Name(n)) if n == value)
}

/// JavaScript or an automatic action anywhere inside `object`: a `/JS` entry or an action
/// of type JavaScript, including in nested dictionaries and arrays.
fn object_has_javascript(object: &Object) -> bool {
    match object {
        Object::Dictionary(dict) => dict_has_javascript(dict),
        Object::Stream(stream) => dict_has_javascript(&stream.dict),
        Object::Array(items) => items.iter().any(object_has_javascript),
        _ => false,
    }
}

fn dict_has_javascript(dict: &Dictionary) -> bool {
    dict.has(b"JS")
        || name_is(dict, b"S", b"JavaScript")
        || dict.iter().any(|(_, value)| object_has_javascript(value))
}

fn content_stream_ids(doc: &Document) -> BTreeSet<ObjectId> {
    let mut ids = BTreeSet::new();
    for page_id in doc.get_pages().values() {
        let Ok(page) = doc.get_dictionary(*page_id) else {
            continue;
        };
        match page.get(b"Contents") {
            Ok(Object::Reference(id)) => {
                // A reference may point at one stream or at an array of streams.
                match doc.get_object(*id) {
                    Ok(Object::Array(items)) => {
                        ids.extend(items.iter().filter_map(|o| o.as_reference().ok()));
                    }
                    _ => {
                        ids.insert(*id);
                    }
                }
            }
            Ok(Object::Array(items)) => {
                ids.extend(items.iter().filter_map(|o| o.as_reference().ok()));
            }
            _ => {}
        }
    }
    ids
}

/// Reads the PDF/A claim (`pdfaid:part` and `pdfaid:conformance`) from the catalog XMP stream.
fn pdfa_claim(doc: &Document) -> Option<String> {
    let catalog = doc.catalog().ok()?;
    let (_, meta) = doc.dereference(catalog.get(b"Metadata").ok()?).ok()?;
    let stream = meta.as_stream().ok()?;
    let bytes = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    let xmp = String::from_utf8_lossy(&bytes);
    let part = xmp_value(&xmp, "pdfaid:part")?;
    let conformance = xmp_value(&xmp, "pdfaid:conformance").unwrap_or_default();
    Some(format!("PDF/A-{part}{}", conformance.to_lowercase()))
}

/// Value of an XMP property written either as an element or as an attribute.
fn xmp_value(xmp: &str, name: &str) -> Option<String> {
    let element = format!("<{name}>");
    if let Some(start) = xmp.find(&element) {
        let rest = &xmp[start + element.len()..];
        return rest.split('<').next().map(|v| v.trim().to_owned());
    }
    let attribute = format!("{name}=\"");
    let start = xmp.find(&attribute)? + attribute.len();
    xmp[start..].split('"').next().map(str::to_owned)
}

/// Builds the structure report for a document that loaded.
pub fn report(doc: &Document, file_size: u64) -> StructureReport {
    let contents = content_stream_ids(doc);
    let mut has_javascript = false;
    let mut embedded_files = 0u32;
    let mut sizes = SizeBreakdown::default();

    for (id, object) in &doc.objects {
        let dict = match object {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        };
        if let Some(d) = dict {
            has_javascript = has_javascript || dict_has_javascript(d);
            if name_is(d, b"Type", b"Filespec") && d.has(b"EF") {
                embedded_files += 1;
            }
        }
        if let Object::Stream(stream) = object {
            let len = stream.content.len() as u64;
            let d = &stream.dict;
            if name_is(d, b"Subtype", b"Image") {
                sizes.images += len;
            } else if d.has(b"Length1")
                || name_is(d, b"Subtype", b"Type1C")
                || name_is(d, b"Subtype", b"CIDFontType0C")
                || name_is(d, b"Subtype", b"OpenType")
            {
                sizes.fonts += len;
            } else if contents.contains(id) {
                sizes.content += len;
            }
        }
    }
    sizes.other = file_size.saturating_sub(sizes.images + sizes.fonts + sizes.content);

    let encryption = encryption::describe(doc);
    let (encryption, permissions) = match encryption {
        Some((info, perms)) => (Some(info), perms),
        None => (None, None),
    };
    StructureReport {
        encryption,
        permissions,
        has_javascript,
        pdfa_claim: pdfa_claim(doc),
        size_breakdown: sizes,
        embedded_files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xmp_values_read_from_elements_and_attributes() {
        let element = "<rdf:Description><pdfaid:part>2</pdfaid:part><pdfaid:conformance>B</pdfaid:conformance>";
        assert_eq!(xmp_value(element, "pdfaid:part").as_deref(), Some("2"));
        assert_eq!(
            xmp_value(element, "pdfaid:conformance").as_deref(),
            Some("B")
        );
        let attribute = "<rdf:Description pdfaid:part=\"1\" pdfaid:conformance=\"A\"/>";
        assert_eq!(xmp_value(attribute, "pdfaid:part").as_deref(), Some("1"));
        assert_eq!(xmp_value("<nothing/>", "pdfaid:part"), None);
    }
}
