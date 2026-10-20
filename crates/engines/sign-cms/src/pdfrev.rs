//! Appending a revision to a PDF file: new and changed objects, a cross-reference section and a
//! trailer that points back to the previous one. The bytes before the revision are never
//! touched, so earlier signatures stay valid.

use std::collections::BTreeMap;

use application::AppError;
use domain::ErrorCode;
use lopdf::xref::XrefType;
use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};

/// A PDF in the middle of getting a new revision.
pub struct Revision {
    original: Vec<u8>,
    doc: Document,
    next_id: u32,
    changed: BTreeMap<ObjectId, Object>,
    streams: bool,
    prev_xref: usize,
}

/// A finished revision: the whole new file and where each new object starts.
pub struct Finished {
    /// The original bytes followed by the revision.
    pub bytes: Vec<u8>,
    /// Offset of every object written, by number.
    pub offsets: BTreeMap<u32, usize>,
}

fn damaged(why: &str) -> AppError {
    AppError::new(
        ErrorCode::PdfDamaged,
        format!(
            "The PDF cannot be signed because {why}. Run Repair first, then sign the repaired copy."
        ),
    )
}

impl Revision {
    /// Reads `bytes` and prepares to add a revision. Files that are protected with a password
    /// are refused: a signature over an encrypted file would need the encryption of the new
    /// objects, and removing the password would change what the person asked for.
    pub fn open(bytes: &[u8]) -> Result<Self, AppError> {
        let doc = Document::load_mem(bytes).map_err(|_| damaged("it could not be read"))?;
        if doc.is_encrypted() || doc.trailer.has(b"Encrypt") {
            return Err(AppError::new(
                ErrorCode::PdfEncrypted,
                "This PDF is protected with a password, and a protected file cannot be signed here. Unlock it first, sign the copy, and protect nothing afterwards because that would break the signature.",
            ));
        }
        let prev_xref = doc.xref_start;
        let at = bytes
            .get(prev_xref..)
            .ok_or_else(|| damaged("its cross-reference table is missing"))?;
        let starts_well = at.starts_with(b"xref") || at.first().is_some_and(u8::is_ascii_digit);
        if !starts_well {
            return Err(damaged("its cross-reference table is damaged"));
        }
        let size_in_trailer = doc
            .trailer
            .get(b"Size")
            .ok()
            .and_then(|o| o.as_i64().ok())
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0);
        let next_id = doc.max_id.max(size_in_trailer.saturating_sub(1)) + 1;
        let streams = matches!(
            doc.reference_table.cross_reference_type,
            XrefType::CrossReferenceStream
        );
        Ok(Self {
            original: bytes.to_vec(),
            doc,
            next_id,
            changed: BTreeMap::new(),
            streams,
            prev_xref,
        })
    }

    /// The document as it was read, for looking things up.
    pub fn doc(&self) -> &Document {
        &self.doc
    }

    /// The object with this id as it will be written: the changed copy when there is one.
    pub fn get(&self, id: ObjectId) -> Option<&Object> {
        self.changed.get(&id).or_else(|| self.doc.objects.get(&id))
    }

    /// Follows references until a direct object.
    #[cfg(test)]
    pub fn resolve<'a>(&'a self, mut object: &'a Object) -> Option<&'a Object> {
        for _ in 0..32 {
            match object {
                Object::Reference(id) => object = self.get(*id)?,
                other => return Some(other),
            }
        }
        None
    }

    /// Adds a new object and returns its id.
    pub fn add(&mut self, object: Object) -> ObjectId {
        let id = (self.next_id, 0);
        self.next_id += 1;
        self.changed.insert(id, object);
        id
    }

    /// Replaces an existing object.
    pub fn replace(&mut self, id: ObjectId, object: Object) {
        self.changed.insert(id, object);
    }

    /// The dictionary of an object, following references.
    #[cfg(test)]
    pub fn dict<'a>(&'a self, object: &'a Object) -> Option<&'a Dictionary> {
        match self.resolve(object)? {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }

    /// The id of the catalog.
    pub fn root(&self) -> Result<ObjectId, AppError> {
        self.doc
            .trailer
            .get(b"Root")
            .ok()
            .and_then(|o| o.as_reference().ok())
            .ok_or_else(|| damaged("it has no document catalog"))
    }

    /// Writes the revision and returns the whole file.
    pub fn finish(mut self) -> Result<Finished, AppError> {
        let mut out = self.original.clone();
        if !out.ends_with(b"\n") && !out.ends_with(b"\r") {
            out.push(b'\n');
        }
        let mut offsets: BTreeMap<u32, usize> = BTreeMap::new();
        let ids: Vec<ObjectId> = self.changed.keys().copied().collect();
        for id in &ids {
            offsets.insert(id.0, out.len());
            out.extend_from_slice(format!("{} {} obj\n", id.0, id.1).as_bytes());
            if let Some(object) = self.changed.get(id) {
                write_object(object, &mut out);
            }
            out.extend_from_slice(b"\nendobj\n");
        }
        let root = self.root()?;
        let trailer_ref =
            |key: &[u8]| -> Option<Object> { self.doc.trailer.get(key).ok().cloned() };
        let xref_at = out.len();
        let mut trailer = Dictionary::new();
        trailer.set("Root", Object::Reference(root));
        for key in ["Info", "ID"] {
            if let Some(v) = trailer_ref(key.as_bytes()) {
                trailer.set(key, v);
            }
        }
        trailer.set(
            "Prev",
            Object::Integer(i64::try_from(self.prev_xref).map_err(|_| damaged("it is too large"))?),
        );
        if self.streams {
            let id = self.next_id;
            self.next_id += 1;
            offsets.insert(id, xref_at);
            let size = i64::from(self.next_id);
            trailer.set("Size", Object::Integer(size));
            trailer.set("Type", Object::Name(b"XRef".to_vec()));
            trailer.set(
                "W",
                Object::Array(vec![
                    Object::Integer(1),
                    Object::Integer(4),
                    Object::Integer(2),
                ]),
            );
            let runs = runs(offsets.keys().copied());
            trailer.set(
                "Index",
                Object::Array(
                    runs.iter()
                        .flat_map(|(start, count)| {
                            [
                                Object::Integer(i64::from(*start)),
                                Object::Integer(i64::from(*count)),
                            ]
                        })
                        .collect(),
                ),
            );
            let mut data = Vec::new();
            for offset in offsets.values() {
                let offset =
                    u32::try_from(*offset).map_err(|_| damaged("it is larger than 4 GB"))?;
                data.push(1);
                data.extend_from_slice(&offset.to_be_bytes());
                data.extend_from_slice(&0u16.to_be_bytes());
            }
            trailer.set(
                "Length",
                Object::Integer(i64::try_from(data.len()).unwrap_or(0)),
            );
            out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
            write_dictionary(&trailer, &mut out);
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(&data);
            out.extend_from_slice(b"\nendstream\nendobj\n");
        } else {
            trailer.set("Size", Object::Integer(i64::from(self.next_id)));
            out.extend_from_slice(b"xref\n");
            for (start, count) in runs(offsets.keys().copied()) {
                out.extend_from_slice(format!("{start} {count}\n").as_bytes());
                for number in start..start + count {
                    let offset = offsets.get(&number).copied().unwrap_or(0);
                    out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
                }
            }
            out.extend_from_slice(b"trailer\n");
            write_dictionary(&trailer, &mut out);
            out.push(b'\n');
        }
        out.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
        Ok(Finished {
            bytes: out,
            offsets,
        })
    }
}

/// Groups sorted numbers into runs of consecutive ones: `(first, count)`.
fn runs(numbers: impl Iterator<Item = u32>) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for n in numbers {
        match out.last_mut() {
            Some((start, count)) if *start + *count == n => *count += 1,
            _ => out.push((n, 1)),
        }
    }
    out
}

fn write_name(name: &[u8], out: &mut Vec<u8>) {
    out.push(b'/');
    for &b in name {
        let regular = b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'_' | b'.' | b'+' | b'*' | b'\'' | b'~');
        if regular {
            out.push(b);
        } else {
            out.extend_from_slice(format!("#{b:02X}").as_bytes());
        }
    }
}

fn write_string(bytes: &[u8], format: StringFormat, out: &mut Vec<u8>) {
    match format {
        StringFormat::Hexadecimal => {
            out.push(b'<');
            for b in bytes {
                out.extend_from_slice(format!("{b:02X}").as_bytes());
            }
            out.push(b'>');
        }
        StringFormat::Literal => {
            out.push(b'(');
            for &b in bytes {
                match b {
                    b'(' | b')' | b'\\' => {
                        out.push(b'\\');
                        out.push(b);
                    }
                    b'\r' => out.extend_from_slice(b"\\r"),
                    b'\n' => out.extend_from_slice(b"\\n"),
                    other => out.push(other),
                }
            }
            out.push(b')');
        }
    }
}

fn write_real(value: f32, out: &mut Vec<u8>) {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e9 {
        out.extend_from_slice(format!("{}", value as i64).as_bytes());
    } else if value.is_finite() {
        let text = format!("{value:.5}");
        out.extend_from_slice(text.trim_end_matches('0').trim_end_matches('.').as_bytes());
    } else {
        out.push(b'0');
    }
}

/// Writes a dictionary.
pub fn write_dictionary(dict: &Dictionary, out: &mut Vec<u8>) {
    out.extend_from_slice(b"<<");
    for (key, value) in dict.iter() {
        write_name(key, out);
        out.push(b' ');
        write_object(value, out);
        out.push(b' ');
    }
    out.extend_from_slice(b">>");
}

/// Writes an object the way it appears in a PDF file.
pub fn write_object(object: &Object, out: &mut Vec<u8>) {
    match object {
        Object::Null => out.extend_from_slice(b"null"),
        Object::Boolean(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Object::Integer(i) => out.extend_from_slice(i.to_string().as_bytes()),
        Object::Real(r) => write_real(*r, out),
        Object::Name(n) => write_name(n, out),
        Object::String(s, f) => write_string(s, *f, out),
        Object::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                write_object(item, out);
            }
            out.push(b']');
        }
        Object::Dictionary(d) => write_dictionary(d, out),
        Object::Stream(s) => {
            let mut dict = s.dict.clone();
            dict.set(
                "Length",
                Object::Integer(i64::try_from(s.content.len()).unwrap_or(0)),
            );
            write_dictionary(&dict, out);
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(&s.content);
            out.extend_from_slice(b"\nendstream");
        }
        Object::Reference((n, g)) => out.extend_from_slice(format!("{n} {g} R").as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn sample() -> Vec<u8> {
        test_support::pdfs::pdf_with_texts(&["one", "two"])
    }

    #[test]
    fn objects_are_written_in_pdf_syntax() {
        let mut out = Vec::new();
        let dict = dictionary! {
            "Type" => "Annot",
            "Name" => Object::Name(b"A B".to_vec()),
            "Rect" => vec![0.into(), Object::Real(1.5), Object::Real(2.0), 4.into()],
            "T" => Object::string_literal("a(b)\\c"),
            "H" => Object::String(vec![0xAB, 0x01], StringFormat::Hexadecimal),
            "R" => Object::Reference((7, 0)),
            "B" => true,
        };
        write_object(&Object::Dictionary(dict), &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("/Name /A#20B"), "{text}");
        assert!(text.contains("/Rect [0 1.5 2 4]"), "{text}");
        assert!(text.contains("/T (a\\(b\\)\\\\c)"), "{text}");
        assert!(text.contains("/H <AB01>"), "{text}");
        assert!(text.contains("/R 7 0 R"), "{text}");
        assert!(text.contains("/B true"), "{text}");
    }

    #[test]
    fn runs_group_consecutive_numbers() {
        assert_eq!(
            runs([3, 4, 5, 9, 10, 20].into_iter()),
            vec![(3, 3), (9, 2), (20, 1)]
        );
        assert!(runs(std::iter::empty()).is_empty());
    }

    #[test]
    fn a_revision_keeps_the_original_bytes_and_adds_objects() {
        let original = sample();
        let mut rev = Revision::open(&original).unwrap();
        let id = rev.add(Object::Dictionary(dictionary! { "Marker" => 42 }));
        let done = rev.finish().unwrap();
        assert_eq!(
            &done.bytes[..original.len()],
            &original[..],
            "earlier bytes never change"
        );
        let reloaded = Document::load_mem(&done.bytes).unwrap();
        let object = reloaded.get_object(id).unwrap().as_dict().unwrap();
        assert_eq!(object.get(b"Marker").unwrap().as_i64().unwrap(), 42);
        assert_eq!(reloaded.get_pages().len(), 2, "the pages are still there");
        assert!(done.bytes.ends_with(b"%%EOF\n"));
    }

    #[test]
    fn a_changed_object_replaces_the_old_one() {
        let original = sample();
        let mut rev = Revision::open(&original).unwrap();
        let root = rev.root().unwrap();
        let mut catalog = rev.dict(&Object::Reference(root)).unwrap().clone();
        catalog.set("Lang", Object::string_literal("en-GB"));
        rev.replace(root, Object::Dictionary(catalog));
        let done = rev.finish().unwrap();
        let reloaded = Document::load_mem(&done.bytes).unwrap();
        let catalog = reloaded.catalog().unwrap();
        assert_eq!(catalog.get(b"Lang").unwrap().as_str().unwrap(), b"en-GB");
        assert!(catalog.has(b"Pages"), "the rest of the catalog is kept");
    }

    #[test]
    fn revisions_stack() {
        let mut bytes = sample();
        for n in 1..=3i64 {
            let mut rev = Revision::open(&bytes).unwrap();
            rev.add(Object::Dictionary(dictionary! { "Round" => n }));
            let before = bytes.len();
            let done = rev.finish().unwrap();
            assert_eq!(&done.bytes[..before], &bytes[..]);
            bytes = done.bytes;
        }
        let doc = Document::load_mem(&bytes).unwrap();
        let rounds = doc
            .objects
            .values()
            .filter(|o| o.as_dict().is_ok_and(|d| d.has(b"Round")))
            .count();
        assert_eq!(rounds, 3);
    }

    #[test]
    fn a_file_that_is_not_a_pdf_is_refused_with_advice() {
        let err = Revision::open(b"this is not a pdf").err().unwrap();
        assert_eq!(err.code, ErrorCode::PdfDamaged);
        assert!(err.message.contains("Repair"), "{}", err.message);
    }
}
