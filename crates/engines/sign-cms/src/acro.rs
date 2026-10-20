//! Reading the form of a PDF: the catalog, the AcroForm, the field tree and the pages.

use lopdf::{Dictionary, Document, Object, ObjectId};

/// Something that holds PDF objects: a document, or a document with a revision in progress.
pub trait Objects {
    /// The object with this id.
    fn object(&self, id: ObjectId) -> Option<&Object>;

    /// The id of the catalog.
    fn root_id(&self) -> Option<ObjectId>;
}

impl Objects for Document {
    fn object(&self, id: ObjectId) -> Option<&Object> {
        self.objects.get(&id)
    }

    fn root_id(&self) -> Option<ObjectId> {
        self.trailer
            .get(b"Root")
            .ok()
            .and_then(|o| o.as_reference().ok())
    }
}

impl Objects for crate::pdfrev::Revision {
    fn object(&self, id: ObjectId) -> Option<&Object> {
        self.get(id)
    }

    fn root_id(&self) -> Option<ObjectId> {
        self.root().ok()
    }
}

/// Follows references until a direct object.
pub fn resolve<'a>(o: &'a dyn Objects, mut object: &'a Object) -> Option<&'a Object> {
    for _ in 0..32 {
        match object {
            Object::Reference(id) => object = o.object(*id)?,
            other => return Some(other),
        }
    }
    None
}

/// The dictionary an object stands for, following references.
pub fn dict<'a>(o: &'a dyn Objects, object: &'a Object) -> Option<&'a Dictionary> {
    match resolve(o, object)? {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

/// The dictionary of the object with this id.
pub fn object_dict(o: &dyn Objects, id: ObjectId) -> Option<&Dictionary> {
    match o.object(id)? {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

/// The array an object stands for, following references.
pub fn array<'a>(o: &'a dyn Objects, object: &'a Object) -> Option<&'a Vec<Object>> {
    match resolve(o, object)? {
        Object::Array(a) => Some(a),
        _ => None,
    }
}

/// The catalog.
pub fn catalog(o: &dyn Objects) -> Option<&Dictionary> {
    let root = o.object(o.root_id()?)?;
    dict(o, root)
}

/// The interactive form dictionary, when there is one.
pub fn acroform(o: &dyn Objects) -> Option<&Dictionary> {
    let value = catalog(o)?.get(b"AcroForm").ok()?;
    dict(o, value)
}

/// A number of an object, whether integer or real.
pub fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(f64::from(*r)),
        _ => None,
    }
}

/// An array of numbers, such as a rectangle.
pub fn numbers(o: &dyn Objects, object: &Object) -> Option<Vec<f64>> {
    array(o, object)?
        .iter()
        .map(|v| resolve(o, v).and_then(number))
        .collect()
}

/// A text string of the PDF as Rust text (PDFDocEncoding approximated as Latin-1, or UTF-16BE
/// when it starts with a byte order mark).
pub fn text(object: &Object) -> Option<String> {
    let Object::String(bytes, _) = object else {
        return None;
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_be_bytes(*p))
            .collect();
        return Some(String::from_utf16_lossy(&units));
    }
    Some(bytes.iter().map(|&b| char::from(b)).collect())
}

/// A field or widget of the form tree.
#[derive(Debug, Clone)]
pub struct Field {
    /// The object, when the field is an indirect object.
    pub id: Option<ObjectId>,
    /// Full name: the partial names of the field and its parents joined by dots.
    pub name: String,
    /// The dictionary of this node.
    pub dict: Dictionary,
    /// Field type, inherited from the parents.
    pub field_type: Option<Vec<u8>>,
    /// Value, inherited from the parents.
    pub value: Option<Object>,
}

fn walk(
    o: &dyn Objects,
    items: &[Object],
    parent_name: &str,
    inherited: (&Option<Vec<u8>>, &Option<Object>),
    depth: usize,
    out: &mut Vec<Field>,
) {
    if depth > 16 || out.len() > 20_000 {
        return;
    }
    for item in items {
        let id = match item {
            Object::Reference(id) => Some(*id),
            _ => None,
        };
        let Some(d) = dict(o, item) else { continue };
        let partial = d.get(b"T").ok().and_then(text);
        let name = match (&partial, parent_name.is_empty()) {
            (Some(p), true) => p.clone(),
            (Some(p), false) => format!("{parent_name}.{p}"),
            (None, _) => parent_name.to_owned(),
        };
        let field_type = d
            .get(b"FT")
            .ok()
            .and_then(|v| resolve(o, v))
            .and_then(|v| v.as_name().ok().map(<[u8]>::to_vec))
            .or_else(|| inherited.0.clone());
        let value = d.get(b"V").ok().cloned().or_else(|| inherited.1.clone());
        out.push(Field {
            id,
            name: name.clone(),
            dict: d.clone(),
            field_type: field_type.clone(),
            value: value.clone(),
        });
        if let Some(kids) = d.get(b"Kids").ok().and_then(|k| array(o, k)) {
            walk(o, kids, &name, (&field_type, &value), depth + 1, out);
        }
    }
}

/// Every node of the field tree, parents before their kids.
pub fn fields(o: &dyn Objects) -> Vec<Field> {
    let mut out = Vec::new();
    if let Some(form) = acroform(o)
        && let Some(roots) = form.get(b"Fields").ok().and_then(|f| array(o, f))
    {
        walk(o, roots, "", (&None, &None), 0, &mut out);
    }
    out
}

impl Field {
    /// True for a signature field.
    pub fn is_signature(&self) -> bool {
        self.field_type.as_deref() == Some(b"Sig".as_slice())
    }

    /// True when the field has a value, which for a signature field means it is signed.
    pub fn is_filled(&self) -> bool {
        !matches!(self.value, None | Some(Object::Null))
    }
}

/// The pages in order, with their ids.
pub fn pages(doc: &Document) -> Vec<ObjectId> {
    doc.get_pages().into_values().collect()
}

/// A page attribute that may be inherited from the parents in the page tree.
pub fn inherited<'a>(o: &'a dyn Objects, page: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    let mut current = page;
    for _ in 0..32 {
        if let Ok(value) = current.get(key) {
            return Some(value);
        }
        let parent = current.get(b"Parent").ok()?;
        current = dict(o, parent)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_is_walked_with_inherited_types_and_full_names() {
        let doc =
            Document::load_mem(&test_support::pdfs::pdf_with_form_field("name", "Ada")).unwrap();
        let all = fields(&doc);
        let named = all.iter().find(|f| f.name == "name").expect("the field");
        assert_eq!(named.field_type.as_deref(), Some(b"Tx".as_slice()));
        assert!(!named.is_signature());
        assert!(named.is_filled());
    }

    #[test]
    fn a_file_without_a_form_has_no_fields() {
        let doc = Document::load_mem(&test_support::pdfs::pdf_with_texts(&["x"])).unwrap();
        assert!(fields(&doc).is_empty());
        assert!(acroform(&doc).is_none());
        assert!(catalog(&doc).is_some());
    }

    #[test]
    fn text_strings_decode_both_encodings() {
        assert_eq!(text(&Object::string_literal("abc")).unwrap(), "abc");
        let utf16 = Object::String(
            vec![0xFE, 0xFF, 0x00, 0x41, 0x00, 0xE9],
            lopdf::StringFormat::Literal,
        );
        assert_eq!(text(&utf16).unwrap(), "A\u{e9}");
        assert!(text(&Object::Integer(1)).is_none());
    }
}
