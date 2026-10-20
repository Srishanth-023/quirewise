//! The document security store (DSS): certificates and revocation answers kept in the file, so a
//! signature can be checked years later without asking anyone (PAdES-B-LT).

use application::AppError;
use lopdf::{Dictionary, Document, Object, Stream, dictionary};

use crate::acro::{self, Objects};
use crate::certs::Cert;
use crate::pdfrev::Revision;
use crate::revocation::Evidence;
use crate::timefmt;

/// What the store of a document holds.
#[derive(Debug, Clone, Default)]
pub struct Stored {
    /// Certificates.
    pub certs: Vec<Cert>,
    /// Revocation answers.
    pub evidence: Evidence,
}

fn stream_bytes(o: &dyn Objects, item: &Object) -> Option<Vec<u8>> {
    match acro::resolve(o, item)? {
        Object::Stream(s) => Some(
            s.decompressed_content()
                .unwrap_or_else(|_| s.content.clone()),
        ),
        _ => None,
    }
}

fn items(o: &dyn Objects, dss: &Dictionary, key: &[u8]) -> Vec<Vec<u8>> {
    dss.get(key)
        .ok()
        .and_then(|a| acro::array(o, a))
        .map(|a| a.iter().filter_map(|i| stream_bytes(o, i)).collect())
        .unwrap_or_default()
}

/// Reads the store of a document. A document without one gives an empty result.
pub fn read(doc: &Document) -> Stored {
    let Some(dss) = acro::catalog(doc)
        .and_then(|c| c.get(b"DSS").ok())
        .and_then(|d| acro::dict(doc, d))
    else {
        return Stored::default();
    };
    Stored {
        certs: items(doc, dss, b"Certs")
            .iter()
            .filter_map(|d| Cert::parse(d).ok())
            .collect(),
        evidence: Evidence {
            ocsp: items(doc, dss, b"OCSPs"),
            crls: items(doc, dss, b"CRLs"),
        },
    }
}

fn references(list: &[lopdf::ObjectId]) -> Vec<Object> {
    list.iter().copied().map(Object::Reference).collect()
}

/// Appends a revision that stores the certificates and answers for one signature, keeping
/// whatever the document stored before.
///
/// `signature_value` is the `/Contents` of the signature, which names the entry of the store.
pub fn append(
    input: &[u8],
    signature_value: &[u8],
    certs: &[Cert],
    evidence: &Evidence,
    at: i64,
) -> Result<Vec<u8>, AppError> {
    let mut rev = Revision::open(input)?;
    let root = rev.root()?;
    let mut catalog = acro::catalog(&rev)
        .cloned()
        .ok_or_else(|| AppError::internal("reading the document catalog"))?;

    // What the store already holds, as references, so that it stays.
    let existing_id = catalog.get(b"DSS").ok().and_then(|d| d.as_reference().ok());
    let existing = catalog
        .get(b"DSS")
        .ok()
        .and_then(|d| acro::dict(&rev, d))
        .cloned()
        .unwrap_or_default();
    let old = |key: &[u8]| -> Vec<Object> {
        existing
            .get(key)
            .ok()
            .and_then(|a| acro::array(&rev, a))
            .cloned()
            .unwrap_or_default()
    };
    let (mut cert_refs, mut ocsp_refs, mut crl_refs) = (old(b"Certs"), old(b"OCSPs"), old(b"CRLs"));

    let add = |rev: &mut Revision, data: &[u8]| {
        rev.add(Object::Stream(Stream::new(
            Dictionary::new(),
            data.to_vec(),
        )))
    };
    let mut mine = (Vec::new(), Vec::new(), Vec::new());
    for cert in certs {
        mine.0.push(add(&mut rev, &cert.der));
    }
    for ocsp in &evidence.ocsp {
        mine.1.push(add(&mut rev, ocsp));
    }
    for crl in &evidence.crls {
        mine.2.push(add(&mut rev, crl));
    }
    cert_refs.extend(references(&mine.0));
    ocsp_refs.extend(references(&mine.1));
    crl_refs.extend(references(&mine.2));

    let key: String = crate::algo::Hash::Sha1
        .digest(signature_value)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect();
    let mut vri = existing
        .get(b"VRI")
        .ok()
        .and_then(|v| acro::dict(&rev, v))
        .cloned()
        .unwrap_or_default();
    let mut entry = dictionary! {
        "Type" => "VRI",
        "TU" => Object::string_literal(timefmt::pdf_date(at)),
    };
    if !mine.0.is_empty() {
        entry.set("Cert", Object::Array(references(&mine.0)));
    }
    if !mine.1.is_empty() {
        entry.set("OCSP", Object::Array(references(&mine.1)));
    }
    if !mine.2.is_empty() {
        entry.set("CRL", Object::Array(references(&mine.2)));
    }
    vri.set(key, Object::Dictionary(entry));

    let mut dss = dictionary! { "Type" => "DSS" };
    if !cert_refs.is_empty() {
        dss.set("Certs", Object::Array(cert_refs));
    }
    if !ocsp_refs.is_empty() {
        dss.set("OCSPs", Object::Array(ocsp_refs));
    }
    if !crl_refs.is_empty() {
        dss.set("CRLs", Object::Array(crl_refs));
    }
    dss.set("VRI", Object::Dictionary(vri));
    let dss_id = match existing_id {
        Some(id) => {
            rev.replace(id, Object::Dictionary(dss));
            id
        }
        None => rev.add(Object::Dictionary(dss)),
    };
    catalog.set("DSS", Object::Reference(dss_id));
    // The extension that tells older readers about the store.
    if !catalog.has(b"Extensions") {
        catalog.set(
            "Extensions",
            Object::Dictionary(dictionary! {
                "ADBE" => dictionary! {
                    "BaseVersion" => "1.7",
                    "ExtensionLevel" => 5,
                },
            }),
        );
    }
    rev.replace(root, Object::Dictionary(catalog));
    Ok(rev.finish()?.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::Pki;

    fn sample() -> Vec<u8> {
        test_support::pdfs::pdf_with_texts(&["x"])
    }

    #[test]
    fn a_document_without_a_store_reads_as_empty() {
        let doc = Document::load_mem(&sample()).unwrap();
        let stored = read(&doc);
        assert!(stored.certs.is_empty() && stored.evidence.is_empty());
    }

    #[test]
    fn certificates_and_answers_are_stored_and_read_back() {
        let pki = Pki::ec_with_addresses();
        let leaf = Cert::parse(&pki.leaf_der).unwrap();
        let root = Cert::parse(&pki.root_der).unwrap();
        let evidence = Evidence {
            ocsp: vec![vec![1, 2, 3]],
            crls: vec![vec![4, 5]],
        };
        let original = sample();
        let out = append(
            &original,
            b"signature bytes",
            &[leaf.clone(), root.clone()],
            &evidence,
            1_800_000_000,
        )
        .unwrap();
        assert_eq!(
            &out[..original.len()],
            &original[..],
            "earlier bytes are untouched"
        );
        let stored = read(&Document::load_mem(&out).unwrap());
        assert_eq!(stored.certs, vec![leaf, root]);
        assert_eq!(stored.evidence, evidence);
    }

    #[test]
    fn a_second_signature_adds_to_the_store() {
        let pki = Pki::ec();
        let leaf = Cert::parse(&pki.leaf_der).unwrap();
        let root = Cert::parse(&pki.root_der).unwrap();
        let first = append(
            &sample(),
            b"one",
            std::slice::from_ref(&leaf),
            &Evidence::default(),
            1_800_000_000,
        )
        .unwrap();
        let second = append(
            &first,
            b"two",
            std::slice::from_ref(&root),
            &Evidence {
                ocsp: vec![vec![9]],
                crls: vec![],
            },
            1_800_000_100,
        )
        .unwrap();
        let doc = Document::load_mem(&second).unwrap();
        let stored = read(&doc);
        assert_eq!(stored.certs, vec![leaf, root]);
        assert_eq!(stored.evidence.ocsp, vec![vec![9]]);
        let dss = acro::dict(&doc, acro::catalog(&doc).unwrap().get(b"DSS").unwrap()).unwrap();
        let vri = acro::dict(&doc, dss.get(b"VRI").unwrap()).unwrap();
        assert_eq!(vri.len(), 2, "one entry for each signature");
    }
}
