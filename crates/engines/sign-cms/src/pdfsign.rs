//! Adding a signature field to a PDF as a new revision, with room reserved for the signature.
//!
//! The revision holds the signature dictionary, a widget that shows it (or an invisible one),
//! and the changed page, form and catalog. The signature itself is put into the reserved space
//! afterwards, once the digest of everything around it is known.

use application::AppError;
use domain::ErrorCode;
use lopdf::{Dictionary, Object, ObjectId, Stream, StringFormat, dictionary};

use crate::acro::{self, Field, Objects};
use crate::algo::Hash;
use crate::pdfrev::Revision;
use crate::timefmt;

/// Where a visible signature goes: shares of the displayed page from its top left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Page, counted from 0.
    pub page: u32,
    /// Left edge.
    pub left: f32,
    /// Top edge.
    pub top: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

/// What to put in the new revision.
#[derive(Debug, Clone, Default)]
pub struct SigPlan {
    /// An empty signature field to fill, instead of making one.
    pub field: Option<String>,
    /// Where to draw the signature, `None` for an invisible one.
    pub visible: Option<Placement>,
    /// The lines of text of a visible signature.
    pub lines: Vec<String>,
    /// Name of the signer.
    pub name: Option<String>,
    /// Reason.
    pub reason: Option<String>,
    /// Place.
    pub location: Option<String>,
    /// Contact.
    pub contact: Option<String>,
    /// Time of signing, seconds since 1970 UTC.
    pub signed_at: i64,
    /// DocMDP permission (1 to 3) for a certifying signature.
    pub certify: Option<u8>,
    /// Make a document time stamp instead of a signature.
    pub document_timestamp: bool,
    /// Bytes reserved for the CMS structure.
    pub reserve: usize,
}

/// A file with an empty signature in it, ready for the digest and the signature.
pub struct Prepared {
    /// The whole file, with zeros where the signature goes.
    pub bytes: Vec<u8>,
    /// Name of the field.
    pub field_name: String,
    byte_range_at: usize,
    contents_open: usize,
    contents_close: usize,
}

const RANGE_PLACEHOLDER: &str = "[0 9999999999 9999999999 9999999999]";

impl Prepared {
    /// The digest of the file without the space of the signature.
    pub fn digest(&self, hash: Hash) -> Vec<u8> {
        let mut h = hash.hasher();
        h.update(&self.bytes[..self.contents_open]);
        h.update(&self.bytes[self.contents_close + 1..]);
        h.finish()
    }

    /// The byte ranges that the signature covers.
    pub fn ranges(&self) -> [usize; 4] {
        let after = self.contents_close + 1;
        [0, self.contents_open, after, self.bytes.len() - after]
    }

    /// Room for the signature, in bytes.
    pub fn capacity(&self) -> usize {
        (self.contents_close - self.contents_open - 1) / 2
    }

    /// Writes the byte ranges over their placeholder. The text has the same length, so nothing
    /// moves.
    fn write_ranges(&mut self) {
        let r = self.ranges();
        let text = format!("[0 {:<10} {:<10} {:<10}]", r[1], r[2], r[3]);
        debug_assert_eq!(text.len(), RANGE_PLACEHOLDER.len());
        self.bytes[self.byte_range_at..self.byte_range_at + text.len()]
            .copy_from_slice(text.as_bytes());
    }

    /// Writes the signature into the reserved space.
    pub fn embed(mut self, signature: &[u8]) -> Result<Vec<u8>, String> {
        if signature.len() > self.capacity() {
            return Err(format!(
                "the signature takes {} bytes but {} were reserved",
                signature.len(),
                self.capacity()
            ));
        }
        let mut at = self.contents_open + 1;
        for b in signature {
            let pair = format!("{b:02X}");
            self.bytes[at..at + 2].copy_from_slice(pair.as_bytes());
            at += 2;
        }
        Ok(self.bytes)
    }
}

fn refuse(message: &str) -> AppError {
    AppError::new(ErrorCode::OptionInvalid, message)
}

/// A text string for a PDF dictionary: plain when ASCII, UTF-16 with a byte order mark otherwise.
pub fn pdf_text(text: &str) -> Object {
    if text.is_ascii() {
        return Object::String(text.as_bytes().to_vec(), StringFormat::Literal);
    }
    let mut bytes = vec![0xFE, 0xFF];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    Object::String(bytes, StringFormat::Literal)
}

/// The page box that is shown: the crop box inside the media box, and the rotation.
struct PageBox {
    x0: f64,
    y0: f64,
    w: f64,
    h: f64,
    rotate: i64,
}

fn page_box(o: &dyn Objects, page: &Dictionary) -> PageBox {
    let boxed = |key: &[u8]| {
        acro::inherited(o, page, key)
            .and_then(|v| acro::numbers(o, v))
            .filter(|b| b.len() == 4)
    };
    let media = boxed(b"MediaBox").unwrap_or_else(|| vec![0.0, 0.0, 612.0, 792.0]);
    let (mx0, mx1) = (media[0].min(media[2]), media[0].max(media[2]));
    let (my0, my1) = (media[1].min(media[3]), media[1].max(media[3]));
    let (mut x0, mut y0, mut x1, mut y1) = (mx0, my0, mx1, my1);
    if let Some(crop) = boxed(b"CropBox") {
        x0 = x0.max(crop[0].min(crop[2]));
        y0 = y0.max(crop[1].min(crop[3]));
        x1 = x1.min(crop[0].max(crop[2]));
        y1 = y1.min(crop[1].max(crop[3]));
    }
    if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
        (x0, y0, x1, y1) = (mx0, my0, mx1, my1);
    }
    let rotate = acro::inherited(o, page, b"Rotate")
        .and_then(|v| acro::resolve(o, v))
        .and_then(acro::number)
        .map_or(0, |r| (r as i64).rem_euclid(360));
    PageBox {
        x0,
        y0,
        w: x1 - x0,
        h: y1 - y0,
        rotate: rotate / 90 * 90,
    }
}

impl PageBox {
    /// Maps a point of the displayed page, measured from its top left, into user space.
    fn to_user(&self, u: f64, v: f64) -> (f64, f64) {
        match self.rotate {
            90 => (self.x0 + v, self.y0 + u),
            180 => (self.x0 + self.w - u, self.y0 + v),
            270 => (self.x0 + self.w - v, self.y0 + self.h - u),
            _ => (self.x0 + u, self.y0 + self.h - v),
        }
    }

    fn displayed(&self) -> (f64, f64) {
        if self.rotate == 90 || self.rotate == 270 {
            (self.h, self.w)
        } else {
            (self.w, self.h)
        }
    }

    /// The rectangle in user space of a box given as shares of the displayed page.
    fn rect(&self, p: &Placement) -> [f64; 4] {
        let (dw, dh) = self.displayed();
        let (u0, v0) = (f64::from(p.left) * dw, f64::from(p.top) * dh);
        let (u1, v1) = (u0 + f64::from(p.width) * dw, v0 + f64::from(p.height) * dh);
        let (ax, ay) = self.to_user(u0, v0);
        let (bx, by) = self.to_user(u1, v1);
        [ax.min(bx), ay.min(by), ax.max(bx), ay.max(by)]
    }

    /// The matrix that turns an upright appearance into the rotated page.
    fn matrix(&self) -> [i64; 4] {
        match self.rotate {
            90 => [0, 1, -1, 0],
            180 => [-1, 0, 0, -1],
            270 => [0, -1, 1, 0],
            _ => [1, 0, 0, 1],
        }
    }
}

fn win_ansi(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if (' '..='~').contains(&c) => out.push(c),
            c if ('\u{a0}'..='\u{ff}').contains(&c) => out.push_str(&format!("\\{:03o}", c as u32)),
            _ => out.push('?'),
        }
    }
    out
}

fn fit(text: &str, size: f64, width: f64) -> String {
    let max = (width / (0.52 * size)).floor().max(1.0) as usize;
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let keep = max.saturating_sub(3).max(1);
    format!("{}...", text.chars().take(keep).collect::<String>())
}

/// The appearance of a visible signature: a thin frame and the lines of text.
fn appearance(width: f64, height: f64, lines: &[String], matrix: [i64; 4]) -> Object {
    let count = lines.len().max(1) as f64;
    let size = ((height - 6.0) / (count * 1.25)).clamp(5.0, 11.0);
    let leading = size * 1.25;
    let mut content = format!(
        "q 0.5 G 0.75 w 0.4 0.4 {:.2} {:.2} re S\n",
        width - 0.8,
        height - 0.8
    );
    content.push_str(&format!(
        "BT 0.1 g {leading:.2} TL 4 {:.2} Td\n",
        height - 4.0 - size
    ));
    for (i, line) in lines.iter().enumerate() {
        let font = if i == 0 { "F2" } else { "F1" };
        let shown = fit(line, size, width - 8.0);
        if i > 0 {
            content.push_str("T*\n");
        }
        content.push_str(&format!("/{font} {size:.2} Tf ({}) Tj\n", win_ansi(&shown)));
    }
    content.push_str("ET Q\n");
    let font = |base: &str| {
        Object::Dictionary(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => base,
            "Encoding" => "WinAnsiEncoding",
        })
    };
    let dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => vec![0.into(), 0.into(), Object::Real(width as f32), Object::Real(height as f32)],
        "Matrix" => vec![matrix[0].into(), matrix[1].into(), matrix[2].into(), matrix[3].into(), 0.into(), 0.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font("Helvetica"), "F2" => font("Helvetica-Bold") },
        },
    };
    Object::Stream(Stream::new(dict, content.into_bytes()))
}

/// The default lines of a visible signature.
pub fn default_lines(
    name: &str,
    signed_at: i64,
    reason: Option<&str>,
    location: Option<&str>,
    contact: Option<&str>,
) -> Vec<String> {
    let mut lines = vec![
        format!("Digitally signed by {name}"),
        format!("Date: {}", timefmt::signature_date(signed_at)),
    ];
    for (label, value) in [
        ("Reason", reason),
        ("Location", location),
        ("Contact", contact),
    ] {
        if let Some(v) = value {
            lines.push(format!("{label}: {v}"));
        }
    }
    lines
}

/// The certification level (DocMDP permission) the file already carries, when it is certified.
pub fn existing_certification(o: &dyn Objects) -> Option<i64> {
    let perms = acro::catalog(o)?.get(b"Perms").ok()?;
    let perms = acro::dict(o, perms)?;
    let sig = acro::dict(o, perms.get(b"DocMDP").ok()?)?;
    let references = acro::array(o, sig.get(b"Reference").ok()?)?;
    for reference in references {
        let r = acro::dict(o, reference)?;
        let is_docmdp = r
            .get(b"TransformMethod")
            .ok()
            .and_then(|m| m.as_name().ok())
            == Some(b"DocMDP".as_slice());
        if is_docmdp {
            let params = acro::dict(o, r.get(b"TransformParams").ok()?)?;
            return Some(
                params
                    .get(b"P")
                    .ok()
                    .and_then(|p| p.as_i64().ok())
                    .unwrap_or(2),
            );
        }
    }
    None
}

/// True when the file already holds a signature.
fn has_signature(o: &dyn Objects) -> bool {
    acro::fields(o)
        .iter()
        .any(|f| f.is_signature() && f.is_filled())
}

fn unique_name(fields: &[Field], base: &str) -> String {
    (1..)
        .map(|n| format!("{base}{n}"))
        .find(|candidate| !fields.iter().any(|f| &f.name == candidate))
        .unwrap_or_else(|| base.to_owned())
}

/// Adds `item` to the array stored under `key` in the dictionary of object `owner`, whether the
/// array is a direct value or an object of its own.
fn push_into(rev: &mut Revision, owner: ObjectId, key: &str, item: Object) -> Result<(), AppError> {
    let mut dict = match rev.get(owner) {
        Some(Object::Dictionary(d)) => d.clone(),
        _ => {
            return Err(refuse(
                "The PDF has an unexpected structure and cannot be signed.",
            ));
        }
    };
    match dict.get(key.as_bytes()).ok().cloned() {
        Some(Object::Reference(array_id)) => {
            let mut items = match rev.get(array_id) {
                Some(Object::Array(a)) => a.clone(),
                _ => Vec::new(),
            };
            items.push(item);
            rev.replace(array_id, Object::Array(items));
        }
        Some(Object::Array(mut items)) => {
            items.push(item);
            dict.set(key, Object::Array(items));
            rev.replace(owner, Object::Dictionary(dict));
        }
        _ => {
            dict.set(key, Object::Array(vec![item]));
            rev.replace(owner, Object::Dictionary(dict));
        }
    }
    Ok(())
}

/// Registers the field in the form of the document, which is made when it is missing.
fn register_field(rev: &mut Revision, field: ObjectId) -> Result<(), AppError> {
    let root = rev.root()?;
    let mut catalog = match rev.get(root) {
        Some(Object::Dictionary(d)) => d.clone(),
        _ => return Err(refuse("The PDF has no catalog and cannot be signed.")),
    };
    match catalog.get(b"AcroForm").ok().cloned() {
        Some(Object::Reference(form_id)) => {
            let mut form = match rev.get(form_id) {
                Some(Object::Dictionary(d)) => d.clone(),
                _ => Dictionary::new(),
            };
            let flags = form
                .get(b"SigFlags")
                .ok()
                .and_then(|f| f.as_i64().ok())
                .unwrap_or(0)
                | 3;
            form.set("SigFlags", flags);
            rev.replace(form_id, Object::Dictionary(form));
            push_into(rev, form_id, "Fields", Object::Reference(field))
        }
        Some(Object::Dictionary(mut form)) => {
            let flags = form
                .get(b"SigFlags")
                .ok()
                .and_then(|f| f.as_i64().ok())
                .unwrap_or(0)
                | 3;
            form.set("SigFlags", flags);
            let mut fields = match form.get(b"Fields").ok().cloned() {
                Some(Object::Array(a)) => a,
                Some(Object::Reference(id)) => {
                    // The list is its own object: change that one and leave the catalog entry.
                    let mut items = match rev.get(id) {
                        Some(Object::Array(a)) => a.clone(),
                        _ => Vec::new(),
                    };
                    items.push(Object::Reference(field));
                    rev.replace(id, Object::Array(items));
                    catalog.set("AcroForm", Object::Dictionary(form));
                    rev.replace(root, Object::Dictionary(catalog));
                    return Ok(());
                }
                _ => Vec::new(),
            };
            fields.push(Object::Reference(field));
            form.set("Fields", Object::Array(fields));
            catalog.set("AcroForm", Object::Dictionary(form));
            rev.replace(root, Object::Dictionary(catalog));
            Ok(())
        }
        _ => {
            catalog.set(
                "AcroForm",
                Object::Dictionary(dictionary! {
                    "Fields" => vec![Object::Reference(field)],
                    "SigFlags" => 3,
                }),
            );
            rev.replace(root, Object::Dictionary(catalog));
            Ok(())
        }
    }
}

fn text_entry(dict: &mut Dictionary, key: &str, value: &Option<String>) {
    if let Some(v) = value {
        dict.set(key, pdf_text(v));
    }
}

fn signature_dictionary(plan: &SigPlan) -> Dictionary {
    let mut dict = Dictionary::new();
    dict.set(
        "Type",
        Object::Name(if plan.document_timestamp {
            b"DocTimeStamp".to_vec()
        } else {
            b"Sig".to_vec()
        }),
    );
    dict.set("Filter", Object::Name(b"Adobe.PPKLite".to_vec()));
    dict.set(
        "SubFilter",
        Object::Name(if plan.document_timestamp {
            b"ETSI.RFC3161".to_vec()
        } else {
            b"ETSI.CAdES.detached".to_vec()
        }),
    );
    dict.set(
        "ByteRange",
        Object::Array(vec![
            0.into(),
            9_999_999_999i64.into(),
            9_999_999_999i64.into(),
            9_999_999_999i64.into(),
        ]),
    );
    dict.set(
        "Contents",
        Object::String(vec![0u8; plan.reserve], StringFormat::Hexadecimal),
    );
    if !plan.document_timestamp {
        dict.set(
            "M",
            Object::string_literal(timefmt::pdf_date(plan.signed_at)),
        );
        text_entry(&mut dict, "Name", &plan.name);
        text_entry(&mut dict, "Reason", &plan.reason);
        text_entry(&mut dict, "Location", &plan.location);
        text_entry(&mut dict, "ContactInfo", &plan.contact);
    }
    if let Some(permission) = plan.certify {
        dict.set(
            "Reference",
            Object::Array(vec![Object::Dictionary(dictionary! {
                "Type" => "SigRef",
                "TransformMethod" => "DocMDP",
                "TransformParams" => dictionary! {
                    "Type" => "TransformParams",
                    "P" => i64::from(permission),
                    "V" => "1.2",
                },
                "DigestMethod" => "SHA256",
            })]),
        );
    }
    dict
}

/// The page that holds a widget: its `/P` entry, or the page whose annotations list it.
fn page_of(rev: &Revision, widget: ObjectId, widget_dict: &Dictionary) -> Option<ObjectId> {
    if let Ok(Object::Reference(p)) = widget_dict.get(b"P") {
        return Some(*p);
    }
    for page_id in acro::pages(rev.doc()) {
        let page = acro::object_dict(rev, page_id)?;
        let annots = page.get(b"Annots").ok().and_then(|a| acro::array(rev, a));
        if annots.is_some_and(|a| a.iter().any(|x| x.as_reference().ok() == Some(widget))) {
            return Some(page_id);
        }
    }
    None
}

/// Adds the revision with an empty signature and returns the file with room for it.
pub fn prepare(input: &[u8], plan: &SigPlan) -> Result<Prepared, AppError> {
    let mut rev = Revision::open(input)?;
    let fields = acro::fields(&rev);
    if let Some(p) = existing_certification(&rev)
        && p == 1
    {
        return Err(refuse(
            "This file was certified with no changes allowed, so a signature cannot be added. Ask the person who certified it for a version that allows signing.",
        ));
    }
    if plan.certify.is_some() && has_signature(&rev) {
        return Err(refuse(
            "Only the first signature of a file can certify it. Sign without certifying, or use a file that is not signed yet.",
        ));
    }
    if plan.certify.is_some() && existing_certification(&rev).is_some() {
        return Err(refuse(
            "This file is already certified. A file can be certified only once.",
        ));
    }

    let sig_id = rev.add(Object::Dictionary(signature_dictionary(plan)));
    let sig_ref = Object::Reference(sig_id);
    let field_name;

    if let Some(wanted) = &plan.field {
        let field = fields
            .iter()
            .find(|f| &f.name == wanted)
            .ok_or_else(|| refuse(&format!("The file has no signature field named \"{wanted}\". Check the name or leave the field empty to add a new one.")))?;
        if !field.is_signature() {
            return Err(refuse(&format!("\"{wanted}\" is not a signature field.")));
        }
        if field.is_filled() {
            return Err(refuse(&format!(
                "The field \"{wanted}\" is already signed."
            )));
        }
        let field_id = field.id.ok_or_else(|| {
            refuse("That signature field is stored in a way that cannot be filled here.")
        })?;
        // The widget is the field itself, or its first kid.
        let widget_id = if field
            .dict
            .get(b"Subtype")
            .ok()
            .and_then(|s| s.as_name().ok())
            == Some(b"Widget".as_slice())
        {
            field_id
        } else {
            field
                .dict
                .get(b"Kids")
                .ok()
                .and_then(|k| acro::array(&rev, k))
                .and_then(|k| k.first())
                .and_then(|k| k.as_reference().ok())
                .unwrap_or(field_id)
        };
        let mut widget = acro::dict(&rev, &Object::Reference(widget_id))
            .cloned()
            .ok_or_else(|| refuse("The signature field is damaged."))?;
        let rect = widget
            .get(b"Rect")
            .ok()
            .and_then(|r| acro::numbers(&rev, r))
            .filter(|r| r.len() == 4);
        if let Some(r) = rect {
            let (w, h) = ((r[2] - r[0]).abs(), (r[3] - r[1]).abs());
            if w >= 1.0 && h >= 1.0 {
                let page_id = page_of(&rev, widget_id, &widget);
                let matrix = page_id
                    .and_then(|p| acro::object_dict(&rev, p))
                    .map_or([1, 0, 0, 1], |page| page_box(&rev, page).matrix());
                let (aw, ah) = if matrix == [0, 1, -1, 0] || matrix == [0, -1, 1, 0] {
                    (h, w)
                } else {
                    (w, h)
                };
                let ap = rev.add(appearance(aw, ah, &plan.lines, matrix));
                widget.set(
                    "AP",
                    Object::Dictionary(dictionary! { "N" => Object::Reference(ap) }),
                );
            }
        }
        if widget_id == field_id {
            widget.set("V", sig_ref);
            rev.replace(field_id, Object::Dictionary(widget));
        } else {
            rev.replace(widget_id, Object::Dictionary(widget));
            let mut parent = field.dict.clone();
            parent.set("V", sig_ref);
            rev.replace(field_id, Object::Dictionary(parent));
        }
        field_name = field.name.clone();
    } else {
        field_name = unique_name(
            &fields,
            if plan.document_timestamp {
                "Timestamp"
            } else {
                "Signature"
            },
        );
        let pages = acro::pages(rev.doc());
        let target = plan.visible.map_or(0, |v| v.page as usize);
        let page_id = *pages
            .get(target)
            .ok_or_else(|| refuse("The page for the signature does not exist in this file."))?;
        let page_dict = acro::dict(&rev, &Object::Reference(page_id))
            .cloned()
            .ok_or_else(|| refuse("The PDF page is damaged and cannot be signed."))?;
        let boxed = page_box(&rev, &page_dict);
        let mut widget = dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "FT" => "Sig",
            "T" => pdf_text(&field_name),
            "V" => sig_ref,
            "F" => 132,
            "P" => Object::Reference(page_id),
        };
        match &plan.visible {
            Some(p) => {
                let r = boxed.rect(p);
                widget.set(
                    "Rect",
                    Object::Array(r.iter().map(|v| Object::Real(*v as f32)).collect()),
                );
                let matrix = boxed.matrix();
                let (rw, rh) = (r[2] - r[0], r[3] - r[1]);
                let (aw, ah) = if matrix == [0, 1, -1, 0] || matrix == [0, -1, 1, 0] {
                    (rh, rw)
                } else {
                    (rw, rh)
                };
                let ap = rev.add(appearance(aw, ah, &plan.lines, matrix));
                widget.set(
                    "AP",
                    Object::Dictionary(dictionary! { "N" => Object::Reference(ap) }),
                );
            }
            None => widget.set(
                "Rect",
                Object::Array(vec![0.into(), 0.into(), 0.into(), 0.into()]),
            ),
        }
        let widget_id = rev.add(Object::Dictionary(widget));
        push_into(&mut rev, page_id, "Annots", Object::Reference(widget_id))?;
        register_field(&mut rev, widget_id)?;
    }

    if plan.certify.is_some() {
        let root = rev.root()?;
        let mut catalog = acro::catalog(&rev)
            .cloned()
            .ok_or_else(|| refuse("The PDF has no catalog."))?;
        // Registering the field may already have changed the catalog.
        if let Some(Object::Dictionary(changed)) = rev.get(root) {
            catalog = changed.clone();
        }
        catalog.set(
            "Perms",
            Object::Dictionary(dictionary! { "DocMDP" => Object::Reference(sig_id) }),
        );
        rev.replace(root, Object::Dictionary(catalog));
    }

    let done = rev.finish()?;
    let at = *done
        .offsets
        .get(&sig_id.0)
        .ok_or_else(|| AppError::internal("locating the signature"))?;
    locate(done.bytes, at, field_name)
}

/// Finds the placeholders inside the signature object that starts at `object_at`.
fn locate(bytes: Vec<u8>, object_at: usize, field_name: String) -> Result<Prepared, AppError> {
    let find = |needle: &[u8], from: usize| {
        bytes[from..]
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|p| p + from)
    };
    let internal = || AppError::internal("locating the signature");
    let range_text = find(b"/ByteRange ", object_at).ok_or_else(internal)? + b"/ByteRange ".len();
    if &bytes[range_text..range_text + RANGE_PLACEHOLDER.len()] != RANGE_PLACEHOLDER.as_bytes() {
        return Err(internal());
    }
    let contents_open = find(b"/Contents <", object_at).ok_or_else(internal)? + b"/Contents ".len();
    let contents_close = find(b">", contents_open).ok_or_else(internal)?;
    let mut prepared = Prepared {
        bytes,
        field_name,
        byte_range_at: range_text,
        contents_open,
        contents_close,
    };
    // The ranges are part of the signed bytes, so they are written before the digest is made.
    prepared.write_ranges();
    Ok(prepared)
}

/// Writes an object to bytes, for tests and debugging.
#[cfg(test)]
fn render(object: &Object) -> String {
    let mut out = Vec::new();
    crate::pdfrev::write_object(object, &mut out);
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::Document;

    fn plan() -> SigPlan {
        SigPlan {
            signed_at: 1_800_000_000,
            reserve: 4000,
            lines: default_lines("Ada", 1_800_000_000, Some("Approval"), None, None),
            ..SigPlan::default()
        }
    }

    fn signature_dict(doc: &Document) -> Dictionary {
        doc.objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .find(|d| d.get(b"Type").ok().and_then(|t| t.as_name().ok()) == Some(b"Sig".as_slice()))
            .cloned()
            .expect("a signature dictionary")
    }

    #[test]
    fn an_invisible_signature_adds_a_field_and_keeps_the_original() {
        let original = test_support::pdfs::pdf_with_texts(&["one", "two"]);
        let prepared = prepare(&original, &plan()).unwrap();
        assert_eq!(&prepared.bytes[..original.len()], &original[..]);
        assert_eq!(prepared.field_name, "Signature1");
        let doc = Document::load_mem(&prepared.bytes).unwrap();
        let sig = signature_dict(&doc);
        assert_eq!(
            sig.get(b"SubFilter").unwrap().as_name().unwrap(),
            b"ETSI.CAdES.detached"
        );
        let fields = acro::fields(&doc);
        let field = fields.iter().find(|f| f.name == "Signature1").unwrap();
        assert!(field.is_signature() && field.is_filled());
        let first_page = doc.get_pages()[&1];
        let annots = doc.get_page_annotations(first_page).unwrap();
        assert_eq!(annots.len(), 1);
        let flags = acro::acroform(&doc)
            .unwrap()
            .get(b"SigFlags")
            .unwrap()
            .as_i64()
            .unwrap();
        assert_eq!(flags & 3, 3);
    }

    #[test]
    fn the_ranges_cover_everything_but_the_signature_space() {
        let prepared = prepare(&test_support::pdfs::pdf_with_texts(&["x"]), &plan()).unwrap();
        let r = prepared.ranges();
        assert_eq!(r[0], 0);
        assert_eq!(r[2] + r[3], prepared.bytes.len());
        assert_eq!(prepared.bytes[r[1]], b'<');
        assert_eq!(prepared.bytes[r[2] - 1], b'>');
        assert_eq!(prepared.capacity(), 4000);
        let h = prepared.digest(Hash::Sha256);
        assert_eq!(h.len(), 32);
    }

    #[test]
    fn embedding_writes_the_ranges_and_the_signature() {
        let prepared = prepare(&test_support::pdfs::pdf_with_texts(&["x"]), &plan()).unwrap();
        let ranges = prepared.ranges();
        let signed = prepared.embed(&[0xAB, 0xCD, 0xEF]).unwrap();
        let doc = Document::load_mem(&signed).unwrap();
        let sig = signature_dict(&doc);
        let br: Vec<i64> = sig
            .get(b"ByteRange")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_i64().unwrap())
            .collect();
        assert_eq!(
            br,
            vec![0, ranges[1] as i64, ranges[2] as i64, ranges[3] as i64]
        );
        let contents = sig.get(b"Contents").unwrap().as_str().unwrap();
        assert_eq!(&contents[..3], &[0xAB, 0xCD, 0xEF]);
        assert!(contents[3..].iter().all(|b| *b == 0));
    }

    #[test]
    fn a_signature_that_is_too_long_is_refused() {
        let prepared = prepare(&test_support::pdfs::pdf_with_texts(&["x"]), &plan()).unwrap();
        assert!(prepared.embed(&vec![1u8; 4001]).is_err());
    }

    #[test]
    fn a_visible_signature_gets_an_appearance_inside_the_page() {
        let mut p = plan();
        p.visible = Some(Placement {
            page: 1,
            left: 0.5,
            top: 0.8,
            width: 0.4,
            height: 0.1,
        });
        let prepared = prepare(&test_support::pdfs::pdf_with_texts(&["one", "two"]), &p).unwrap();
        let doc = Document::load_mem(&prepared.bytes).unwrap();
        let page2 = doc.get_pages()[&2];
        let annots = doc.get_page_annotations(page2).unwrap();
        assert_eq!(annots.len(), 1);
        let widget = annots[0];
        let rect = widget.get(b"Rect").unwrap().as_array().unwrap();
        let v: Vec<f64> = rect.iter().map(|n| acro::number(n).unwrap()).collect();
        // Letter page: 612 x 792. Left 0.5 of 612, top 0.8 down means 0.2 up from the bottom.
        assert!((v[0] - 306.0).abs() < 0.5, "{v:?}");
        assert!((v[2] - 550.8).abs() < 0.5, "{v:?}");
        assert!(
            (v[1] - 79.2).abs() < 0.5 && (v[3] - 158.4).abs() < 0.5,
            "{v:?}"
        );
        assert!(widget.has(b"AP"));
        assert!(
            doc.get_page_annotations(doc.get_pages()[&1])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_rotated_page_places_the_box_where_it_is_seen() {
        // 90 degrees clockwise: the left edge of the display is the bottom of the page.
        let pb = PageBox {
            x0: 0.0,
            y0: 0.0,
            w: 612.0,
            h: 792.0,
            rotate: 90,
        };
        let (dw, dh) = pb.displayed();
        assert_eq!((dw, dh), (792.0, 612.0));
        let r = pb.rect(&Placement {
            page: 0,
            left: 0.0,
            top: 0.0,
            width: 0.5,
            height: 0.5,
        });
        // The top left of the display is the bottom left of the unrotated page.
        assert!(
            (r[0] - 0.0).abs() < 0.01 && (r[1] - 0.0).abs() < 0.01,
            "{r:?}"
        );
        assert!(
            (r[2] - 306.0).abs() < 0.01 && (r[3] - 396.0).abs() < 0.01,
            "{r:?}"
        );
        assert_eq!(pb.matrix(), [0, 1, -1, 0]);
    }

    #[test]
    fn certifying_adds_docmdp_and_only_once() {
        let mut p = plan();
        p.certify = Some(2);
        let prepared = prepare(&test_support::pdfs::pdf_with_texts(&["x"]), &p).unwrap();
        let doc = Document::load_mem(&prepared.bytes).unwrap();
        assert_eq!(existing_certification(&doc), Some(2));
        let mut sig = plan();
        sig.certify = Some(1);
        let signed = prepared.embed(&[1, 2, 3]).unwrap();
        let err = prepare(&signed, &sig).err().unwrap();
        assert!(err.message.contains("certif"), "{}", err.message);
        // An ordinary signature is still allowed after a form-fill certification.
        assert!(prepare(&signed, &plan()).is_ok());
    }

    #[test]
    fn a_file_certified_with_no_changes_refuses_more_signatures() {
        let mut p = plan();
        p.certify = Some(1);
        let signed = prepare(&test_support::pdfs::pdf_with_texts(&["x"]), &p)
            .unwrap()
            .embed(&[1])
            .unwrap();
        let err = prepare(&signed, &plan()).err().unwrap();
        assert!(err.message.contains("no changes"), "{}", err.message);
    }

    #[test]
    fn a_second_signature_gets_a_new_name_and_the_first_stays() {
        let first = prepare(&test_support::pdfs::pdf_with_texts(&["x"]), &plan())
            .unwrap()
            .embed(&[1])
            .unwrap();
        let second = prepare(&first, &plan()).unwrap();
        assert_eq!(second.field_name, "Signature2");
        assert_eq!(&second.bytes[..first.len()], &first[..]);
    }

    #[test]
    fn an_existing_empty_field_is_filled() {
        let original = pdf_with_empty_signature_field("Approve");
        let mut p = plan();
        p.field = Some("Approve".into());
        let prepared = prepare(&original, &p).unwrap();
        assert_eq!(prepared.field_name, "Approve");
        let doc = Document::load_mem(&prepared.bytes).unwrap();
        let fields = acro::fields(&doc);
        assert_eq!(
            fields.iter().filter(|f| f.is_signature()).count(),
            1,
            "no new field"
        );
        assert!(
            fields
                .iter()
                .find(|f| f.name == "Approve")
                .unwrap()
                .is_filled()
        );
        let mut missing = plan();
        missing.field = Some("Nope".into());
        assert!(prepare(&original, &missing).is_err());
        let again = prepared.embed(&[1]).unwrap();
        assert!(
            prepare(&again, &p).is_err(),
            "a signed field cannot be signed twice"
        );
    }

    #[test]
    fn non_ascii_text_is_stored_as_utf16() {
        assert_eq!(render(&pdf_text("plain")), "(plain)");
        let Object::String(bytes, _) = pdf_text("Zoë") else {
            panic!("a string")
        };
        assert_eq!(&bytes[..2], &[0xFE, 0xFF]);
        assert_eq!(acro::text(&pdf_text("Zoë")).unwrap(), "Zoë");
    }

    #[test]
    fn a_protected_file_is_refused() {
        let plain = test_support::pdfs::pdf_with_texts(&["x"]);
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (dir.path().join("a.pdf"), dir.path().join("b.pdf"));
        std::fs::write(&src, plain).unwrap();
        test_support::encrypt_pdf(&src, &dst, "pw");
        let err = prepare(&std::fs::read(&dst).unwrap(), &plan())
            .err()
            .unwrap();
        assert_eq!(err.code, ErrorCode::PdfEncrypted);
    }

    /// A one page file with an empty signature field called `name`, as Create forms makes it.
    fn pdf_with_empty_signature_field(name: &str) -> Vec<u8> {
        let base = test_support::pdfs::pdf_with_texts(&["x"]);
        let mut rev = Revision::open(&base).unwrap();
        let page_id = acro::pages(rev.doc())[0];
        let widget = rev.add(Object::Dictionary(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "FT" => "Sig",
            "T" => pdf_text(name),
            "F" => 4,
            "Rect" => vec![100.into(), 100.into(), 300.into(), 160.into()],
            "P" => Object::Reference(page_id),
        }));
        push_into(&mut rev, page_id, "Annots", Object::Reference(widget)).unwrap();
        register_field(&mut rev, widget).unwrap();
        rev.finish().unwrap().bytes
    }
}
