//! Finding the signatures of a PDF and what changed after each of them.

use std::collections::HashSet;

use lopdf::xref::XrefEntry;
use lopdf::{Dictionary, Document, Object, ObjectId};
use regex::bytes::Regex;

use crate::acro;
use crate::timefmt;

/// What kind of signature a signature field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A signature that approves the document.
    Approval,
    /// A certifying signature with its DocMDP permission.
    Certification(u8),
    /// A document time stamp.
    Timestamp,
}

/// A signature found in the file.
#[derive(Debug, Clone)]
pub struct Found {
    /// Name of the field.
    pub field: String,
    /// Kind.
    pub kind: Kind,
    /// The byte ranges that were signed, as stored.
    pub ranges: [i64; 4],
    /// The signature value (`/Contents`).
    pub contents: Vec<u8>,
    /// Name of the signer, as the signer wrote it.
    pub name: Option<String>,
    /// Reason.
    pub reason: Option<String>,
    /// Place.
    pub location: Option<String>,
    /// Contact.
    pub contact: Option<String>,
    /// The time the signer claims, seconds since 1970.
    pub claimed: Option<i64>,
}

/// Why the byte ranges cannot be trusted, or how far they reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// The ranges are sound; the signature covers the file up to this offset.
    To(usize),
    /// The ranges do not describe the file.
    Broken(String),
}

fn text_entry(d: &Dictionary, key: &[u8]) -> Option<String> {
    d.get(key).ok().and_then(acro::text)
}

fn integer(o: &Document, object: &Object) -> Option<i64> {
    match acro::resolve(o, object)? {
        Object::Integer(i) => Some(*i),
        Object::Real(r) => Some(*r as i64),
        _ => None,
    }
}

/// The signatures of a document in the order they were added.
pub fn find(doc: &Document) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    for field in acro::fields(doc) {
        if !field.is_signature() || !field.is_filled() {
            continue;
        }
        let Some(value) = &field.value else { continue };
        let Some(dict) = acro::dict(doc, value) else {
            continue;
        };
        let Some(range) = dict
            .get(b"ByteRange")
            .ok()
            .and_then(|r| acro::array(doc, r))
        else {
            continue;
        };
        let numbers: Vec<i64> = range.iter().filter_map(|n| integer(doc, n)).collect();
        let Ok(ranges): Result<[i64; 4], _> = numbers.try_into() else {
            continue;
        };
        let Some(Object::String(contents, _)) = dict
            .get(b"Contents")
            .ok()
            .and_then(|c| acro::resolve(doc, c))
        else {
            continue;
        };
        let subfilter = dict
            .get(b"SubFilter")
            .ok()
            .and_then(|s| s.as_name().ok())
            .map(<[u8]>::to_vec);
        let is_timestamp = dict.get(b"Type").ok().and_then(|t| t.as_name().ok())
            == Some(b"DocTimeStamp".as_slice())
            || subfilter.as_deref() == Some(b"ETSI.RFC3161".as_slice());
        let permission = certification(doc, dict);
        let kind = if is_timestamp {
            Kind::Timestamp
        } else if let Some(p) = permission {
            Kind::Certification(u8::try_from(p.clamp(1, 3)).unwrap_or(2))
        } else {
            Kind::Approval
        };
        found.push(Found {
            field: field.name.clone(),
            kind,
            ranges,
            contents: contents.clone(),
            name: text_entry(dict, b"Name"),
            reason: text_entry(dict, b"Reason"),
            location: text_entry(dict, b"Location"),
            contact: text_entry(dict, b"ContactInfo"),
            claimed: text_entry(dict, b"M").and_then(|m| timefmt::parse_pdf_date(&m)),
        });
    }
    found.sort_by_key(|f| f.ranges[2].saturating_add(f.ranges[3]));
    found
}

/// The DocMDP permission of a signature dictionary, when it certifies the document.
fn certification(doc: &Document, dict: &Dictionary) -> Option<i64> {
    let refs = acro::array(doc, dict.get(b"Reference").ok()?)?;
    for r in refs {
        let r = acro::dict(doc, r)?;
        if r.get(b"TransformMethod")
            .ok()
            .and_then(|m| m.as_name().ok())
            == Some(b"DocMDP".as_slice())
        {
            let params = r
                .get(b"TransformParams")
                .ok()
                .and_then(|p| acro::dict(doc, p));
            let p = params
                .and_then(|p| p.get(b"P").ok())
                .and_then(|p| integer(doc, p))
                .unwrap_or(2);
            return Some(p);
        }
    }
    None
}

fn hex_value(text: &[u8]) -> Option<Vec<u8>> {
    let digits: Vec<u8> = text
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    digits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| {
            let hi = char::from(p[0]).to_digit(16)?;
            let lo = char::from(p[1]).to_digit(16)?;
            u8::try_from(hi * 16 + lo).ok()
        })
        .collect()
}

impl Found {
    /// Checks that the ranges describe the file: they start at the beginning, leave out exactly
    /// the space of the signature value, and stay inside the file.
    pub fn reach(&self, bytes: &[u8]) -> Reach {
        let [a, b, c, d] = self.ranges;
        let len = bytes.len() as i64;
        if a != 0 {
            return Reach::Broken(
                "the signed part does not start at the beginning of the file".to_owned(),
            );
        }
        if b < 0 || c < b || d < 0 || c + d > len {
            return Reach::Broken("the signed ranges point outside the file".to_owned());
        }
        let (gap_start, gap_end) = (b as usize, c as usize);
        let gap = &bytes[gap_start..gap_end];
        let inside = gap
            .strip_prefix(b"<")
            .and_then(|g| g.strip_suffix(b">"))
            .and_then(hex_value);
        if inside.as_deref() != Some(self.contents.as_slice()) {
            return Reach::Broken(
                "the part left out of the signature is more than the signature itself".to_owned(),
            );
        }
        Reach::To((c + d) as usize)
    }

    /// The bytes the signature covers.
    pub fn signed<'a>(&self, bytes: &'a [u8]) -> (&'a [u8], &'a [u8]) {
        let [_, b, c, d] = self.ranges;
        (&bytes[..b as usize], &bytes[c as usize..(c + d) as usize])
    }
}

/// What changed in the part of the file after a signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tail {
    /// Objects that were added or replaced.
    pub objects: usize,
    /// Signatures, time stamps and validation data.
    pub validation: usize,
    /// Form fields and comments.
    pub forms: usize,
    /// The catalog, the form, the pages and lists that point to what was added.
    pub bookkeeping: usize,
    /// Anything else: the content of pages, fonts, pictures, the page tree.
    pub content: usize,
}

/// Looks at the objects written after `end` and sorts them by what they are.
pub fn tail(doc: &Document, bytes: &[u8], end: usize) -> Tail {
    let mut out = Tail::default();
    let Some(part) = bytes.get(end..) else {
        return out;
    };
    if part.iter().all(u8::is_ascii_whitespace) {
        return out;
    }
    let Ok(header) = Regex::new(r"(?:^|[\r\n\s])(\d{1,10})\s+(\d{1,5})\s+obj\b") else {
        return out;
    };
    let mut ids: HashSet<u32> = header
        .captures_iter(part)
        .filter_map(|c| std::str::from_utf8(&c[1]).ok()?.parse().ok())
        .collect();
    // Objects kept inside object streams written after the signature.
    let containers: HashSet<u32> = ids.clone();
    for (id, entry) in &doc.reference_table.entries {
        if let XrefEntry::Compressed { container, .. } = entry
            && containers.contains(container)
        {
            ids.insert(*id);
        }
    }
    let mut validation_streams = stored_streams(doc);
    let (annotation_streams, signature_streams) = appearance_streams(doc);
    validation_streams.extend(signature_streams);
    let root = doc
        .trailer
        .get(b"Root")
        .ok()
        .and_then(|r| r.as_reference().ok());
    let form = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"AcroForm").ok())
        .and_then(|f| f.as_reference().ok());
    let info = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|i| i.as_reference().ok());
    for id in ids {
        let key: ObjectId = doc
            .objects
            .keys()
            .find(|k| k.0 == id)
            .copied()
            .unwrap_or((id, 0));
        let Some(object) = doc.objects.get(&key) else {
            continue;
        };
        let dict = match object {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        };
        let name = |d: &Dictionary, k: &[u8]| {
            d.get(k)
                .ok()
                .and_then(|v| v.as_name().ok())
                .map(<[u8]>::to_vec)
        };
        let ty = dict.and_then(|d| name(d, b"Type"));
        if ty.as_deref() == Some(b"XRef".as_slice()) || ty.as_deref() == Some(b"ObjStm".as_slice())
        {
            continue;
        }
        out.objects += 1;
        let ft = dict.and_then(|d| name(d, b"FT"));
        let subtype = dict.and_then(|d| name(d, b"Subtype"));
        let is_validation = matches!(
            ty.as_deref(),
            Some(b"Sig" | b"DocTimeStamp" | b"DSS" | b"VRI")
        ) || ft.as_deref() == Some(b"Sig".as_slice())
            || validation_streams.contains(&key);
        // The properties of the document and its XMP packet describe the file; they are not
        // what the signature shows. Programs rewrite them when they add a signature.
        let is_bookkeeping = Some(key) == root
            || Some(key) == form
            || Some(key) == info
            || ty.as_deref() == Some(b"Metadata".as_slice())
            || matches!(ty.as_deref(), Some(b"Page" | b"Pages"))
            || matches!(object, Object::Array(_));
        let is_form = matches!(ty.as_deref(), Some(b"Annot"))
            || subtype.as_deref() == Some(b"Widget".as_slice())
            || ft.is_some()
            || annotation_streams.contains(&key);
        if is_validation {
            out.validation += 1;
        } else if is_bookkeeping {
            out.bookkeeping += 1;
        } else if is_form {
            out.forms += 1;
        } else {
            out.content += 1;
        }
    }
    out
}

/// The streams the document security store points to.
fn stored_streams(doc: &Document) -> HashSet<ObjectId> {
    let mut set = HashSet::new();
    let Some(dss) = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"DSS").ok())
        .and_then(|d| acro::dict(doc, d))
    else {
        return set;
    };
    for key in [b"Certs".as_slice(), b"OCSPs", b"CRLs"] {
        if let Some(list) = dss.get(key).ok().and_then(|a| acro::array(doc, a)) {
            set.extend(list.iter().filter_map(|i| i.as_reference().ok()));
        }
    }
    set
}

/// The streams that annotations use to draw themselves: those of comments and form fields, and
/// those of signature fields.
fn appearance_streams(doc: &Document) -> (HashSet<ObjectId>, HashSet<ObjectId>) {
    let (mut others, mut signatures) = (HashSet::new(), HashSet::new());
    for object in doc.objects.values() {
        let Object::Dictionary(d) = object else {
            continue;
        };
        let Some(ap) = d.get(b"AP").ok().and_then(|a| acro::dict(doc, a)) else {
            continue;
        };
        let is_signature =
            d.get(b"FT").ok().and_then(|t| t.as_name().ok()) == Some(b"Sig".as_slice());
        let set = if is_signature {
            &mut signatures
        } else {
            &mut others
        };
        for (_, state) in ap.iter() {
            match state {
                Object::Reference(id) => {
                    set.insert(*id);
                }
                Object::Dictionary(states) => {
                    set.extend(states.iter().filter_map(|(_, v)| v.as_reference().ok()));
                }
                _ => {}
            }
        }
    }
    (others, signatures)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdfrev::Revision;
    use crate::pdfsign::{SigPlan, prepare};
    use lopdf::dictionary;

    fn signed(bytes: &[u8]) -> Vec<u8> {
        let plan = SigPlan {
            signed_at: 1_800_000_000,
            reserve: 2000,
            reason: Some("Approval".into()),
            name: Some("Ada".into()),
            ..SigPlan::default()
        };
        prepare(bytes, &plan).unwrap().embed(&[1, 2, 3, 4]).unwrap()
    }

    #[test]
    fn a_signature_is_found_with_its_details() {
        let bytes = signed(&test_support::pdfs::pdf_with_texts(&["x"]));
        let doc = Document::load_mem(&bytes).unwrap();
        let found = find(&doc);
        assert_eq!(found.len(), 1);
        let f = &found[0];
        assert_eq!(f.field, "Signature1");
        assert_eq!(f.kind, Kind::Approval);
        assert_eq!(f.reason.as_deref(), Some("Approval"));
        assert_eq!(f.name.as_deref(), Some("Ada"));
        assert_eq!(f.claimed, Some(1_800_000_000));
        assert_eq!(&f.contents[..4], &[1, 2, 3, 4]);
        assert_eq!(f.reach(&bytes), Reach::To(bytes.len()));
    }

    #[test]
    fn signatures_are_listed_in_the_order_they_were_added() {
        let first = signed(&test_support::pdfs::pdf_with_texts(&["x"]));
        let second = signed(&first);
        let doc = Document::load_mem(&second).unwrap();
        let names: Vec<String> = find(&doc).into_iter().map(|f| f.field).collect();
        assert_eq!(names, ["Signature1", "Signature2"]);
    }

    #[test]
    fn ranges_that_do_not_describe_the_file_are_broken() {
        let bytes = signed(&test_support::pdfs::pdf_with_texts(&["x"]));
        let doc = Document::load_mem(&bytes).unwrap();
        let mut f = find(&doc).remove(0);
        f.ranges[2] += 10;
        assert!(matches!(f.reach(&bytes), Reach::Broken(_)));
        let mut g = find(&doc).remove(0);
        g.ranges[0] = 5;
        assert!(matches!(g.reach(&bytes), Reach::Broken(_)));
        let mut h = find(&doc).remove(0);
        h.ranges[3] += 1_000_000;
        assert!(matches!(h.reach(&bytes), Reach::Broken(_)));
        let mut wrong = find(&doc).remove(0);
        wrong.contents[0] ^= 1;
        assert!(
            matches!(wrong.reach(&bytes), Reach::Broken(_)),
            "the gap must be the value"
        );
    }

    #[test]
    fn nothing_after_the_signature_means_no_changes() {
        let bytes = signed(&test_support::pdfs::pdf_with_texts(&["x"]));
        let doc = Document::load_mem(&bytes).unwrap();
        assert_eq!(tail(&doc, &bytes, bytes.len()), Tail::default());
    }

    #[test]
    fn a_second_signature_is_only_validation_and_form_bookkeeping() {
        let first = signed(&test_support::pdfs::pdf_with_texts(&["x"]));
        let second = signed(&first);
        let doc = Document::load_mem(&second).unwrap();
        let t = tail(&doc, &second, first.len());
        assert!(t.validation >= 2, "{t:?}");
        assert_eq!((t.content, t.forms), (0, 0), "{t:?}");
        assert!(
            t.bookkeeping >= 1,
            "the catalog or the page was updated: {t:?}"
        );
    }

    #[test]
    fn rewritten_document_properties_are_not_content() {
        let first = signed(&test_support::pdfs::pdf_with_metadata("Title", "Author"));
        let mut rev = Revision::open(&first).unwrap();
        // What other programs write along with a signature: an XMP packet for the catalog.
        let xmp = rev.add(Object::Stream(lopdf::Stream::new(
            dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
            b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"></x:xmpmeta>".to_vec(),
        )));
        let root = rev.root().unwrap();
        let mut catalog = rev.dict(&Object::Reference(root)).unwrap().clone();
        catalog.set("Metadata", Object::Reference(xmp));
        rev.replace(root, Object::Dictionary(catalog));
        let edited = rev.finish().unwrap().bytes;
        let doc = Document::load_mem(&edited).unwrap();
        let t = tail(&doc, &edited, first.len());
        assert_eq!((t.content, t.forms), (0, 0), "{t:?}");
        assert!(t.bookkeeping >= 2, "{t:?}");
    }

    #[test]
    fn a_changed_page_is_content() {
        let first = signed(&test_support::pdfs::pdf_with_texts(&["x"]));
        let mut rev = Revision::open(&first).unwrap();
        // A new content stream, as an edit that adds text would write.
        rev.add(Object::Stream(lopdf::Stream::new(
            dictionary! {},
            b"BT /F1 12 Tf 10 10 Td (hello) Tj ET".to_vec(),
        )));
        let edited = rev.finish().unwrap().bytes;
        let doc = Document::load_mem(&edited).unwrap();
        let t = tail(&doc, &edited, first.len());
        assert_eq!(t.content, 1, "{t:?}");
        assert_eq!(t.validation, 0);
    }

    #[test]
    fn a_file_certified_with_a_permission_reports_it() {
        let plan = SigPlan {
            signed_at: 1_800_000_000,
            reserve: 2000,
            certify: Some(2),
            ..SigPlan::default()
        };
        let bytes = prepare(&test_support::pdfs::pdf_with_texts(&["x"]), &plan)
            .unwrap()
            .embed(&[1])
            .unwrap();
        let doc = Document::load_mem(&bytes).unwrap();
        assert_eq!(find(&doc)[0].kind, Kind::Certification(2));
    }
}
