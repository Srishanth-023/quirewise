//! Document information and XMP metadata.

use application::AppError;
use domain::tools::document::{MetadataChange, MetadataField, Trapped};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};

/// A PDF text string: plain bytes for ASCII, UTF-16 with a byte order mark otherwise.
pub fn text_string(text: &str) -> Object {
    if text.is_ascii() {
        Object::String(text.as_bytes().to_vec(), StringFormat::Literal)
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        Object::String(bytes, StringFormat::Literal)
    }
}

/// Reads a PDF text string back to text (UTF-16 with BOM, UTF-8 with BOM, or Latin-1).
pub fn read_text(object: &Object) -> Option<String> {
    let Object::String(bytes, _) = object else {
        return None;
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        return Some(String::from_utf16_lossy(&units));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Some(String::from_utf8_lossy(rest).into_owned());
    }
    Some(bytes.iter().map(|b| char::from(*b)).collect())
}

/// The id of the document information dictionary, when the trailer names one that exists.
fn info_id(doc: &Document) -> Option<ObjectId> {
    let id = doc.trailer.get(b"Info").ok()?.as_reference().ok()?;
    doc.get_dictionary(id).ok().map(|_| id)
}

fn xmp_id(doc: &Document) -> Option<ObjectId> {
    let id = doc
        .catalog()
        .ok()?
        .get(b"Metadata")
        .ok()?
        .as_reference()
        .ok()?;
    doc.get_object(id).ok()?.as_stream().ok().map(|_| id)
}

/// `D:20261019143005Z` as `2026-10-19T14:30:05Z`. Anything else is returned as it is.
fn xmp_date(pdf_date: &str) -> String {
    let digits: String = pdf_date
        .trim_start_matches("D:")
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.len() < 8 {
        return pdf_date.to_owned();
    }
    let part = |from: usize, to: usize, default: &'static str| -> String {
        digits
            .get(from..to)
            .map_or_else(|| default.to_owned(), str::to_owned)
    };
    format!(
        "{}-{}-{}T{}:{}:{}Z",
        &digits[0..4],
        &digits[4..6],
        &digits[6..8],
        part(8, 10, "00"),
        part(10, 12, "00"),
        part(12, 14, "00"),
    )
}

pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// An XMP packet that says what the information dictionary says.
fn build_xmp(info: &Dictionary) -> String {
    let get = |key: &[u8]| info.get(key).ok().and_then(read_text);
    let mut body = String::new();
    let lang_alt = |tag: &str, value: &str| {
        format!(
            "<{tag}><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></{tag}>",
            escape(value)
        )
    };
    if let Some(v) = get(b"Title") {
        body.push_str(&lang_alt("dc:title", &v));
    }
    if let Some(v) = get(b"Author") {
        body.push_str(&format!(
            "<dc:creator><rdf:Seq><rdf:li>{}</rdf:li></rdf:Seq></dc:creator>",
            escape(&v)
        ));
    }
    if let Some(v) = get(b"Subject") {
        body.push_str(&lang_alt("dc:description", &v));
    }
    if let Some(v) = get(b"Keywords") {
        body.push_str(&format!("<pdf:Keywords>{}</pdf:Keywords>", escape(&v)));
    }
    if let Some(v) = get(b"Producer") {
        body.push_str(&format!("<pdf:Producer>{}</pdf:Producer>", escape(&v)));
    }
    if let Some(v) = get(b"Creator") {
        body.push_str(&format!(
            "<xmp:CreatorTool>{}</xmp:CreatorTool>",
            escape(&v)
        ));
    }
    if let Some(v) = get(b"CreationDate") {
        body.push_str(&format!(
            "<xmp:CreateDate>{}</xmp:CreateDate>",
            escape(&xmp_date(&v))
        ));
    }
    if let Some(v) = get(b"ModDate") {
        body.push_str(&format!(
            "<xmp:ModifyDate>{}</xmp:ModifyDate>",
            escape(&xmp_date(&v))
        ));
    }
    if let Ok(Object::Name(name)) = info.get(b"Trapped") {
        body.push_str(&format!(
            "<pdf:Trapped>{}</pdf:Trapped>",
            escape(&String::from_utf8_lossy(name))
        ));
    }
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
         <rdf:Description rdf:about=\"\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
         xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" \
         xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\">{body}</rdf:Description>\
         </rdf:RDF></x:xmpmeta>\n<?xpacket end=\"w\"?>"
    )
}

/// Applies `change` to the information dictionary. When the file already has an XMP packet it is
/// rewritten from the new values (a PDF/A claim in the old packet does not survive an edit).
pub fn set_metadata(doc: &mut Document, change: &MetadataChange) -> Result<(), AppError> {
    if change.remove_all {
        remove_all(doc);
        return Ok(());
    }
    let id = match info_id(doc) {
        Some(id) => id,
        None => {
            let id = doc.add_object(Dictionary::new());
            doc.trailer.set("Info", Object::Reference(id));
            id
        }
    };
    {
        let info = doc
            .get_dictionary_mut(id)
            .map_err(|_| AppError::internal("editing the document information"))?;
        for (field, value) in &change.fields {
            let key = field.info_key().as_bytes();
            match value {
                Some(text) => {
                    let object = match field {
                        MetadataField::CreationDate | MetadataField::ModDate => {
                            Object::String(text.as_bytes().to_vec(), StringFormat::Literal)
                        }
                        _ => text_string(text),
                    };
                    info.set(key.to_vec(), object);
                }
                None => {
                    info.remove(key);
                }
            }
        }
        if let Some(trapped) = change.trapped {
            info.set(
                "Trapped",
                Object::Name(Trapped::pdf_name(trapped).as_bytes().to_vec()),
            );
        }
    }
    if let Some(xmp) = xmp_id(doc) {
        let packet = build_xmp(
            doc.get_dictionary(id)
                .map_err(|_| AppError::internal("reading the document information"))?,
        );
        if let Ok(Object::Stream(stream)) = doc.get_object_mut(xmp) {
            stream.dict.remove(b"Filter");
            stream.dict.remove(b"DecodeParms");
            stream.set_plain_content(packet.into_bytes());
        }
    }
    Ok(())
}

/// Removes the information dictionary and the document-level XMP packet.
pub fn remove_all(doc: &mut Document) {
    remove_info(doc);
    remove_xmp(doc);
}

/// Removes the document information dictionary.
pub fn remove_info(doc: &mut Document) {
    if let Some(id) = info_id(doc) {
        doc.trailer.remove(b"Info");
        doc.objects.remove(&id);
    } else {
        doc.trailer.remove(b"Info");
    }
}

/// Removes XMP packets from the catalog, pages and other objects.
pub fn remove_xmp(doc: &mut Document) {
    if let Some(id) = xmp_id(doc) {
        doc.objects.remove(&id);
    }
    if let Ok(catalog) = doc.catalog_mut() {
        catalog.remove(b"Metadata");
    }
    // Pages, fonts and images may carry packets of their own.
    let mut streams: Vec<ObjectId> = Vec::new();
    for (id, object) in &doc.objects {
        if let Object::Dictionary(dict) = object
            && let Ok(Object::Reference(target)) = dict.get(b"Metadata")
            && matches!(doc.get_object(*target), Ok(Object::Stream(s))
                if s.dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|n| n == b"XML"))
        {
            streams.push(*id);
        }
    }
    for id in streams {
        if let Ok(dict) = doc.get_dictionary_mut(id) {
            dict.remove(b"Metadata");
        }
    }
    // Streams (images, forms) keep their own dictionary.
    for object in doc.objects.values_mut() {
        if let Object::Stream(stream) = object {
            stream.dict.remove(b"Metadata");
        }
    }
}

/// Keeps `Stream` in scope for the XMP rewrite above.
#[allow(dead_code)]
type XmpStream = Stream;

#[cfg(test)]
mod tests {
    use super::*;
    use domain::tools::document::MetadataOptions;
    use test_support::pdfs;

    fn load(bytes: &[u8]) -> Document {
        Document::load_mem(bytes).unwrap()
    }

    fn info_text(doc: &Document, key: &[u8]) -> Option<String> {
        let id = info_id(doc)?;
        doc.get_dictionary(id)
            .ok()?
            .get(key)
            .ok()
            .and_then(read_text)
    }

    fn change(options: MetadataOptions) -> MetadataChange {
        options.change().unwrap()
    }

    #[test]
    fn sets_replaces_and_clears_fields() {
        let mut doc = load(&pdfs::pdf_with_metadata("Old title", "Old author"));
        assert_eq!(info_text(&doc, b"Title").as_deref(), Some("Old title"));
        set_metadata(
            &mut doc,
            &change(MetadataOptions {
                title: Some("New title".into()),
                author: Some(String::new()),
                subject: Some("Quarterly".into()),
                creation_date: Some("2026-10-19T14:30:00".into()),
                trapped: Some(Trapped::False),
                ..MetadataOptions::default()
            }),
        )
        .unwrap();
        assert_eq!(info_text(&doc, b"Title").as_deref(), Some("New title"));
        assert_eq!(info_text(&doc, b"Author"), None);
        assert_eq!(info_text(&doc, b"Subject").as_deref(), Some("Quarterly"));
        assert_eq!(
            info_text(&doc, b"CreationDate").as_deref(),
            Some("D:20261019143000Z")
        );
        let id = info_id(&doc).unwrap();
        assert_eq!(
            doc.get_dictionary(id)
                .unwrap()
                .get(b"Trapped")
                .unwrap()
                .as_name()
                .unwrap(),
            b"False"
        );
    }

    #[test]
    fn creates_the_information_dictionary_when_there_is_none() {
        let mut doc = load(&pdfs::minimal_pdf_without_info());
        assert!(info_id(&doc).is_none());
        set_metadata(
            &mut doc,
            &change(MetadataOptions {
                title: Some("Fresh".into()),
                ..MetadataOptions::default()
            }),
        )
        .unwrap();
        assert_eq!(info_text(&doc, b"Title").as_deref(), Some("Fresh"));
    }

    #[test]
    fn non_ascii_text_round_trips_as_utf16() {
        let mut doc = load(&pdfs::pdf_with_metadata("a", "b"));
        set_metadata(
            &mut doc,
            &change(MetadataOptions {
                title: Some("Café – 日本".into()),
                ..MetadataOptions::default()
            }),
        )
        .unwrap();
        assert_eq!(info_text(&doc, b"Title").as_deref(), Some("Café – 日本"));
    }

    #[test]
    fn the_xmp_packet_follows_the_information_dictionary() {
        let mut doc = load(&pdfs::pdf_with_metadata_and_xmp("Old", "Writer"));
        let before = xmp_id(&doc).expect("fixture has XMP");
        set_metadata(
            &mut doc,
            &change(MetadataOptions {
                title: Some("A & B <draft>".into()),
                ..MetadataOptions::default()
            }),
        )
        .unwrap();
        let Object::Stream(stream) = doc.get_object(before).unwrap() else {
            panic!("xmp is a stream");
        };
        let xml = String::from_utf8(stream.get_plain_content().unwrap()).unwrap();
        assert!(xml.contains("A &amp; B &lt;draft&gt;"), "{xml}");
        assert!(xml.contains("Writer"), "author is kept in sync too");
        assert!(!xml.contains(">Old<"));
    }

    #[test]
    fn remove_all_drops_info_and_xmp() {
        let mut doc = load(&pdfs::pdf_with_metadata_and_xmp("T", "A"));
        set_metadata(
            &mut doc,
            &change(MetadataOptions {
                remove_all: true,
                ..MetadataOptions::default()
            }),
        )
        .unwrap();
        assert!(info_id(&doc).is_none());
        assert!(xmp_id(&doc).is_none());
        assert!(doc.catalog().unwrap().get(b"Metadata").is_err());
    }

    #[test]
    fn dates_convert_to_xmp_form() {
        assert_eq!(xmp_date("D:20261019143005Z"), "2026-10-19T14:30:05Z");
        assert_eq!(xmp_date("D:20261019"), "2026-10-19T00:00:00Z");
        assert_eq!(xmp_date("garbage"), "garbage");
    }
}
