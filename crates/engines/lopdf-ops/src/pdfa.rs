//! Making a document fit PDF/A: what is forbidden is taken out, what is required is added.
//!
//! The work here is normalization, not conversion of content: fonts that are not in the file
//! are embedded from the fonts installed on the computer, a color profile and an XMP packet are
//! added, and actions, scripts, attachments and annotation settings that PDF/A does not allow are
//! removed. Whether the result conforms is decided by the validator afterwards, and what cannot
//! be fixed here (tagging, CMYK, transparency in part 1) is left for it to report.

use std::collections::{HashMap, HashSet};

use application::AppError;
use application::ports::PdfaNormalizer;
use domain::tools::pdfa::PdfaLevel;
use domain::tools::secure::SanitizeOptions;
use lopdf::xref::XrefType;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::fonts::{Embedded, find_installed};
use crate::pdfa_icc;
use crate::pdfa_xmp::{Info, Moment, packet};
use crate::{LopdfStructure, sanitize};

/// Annotation types that PDF/A does not allow at all.
const FORBIDDEN_ANNOTATIONS: [&str; 6] = ["Sound", "Movie", "Screen", "3D", "RichMedia", "TrapNet"];
/// Actions that PDF/A does not allow.
const FORBIDDEN_ACTIONS: [&str; 13] = [
    "Launch",
    "Sound",
    "Movie",
    "ResetForm",
    "ImportData",
    "JavaScript",
    "Hide",
    "SetOCGState",
    "Rendition",
    "Trans",
    "GoTo3DView",
    "RichMediaExecute",
    "SetState",
];
/// Annotation flags that must be clear: invisible, hidden, no view, toggle no view.
const BAD_FLAGS: i64 = 1 | 2 | 32 | 256;
const PRINT_FLAG: i64 = 4;

fn name_of(dict: &Dictionary, key: &[u8]) -> Option<String> {
    dict.get(key)
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| String::from_utf8_lossy(n).into_owned())
}

/// Every dictionary in the document, those of streams included.
fn for_each_dict(doc: &mut Document, mut action: impl FnMut(ObjectId, &mut Dictionary)) {
    for (id, object) in &mut doc.objects {
        match object {
            Object::Dictionary(dict) => action(*id, dict),
            Object::Stream(stream) => action(*id, &mut stream.dict),
            _ => {}
        }
    }
}

/// What the run did, for the person to read.
#[derive(Default)]
struct Notes(Vec<String>);

impl Notes {
    fn add(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !self.0.contains(&text) {
            self.0.push(text);
        }
    }
}

// --- Streams -------------------------------------------------------------------------------

/// LZW is not allowed and a stream may not live in another file: both are fixed in place.
fn fix_streams(doc: &mut Document, notes: &mut Notes) {
    let mut recompress: Vec<ObjectId> = Vec::new();
    for (id, object) in &mut doc.objects {
        let Object::Stream(stream) = object else {
            continue;
        };
        for key in ["F", "FFilter", "FDecodeParms"] {
            if stream.dict.remove(key.as_bytes()).is_some() {
                notes.add("Removed references to external files");
            }
        }
        if has_filter(&stream.dict, b"LZWDecode") {
            recompress.push(*id);
        }
    }
    for id in recompress {
        if let Ok(Object::Stream(stream)) = doc.get_object_mut(id)
            && stream.decompress().is_ok()
        {
            let _ = stream.compress();
            notes.add("Recompressed streams that used LZW");
        }
    }
}

fn has_filter(dict: &Dictionary, name: &[u8]) -> bool {
    match dict.get(b"Filter") {
        Ok(Object::Name(n)) => n == name,
        Ok(Object::Array(items)) => items
            .iter()
            .any(|i| matches!(i, Object::Name(n) if n == name)),
        _ => false,
    }
}

// --- Document level ------------------------------------------------------------------------

fn fix_catalog(doc: &mut Document, level: PdfaLevel, notes: &mut Notes) {
    let Ok(catalog) = doc.catalog_mut() else {
        return;
    };
    for key in [
        "OpenAction",
        "AA",
        "Perms",
        "Requirements",
        "NeedsRendering",
        "Legal",
    ] {
        if let Some(value) = catalog.get(key.as_bytes()).ok().cloned() {
            // A plain destination in OpenAction is harmless; an action that runs something is not.
            let harmless = key == "OpenAction" && matches!(value, Object::Array(_));
            if !harmless {
                catalog.remove(key.as_bytes());
                notes.add(format!("Removed {key} from the document"));
            }
        }
    }
    if let Ok(Object::Dictionary(form)) = catalog.get_mut(b"AcroForm") {
        form.remove(b"NeedAppearances");
        if form.remove(b"XFA").is_some() {
            notes.add("Removed XFA form data");
        }
    }
    if level.is_pdf14() && catalog.remove(b"OCProperties").is_some() {
        notes.add("Removed layers (PDF/A-1 has none)");
    }
}

/// The form may be an object of its own.
fn fix_acroform(doc: &mut Document) {
    let id = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"AcroForm").ok())
        .and_then(|f| f.as_reference().ok());
    if let Some(id) = id
        && let Ok(form) = doc.get_dictionary_mut(id)
    {
        form.remove(b"NeedAppearances");
        form.remove(b"XFA");
    }
}

/// Layer configurations: each needs a name and none may switch layers by itself.
fn fix_layers(doc: &mut Document) {
    let Ok(catalog) = doc.catalog() else { return };
    let Some(props) = catalog.get(b"OCProperties").ok().and_then(|p| match p {
        Object::Reference(id) => Some(*id),
        _ => None,
    }) else {
        // Direct dictionary in the catalog.
        if let Ok(catalog) = doc.catalog_mut()
            && let Ok(Object::Dictionary(props)) = catalog.get_mut(b"OCProperties")
        {
            clean_layers(props);
        }
        return;
    };
    if let Ok(props) = doc.get_dictionary_mut(props) {
        clean_layers(props);
    }
}

fn clean_layers(props: &mut Dictionary) {
    let name = |config: &mut Dictionary, fallback: &str| {
        config.remove(b"AS");
        if !config.has(b"Name") {
            config.set("Name", Object::string_literal(fallback));
        }
    };
    if let Ok(Object::Dictionary(d)) = props.get_mut(b"D") {
        name(d, "Default");
    }
    if let Ok(Object::Array(configs)) = props.get_mut(b"Configs") {
        for (i, c) in configs.iter_mut().enumerate() {
            if let Object::Dictionary(d) = c {
                name(d, &format!("Configuration {}", i + 1));
            }
        }
    }
}

// --- Pages, annotations and actions --------------------------------------------------------

fn fix_pages(doc: &mut Document, level: PdfaLevel, notes: &mut Notes) {
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    for page in pages {
        if let Ok(dict) = doc.get_dictionary_mut(page) {
            if dict.remove(b"AA").is_some() {
                notes.add("Removed page actions");
            }
            dict.remove(b"Trans");
            if level.is_pdf14()
                && let Ok(Object::Dictionary(group)) = dict.get_mut(b"Group")
                && name_of(group, b"S").as_deref() == Some("Transparency")
            {
                dict.remove(b"Group");
                notes.add("Removed page transparency groups (PDF/A-1 has no transparency)");
            }
        }
    }
}

/// Annotation dictionaries: objects of type `Annot`, or with a subtype and a rectangle.
fn annotation_ids(doc: &Document) -> Vec<ObjectId> {
    doc.objects
        .iter()
        .filter_map(|(id, o)| {
            let Object::Dictionary(d) = o else {
                return None;
            };
            let is_annot = name_of(d, b"Type").as_deref() == Some("Annot")
                || (d.has(b"Subtype") && d.has(b"Rect") && name_of(d, b"Type").is_none());
            is_annot.then_some(*id)
        })
        .collect()
}

fn remove_action_if_forbidden(
    dict: &mut Dictionary,
    key: &[u8],
    doc_actions: &HashSet<ObjectId>,
) -> bool {
    let forbidden = match dict.get(key) {
        Ok(Object::Dictionary(action)) => {
            name_of(action, b"S").is_some_and(|s| FORBIDDEN_ACTIONS.contains(&s.as_str()))
        }
        Ok(Object::Reference(id)) => doc_actions.contains(id),
        _ => false,
    };
    if forbidden {
        dict.remove(key);
    }
    forbidden
}

fn fix_annotations(doc: &mut Document, level: PdfaLevel, notes: &mut Notes) {
    // Actions that live in objects of their own.
    let forbidden_actions: HashSet<ObjectId> = doc
        .objects
        .iter()
        .filter_map(|(id, o)| match o {
            Object::Dictionary(d)
                if name_of(d, b"S").is_some_and(|s| FORBIDDEN_ACTIONS.contains(&s.as_str())) =>
            {
                Some(*id)
            }
            _ => None,
        })
        .collect();
    let mut removed: HashSet<ObjectId> = HashSet::new();
    for id in annotation_ids(doc) {
        let (subtype, has_ap) = {
            let Ok(d) = doc.get_dictionary(id) else {
                continue;
            };
            (
                name_of(d, b"Subtype").unwrap_or_default(),
                has_appearance(doc, d),
            )
        };
        let drop_it = FORBIDDEN_ANNOTATIONS.contains(&subtype.as_str())
            || (subtype == "FileAttachment" && !level.allows_attachments())
            || (!matches!(subtype.as_str(), "Popup" | "Link") && !has_ap);
        if drop_it {
            removed.insert(id);
            notes.add(format!(
                "Removed {subtype} annotations that PDF/A cannot hold"
            ));
            continue;
        }
        // The appearance dictionary may hold the normal appearance only.
        let ap_id = doc
            .get_dictionary(id)
            .ok()
            .and_then(|d| d.get(b"AP").ok())
            .and_then(|a| a.as_reference().ok());
        if let Some(ap) = ap_id
            && let Ok(ap) = doc.get_dictionary_mut(ap)
        {
            ap.remove(b"D");
            ap.remove(b"R");
        }
        let Ok(d) = doc.get_dictionary_mut(id) else {
            continue;
        };
        if let Ok(Object::Dictionary(ap)) = d.get_mut(b"AP") {
            ap.remove(b"D");
            ap.remove(b"R");
        }
        if subtype != "Popup" {
            let flags = d.get(b"F").ok().and_then(|f| f.as_i64().ok()).unwrap_or(0);
            let fixed = (flags | PRINT_FLAG) & !BAD_FLAGS;
            if fixed != flags || !d.has(b"F") {
                d.set("F", Object::Integer(fixed));
                notes.add("Set the print flag of annotations and cleared the hidden ones");
            }
        }
        d.remove(b"AA");
        if remove_action_if_forbidden(d, b"A", &forbidden_actions) {
            notes.add("Removed actions that PDF/A does not allow");
        }
    }
    // Fields and links keep no action that runs something.
    for_each_dict(doc, |_, d| {
        if name_of(d, b"Type").as_deref() == Some("Annot") || d.has(b"FT") || d.has(b"Subtype") {
            d.remove(b"AA");
            remove_action_if_forbidden(d, b"A", &forbidden_actions);
        }
    });
    if removed.is_empty() {
        return;
    }
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    for page in pages {
        let Ok(dict) = doc.get_dictionary_mut(page) else {
            continue;
        };
        match dict.get_mut(b"Annots") {
            Ok(Object::Array(items)) => {
                items.retain(|i| !matches!(i, Object::Reference(r) if removed.contains(r)));
            }
            Ok(Object::Reference(list)) => {
                let list = *list;
                if let Ok(Object::Array(items)) = doc.get_object_mut(list) {
                    items.retain(|i| !matches!(i, Object::Reference(r) if removed.contains(r)));
                }
            }
            _ => {}
        }
    }
    for id in removed {
        doc.objects.remove(&id);
    }
}

fn has_appearance(doc: &Document, annot: &Dictionary) -> bool {
    let ap = match annot.get(b"AP") {
        Ok(Object::Dictionary(d)) => Some(d),
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).ok(),
        _ => None,
    };
    ap.is_some_and(|ap| ap.has(b"N"))
}

// --- Pictures and graphics state -----------------------------------------------------------

fn fix_graphics(doc: &mut Document, notes: &mut Notes) {
    for_each_dict(doc, |_, d| {
        if name_of(d, b"Subtype").as_deref() == Some("Image") {
            if matches!(d.get(b"Interpolate"), Ok(Object::Boolean(true))) {
                d.set("Interpolate", Object::Boolean(false));
                notes.add("Turned off smoothing of enlarged pictures");
            }
            d.remove(b"Alternates");
            d.remove(b"OPI");
        }
        if name_of(d, b"Type").as_deref() == Some("ExtGState") || d.has(b"CA") || d.has(b"ca") {
            for key in ["TR", "TR2", "HT"] {
                let default =
                    matches!(d.get(key.as_bytes()), Ok(Object::Name(n)) if n == b"Default");
                if d.has(key.as_bytes()) && !default {
                    d.remove(key.as_bytes());
                    notes.add("Removed transfer functions and halftones");
                }
            }
        }
    });
}

// --- Fonts ---------------------------------------------------------------------------------

/// The standard font families and the installed fonts that match their measurements.
fn substitute_family(base: &str) -> Option<&'static str> {
    let lower = base.to_ascii_lowercase();
    let stem = lower.split([',', '-']).next().unwrap_or(&lower);
    match stem {
        "helvetica" | "arial" | "arialmt" => Some("Arial"),
        "times" | "timesnewroman" | "timesnewromanps" | "timesnewromanpsmt" | "times new roman" => {
            Some("Times New Roman")
        }
        "courier" | "couriernew" | "couriernewps" | "couriernewpsmt" | "courier new" => {
            Some("Courier New")
        }
        _ => None,
    }
}

fn is_bold(base: &str) -> bool {
    let l = base.to_ascii_lowercase();
    l.contains("bold") || l.contains("black") || l.contains("heavy")
}

fn is_italic(base: &str) -> bool {
    let l = base.to_ascii_lowercase();
    l.contains("italic") || l.contains("oblique")
}

/// The name without the six capital letters and plus that mark a subset.
fn without_subset_tag(name: &str) -> &str {
    match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()) => rest,
        _ => name,
    }
}

/// The family of an installed font that carries this name, such as `Calibri-Bold`.
fn family_guess(base: &str) -> String {
    let stem = base.split([',', '-']).next().unwrap_or(base);
    let cleaned = stem
        .strip_suffix("PSMT")
        .or_else(|| stem.strip_suffix("MT"))
        .or_else(|| stem.strip_suffix("PS"))
        .unwrap_or(stem);
    // `SegoeUI` is `Segoe UI`: put a space before each capital that follows a lower case letter.
    let mut out = String::new();
    let mut previous_lower = false;
    for c in cleaned.chars() {
        if c.is_ascii_uppercase() && previous_lower {
            out.push(' ');
        }
        previous_lower = c.is_ascii_lowercase();
        out.push(c);
    }
    out
}

fn is_embedded(doc: &Document, font: &Dictionary) -> bool {
    let descriptor = match font.get(b"FontDescriptor") {
        Ok(Object::Dictionary(d)) => Some(d),
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).ok(),
        _ => None,
    };
    descriptor.is_some_and(|d| d.has(b"FontFile") || d.has(b"FontFile2") || d.has(b"FontFile3"))
}

/// True when the codes of the font mean what WinAnsi says they mean: no encoding at all (the
/// standard fonts use their built in one, which agrees for letters and digits), or WinAnsi.
fn encoding_is_plain(doc: &Document, font: &Dictionary) -> bool {
    match font.get(b"Encoding") {
        Err(_) => true,
        Ok(Object::Name(n)) => n == b"WinAnsiEncoding" || n == b"StandardEncoding",
        Ok(Object::Dictionary(d)) => encoding_dict_is_plain(d),
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).is_ok_and(encoding_dict_is_plain),
        _ => false,
    }
}

fn encoding_dict_is_plain(d: &Dictionary) -> bool {
    !d.has(b"Differences")
        && match d.get(b"BaseEncoding") {
            Err(_) => true,
            Ok(Object::Name(n)) => n == b"WinAnsiEncoding" || n == b"StandardEncoding",
            _ => false,
        }
}

fn fix_fonts(doc: &mut Document, level: PdfaLevel, notes: &mut Notes) {
    let mut cache: HashMap<(String, bool, bool), Option<Embedded>> = HashMap::new();
    let ids: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter_map(|(id, o)| match o {
            Object::Dictionary(d)
                if name_of(d, b"Type").as_deref() == Some("Font")
                    && matches!(
                        name_of(d, b"Subtype").as_deref(),
                        Some("Type1" | "TrueType" | "MMType1")
                    ) =>
            {
                Some(*id)
            }
            _ => None,
        })
        .collect();
    for id in ids {
        let Some((base, plain)) = ({
            let Ok(font) = doc.get_dictionary(id) else {
                continue;
            };
            if is_embedded(doc, font) {
                continue;
            }
            name_of(font, b"BaseFont").map(|b| (b, encoding_is_plain(doc, font)))
        }) else {
            continue;
        };
        let clean = without_subset_tag(&base).to_owned();
        let family = substitute_family(&clean)
            .map(str::to_owned)
            .unwrap_or_else(|| family_guess(&clean));
        if !plain {
            notes.add(format!(
                "The font {clean} uses a custom encoding and could not be embedded"
            ));
            continue;
        }
        let key = (family.clone(), is_bold(&clean), is_italic(&clean));
        let found = cache
            .entry(key)
            .or_insert_with_key(|k| find_installed(&k.0, k.1, k.2));
        let Some(embedded) = found.clone() else {
            notes.add(format!(
                "The font {clean} is not installed and could not be embedded"
            ));
            continue;
        };
        let mut replacement = embedded.build(doc);
        if level.needs_unicode() {
            let to_unicode = Embedded::to_unicode_stream(doc);
            replacement.set("ToUnicode", Object::Reference(to_unicode));
        }
        doc.objects.insert(id, Object::Dictionary(replacement));
        notes.add(format!("Embedded {family} in place of {clean}"));
    }
}

// --- Color, metadata, trailer --------------------------------------------------------------

fn fix_output_intent(doc: &mut Document, notes: &mut Notes) {
    let existing: Vec<Object> = match doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"OutputIntents").ok())
    {
        Some(Object::Array(items)) => items.clone(),
        Some(Object::Reference(id)) => match doc.get_object(*id) {
            Ok(Object::Array(items)) => items.clone(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    // An intent that brings its own profile is kept, and named as PDF/A wants.
    let mut kept: Vec<Object> = Vec::new();
    for item in existing {
        let id = match &item {
            Object::Reference(id) => Some(*id),
            _ => None,
        };
        let has_profile = match &item {
            Object::Dictionary(d) => d.has(b"DestOutputProfile"),
            Object::Reference(r) => doc
                .get_dictionary(*r)
                .is_ok_and(|d| d.has(b"DestOutputProfile")),
            _ => false,
        };
        if has_profile {
            if let Some(id) = id
                && let Ok(d) = doc.get_dictionary_mut(id)
            {
                d.set("S", Object::Name(b"GTS_PDFA1".to_vec()));
            }
            kept.push(item);
        }
    }
    if kept.is_empty() {
        let mut profile = Stream::new(Dictionary::new(), pdfa_icc::srgb());
        profile.dict.set("N", Object::Integer(3));
        let _ = profile.compress();
        let profile_id = doc.add_object(profile);
        let mut intent = Dictionary::new();
        intent.set("Type", Object::Name(b"OutputIntent".to_vec()));
        intent.set("S", Object::Name(b"GTS_PDFA1".to_vec()));
        intent.set(
            "OutputConditionIdentifier",
            Object::string_literal("sRGB IEC61966-2.1"),
        );
        intent.set(
            "RegistryName",
            Object::string_literal("http://www.color.org"),
        );
        intent.set("Info", Object::string_literal("sRGB IEC61966-2.1"));
        intent.set("DestOutputProfile", Object::Reference(profile_id));
        let intent_id = doc.add_object(Object::Dictionary(intent));
        kept.push(Object::Reference(intent_id));
        notes.add("Added the sRGB color profile as the output intent");
    }
    if let Ok(catalog) = doc.catalog_mut() {
        catalog.set("OutputIntents", Object::Array(kept));
    }
}

fn info_id(doc: &mut Document) -> ObjectId {
    if let Some(id) = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|i| i.as_reference().ok())
        && doc.get_dictionary(id).is_ok()
    {
        return id;
    }
    let id = doc.add_object(Dictionary::new());
    doc.trailer.set("Info", Object::Reference(id));
    id
}

fn fix_metadata(doc: &mut Document, level: PdfaLevel, now: Moment, notes: &mut Notes) {
    let id = info_id(doc);
    let mut info = doc.get_dictionary(id).map(Info::read).unwrap_or_default();
    if info.producer.is_none() {
        info.producer = Some("Quirewise".to_owned());
    }
    if let Ok(dict) = doc.get_dictionary_mut(id) {
        info.write(dict, now);
    }
    let xmp = packet(&info, level, now).into_bytes();
    let mut stream = Stream::new(Dictionary::new(), xmp);
    stream.dict.set("Type", Object::Name(b"Metadata".to_vec()));
    stream.dict.set("Subtype", Object::Name(b"XML".to_vec()));
    // The packet must stay readable as text, so it is never compressed.
    stream.set_plain_content(stream.content.clone());
    let xmp_id = doc.add_object(stream);
    if let Ok(catalog) = doc.catalog_mut() {
        catalog.set("Metadata", Object::Reference(xmp_id));
    }
    notes.add(format!(
        "Wrote the XMP metadata that claims {}",
        level.name()
    ));
}

fn fix_trailer(doc: &mut Document, now: Moment) {
    if doc.trailer.has(b"ID") {
        return;
    }
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (now.0, doc.objects.len(), doc.max_id).hash(&mut hasher);
    let a = hasher.finish();
    (a, now.0).hash(&mut hasher);
    let b = hasher.finish();
    let mut bytes = a.to_be_bytes().to_vec();
    bytes.extend_from_slice(&b.to_be_bytes());
    let id = Object::String(bytes, lopdf::StringFormat::Hexadecimal);
    doc.trailer.set("ID", Object::Array(vec![id.clone(), id]));
}

/// Takes out what only exists for the file format of newer readers.
fn flatten_structure(doc: &mut Document, level: PdfaLevel) {
    doc.objects.retain(|_, o| {
        let Object::Stream(s) = o else { return true };
        !matches!(
            name_of(&s.dict, b"Type").as_deref(),
            Some("ObjStm" | "XRef")
        )
    });
    doc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;
    doc.version = if level.is_pdf14() { "1.4" } else { "1.7" }.to_owned();
}

/// Applies every fix to a loaded document and returns what was done.
pub(crate) fn normalize(
    doc: &mut Document,
    level: PdfaLevel,
    now: Moment,
) -> Result<Vec<String>, AppError> {
    let mut notes = Notes::default();
    // Scripts, attachments and private data go first so that nothing below works on them.
    sanitize::sanitize(
        doc,
        &SanitizeOptions {
            metadata: false,
            xmp: false,
            javascript: true,
            embedded_files: !level.allows_attachments(),
            hidden_layers: false,
            form_actions: false,
            thumbnails: true,
            private_data: true,
        },
    )?;
    fix_catalog(doc, level, &mut notes);
    fix_acroform(doc);
    fix_layers(doc);
    fix_streams(doc, &mut notes);
    fix_pages(doc, level, &mut notes);
    fix_annotations(doc, level, &mut notes);
    fix_graphics(doc, &mut notes);
    fix_fonts(doc, level, &mut notes);
    fix_output_intent(doc, &mut notes);
    fix_metadata(doc, level, now, &mut notes);
    fix_trailer(doc, now);
    flatten_structure(doc, level);
    doc.prune_objects();
    if level.needs_tags() {
        notes.add("Tagging for accessibility cannot be added, so level A is not reached");
    }
    Ok(notes.0)
}

/// [`PdfaNormalizer`] over lopdf.
#[derive(Debug, Clone, Copy, Default)]
pub struct LopdfPdfa;

impl PdfaNormalizer for LopdfPdfa {
    fn normalize(
        &self,
        source: &std::path::Path,
        level: PdfaLevel,
        now_unix: i64,
        dest: &std::path::Path,
    ) -> Result<Vec<String>, AppError> {
        let mut doc = LopdfStructure::load(source, None)?;
        let notes = normalize(&mut doc, level, Moment(now_unix))?;
        doc.save(dest)
            .map_err(|e| AppError::from_io("write the PDF/A document", &e))?;
        Ok(notes)
    }
}

/// Reads the title of a document, for tests.
#[cfg(test)]
fn title_of(doc: &Document) -> Option<String> {
    use crate::metadata::read_text;
    let id = doc.trailer.get(b"Info").ok()?.as_reference().ok()?;
    doc.get_dictionary(id)
        .ok()?
        .get(b"Title")
        .ok()
        .and_then(read_text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::pdfs;

    const NOW: i64 = 1_800_000_000;

    fn run(bytes: &[u8], level: PdfaLevel) -> (Document, Vec<String>) {
        let mut doc = Document::load_mem(bytes).unwrap();
        let notes = normalize(&mut doc, level, Moment(NOW)).unwrap();
        // Round trip through a file, as the tool does.
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        (Document::load_mem(&out).unwrap(), notes)
    }

    #[test]
    fn a_plain_file_gets_metadata_an_output_intent_an_id_and_the_claim() {
        let (doc, notes) = run(&pdfs::pdf_with_metadata("Report", "Ada"), PdfaLevel::A2b);
        let catalog = doc.catalog().unwrap();
        assert!(catalog.has(b"Metadata") && catalog.has(b"OutputIntents"));
        assert!(doc.trailer.has(b"ID"));
        assert_eq!(title_of(&doc).as_deref(), Some("Report"));
        let Object::Stream(xmp) = doc
            .get_object(catalog.get(b"Metadata").unwrap().as_reference().unwrap())
            .unwrap()
        else {
            panic!("a stream")
        };
        assert!(!xmp.dict.has(b"Filter"), "the packet is plain text");
        let text = String::from_utf8_lossy(&xmp.content).into_owned();
        assert!(text.contains("<pdfaid:part>2</pdfaid:part>"), "{text}");
        assert!(text.contains("<pdfaid:conformance>B</pdfaid:conformance>"));
        assert!(text.contains("Report") && text.contains("Ada"));
        assert!(notes.iter().any(|n| n.contains("XMP")));
        assert!(notes.iter().any(|n| n.contains("output intent")));
    }

    #[test]
    fn the_output_intent_carries_a_three_channel_profile() {
        let (doc, _) = run(&pdfs::pdf_with_texts(&["x"]), PdfaLevel::A2b);
        let intents = doc
            .catalog()
            .unwrap()
            .get(b"OutputIntents")
            .unwrap()
            .as_array()
            .unwrap();
        let intent = doc
            .get_dictionary(intents[0].as_reference().unwrap())
            .unwrap();
        assert_eq!(intent.get(b"S").unwrap().as_name().unwrap(), b"GTS_PDFA1");
        let Object::Stream(profile) = doc
            .get_object(
                intent
                    .get(b"DestOutputProfile")
                    .unwrap()
                    .as_reference()
                    .unwrap(),
            )
            .unwrap()
        else {
            panic!("a stream")
        };
        assert_eq!(profile.dict.get(b"N").unwrap().as_i64().unwrap(), 3);
        let bytes = profile.decompressed_content().unwrap();
        assert_eq!(&bytes[36..40], b"acsp");
    }

    #[test]
    fn level_u_adds_a_unicode_map_and_level_b_does_not() {
        let has_to_unicode = |doc: &Document| {
            doc.objects.values().any(|o| {
                o.as_dict().is_ok_and(|d| {
                    name_of(d, b"Type").as_deref() == Some("Font") && d.has(b"ToUnicode")
                })
            })
        };
        let source = pdfs::pdf_with_texts(&["x"]);
        let (b, _) = run(&source, PdfaLevel::A2b);
        let (u, _) = run(&source, PdfaLevel::A2u);
        // Only fonts that were embedded here get one, and that needs an installed Arial.
        if let Some(_installed) = find_installed("Arial", false, false) {
            assert!(!has_to_unicode(&b));
            assert!(has_to_unicode(&u));
        }
    }

    #[test]
    fn standard_fonts_are_embedded_when_the_matching_font_is_installed() {
        let (doc, notes) = run(&pdfs::pdf_with_texts(&["x"]), PdfaLevel::A2b);
        let embedded = doc.objects.values().any(|o| {
            o.as_dict().is_ok_and(|d| {
                name_of(d, b"Type").as_deref() == Some("Font") && is_embedded(&doc, d)
            })
        });
        if find_installed("Arial", false, false).is_some() {
            assert!(embedded, "{notes:?}");
        } else {
            assert!(notes.iter().any(|n| n.contains("not installed")));
        }
    }

    #[test]
    fn level_a_says_it_cannot_tag() {
        let (_, notes) = run(&pdfs::pdf_with_texts(&["x"]), PdfaLevel::A2a);
        assert!(notes.iter().any(|n| n.contains("Tagging")), "{notes:?}");
        let (_, plain) = run(&pdfs::pdf_with_texts(&["x"]), PdfaLevel::A2b);
        assert!(!plain.iter().any(|n| n.contains("Tagging")));
    }

    #[test]
    fn scripts_and_attachments_are_removed_from_parts_one_and_two_only() {
        let source = pdfs::pdf_with_risky_content();
        let (two, _) = run(&source, PdfaLevel::A2b);
        let any = |doc: &Document, needle: &[u8]| {
            doc.objects.values().any(|o| match o {
                Object::Stream(s) => s.content.windows(needle.len()).any(|w| w == needle),
                Object::Dictionary(d) => {
                    format!("{d:?}").contains(&String::from_utf8_lossy(needle).into_owned())
                }
                _ => false,
            })
        };
        assert!(!any(&two, b"app.alert"), "JavaScript is gone");
        assert!(!any(&two, b"attached secret"), "the attachment is gone");
        let (three, _) = run(&source, PdfaLevel::A3b);
        assert!(
            !any(&three, b"app.alert"),
            "JavaScript is gone from part 3 too"
        );
    }

    #[test]
    fn annotations_get_the_print_flag_and_lose_forbidden_parts() {
        let (doc, notes) = run(&pdfs::pdf_with_annotations(), PdfaLevel::A2b);
        for id in annotation_ids(&doc) {
            let d = doc.get_dictionary(id).unwrap();
            if name_of(d, b"Subtype").as_deref() == Some("Popup") {
                continue;
            }
            let flags = d.get(b"F").unwrap().as_i64().unwrap();
            assert_eq!(flags & PRINT_FLAG, PRINT_FLAG, "{d:?}");
            assert_eq!(flags & BAD_FLAGS, 0, "{d:?}");
            assert!(!d.has(b"AA"));
        }
        let _ = notes;
    }

    #[test]
    fn the_structure_is_plain_for_old_readers() {
        let (doc, _) = run(&pdfs::pdf_with_texts(&["x"]), PdfaLevel::A1b);
        assert_eq!(doc.version, "1.4");
        assert!(!doc.objects.values().any(|o| matches!(o, Object::Stream(s)
            if matches!(name_of(&s.dict, b"Type").as_deref(), Some("ObjStm" | "XRef")))));
        let (seven, _) = run(&pdfs::pdf_with_texts(&["x"]), PdfaLevel::A3b);
        assert_eq!(seven.version, "1.7");
    }

    #[test]
    fn names_of_fonts_are_understood() {
        assert_eq!(substitute_family("Helvetica-Bold"), Some("Arial"));
        assert_eq!(
            substitute_family("TimesNewRomanPS-BoldMT"),
            Some("Times New Roman")
        );
        assert_eq!(substitute_family("CourierNewPSMT"), Some("Courier New"));
        assert_eq!(substitute_family("Calibri"), None);
        assert!(is_bold("Arial-BoldMT") && !is_bold("ArialMT"));
        assert!(is_italic("Helvetica-Oblique") && is_italic("Times-Italic"));
        assert_eq!(without_subset_tag("ABCDEF+Calibri"), "Calibri");
        assert_eq!(without_subset_tag("Calibri"), "Calibri");
        assert_eq!(family_guess("SegoeUI-Semibold"), "Segoe UI");
        assert_eq!(family_guess("Calibri-Bold"), "Calibri");
        assert_eq!(family_guess("Verdana"), "Verdana");
    }
}
