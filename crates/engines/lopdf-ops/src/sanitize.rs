//! Removing hidden and risky content: metadata, JavaScript, attachments, hidden layers, form
//! actions, thumbnails and private application data.

use application::AppError;
use domain::tools::secure::SanitizeOptions;
use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::{HashMap, HashSet};

use crate::MAX_STREAM_EXPANSION;
use crate::metadata::{remove_info, remove_xmp};

/// Actions that run code or leave the document.
const OUTWARD_ACTIONS: [&str; 12] = [
    "Launch",
    "URI",
    "GoToR",
    "GoToE",
    "SubmitForm",
    "ResetForm",
    "ImportData",
    "JavaScript",
    "Rendition",
    "Movie",
    "Sound",
    "Hide",
];

/// Every dictionary in the document, the dictionaries of streams included.
fn for_each_dict(doc: &mut Document, mut action: impl FnMut(ObjectId, &mut Dictionary)) {
    for (id, object) in &mut doc.objects {
        match object {
            Object::Dictionary(dict) => action(*id, dict),
            Object::Stream(stream) => action(*id, &mut stream.dict),
            _ => {}
        }
    }
}

/// Ids of action dictionaries, with the action name (`S`), for the given names.
fn action_ids(doc: &Document, names: &[&str]) -> HashMap<ObjectId, String> {
    let mut found = HashMap::new();
    for (id, object) in &doc.objects {
        if let Object::Dictionary(dict) = object
            && let Ok(name) = dict.get(b"S").and_then(Object::as_name)
        {
            let name = String::from_utf8_lossy(name).into_owned();
            if names.contains(&name.as_str()) {
                found.insert(*id, name);
            }
        }
    }
    found
}

/// True when `value` (a reference or an inline dictionary) is an action named in `names`.
fn is_action(value: &Object, ids: &HashMap<ObjectId, String>, names: &[&str]) -> bool {
    match value {
        Object::Reference(id) => ids.contains_key(id),
        Object::Dictionary(dict) => dict
            .get(b"S")
            .and_then(Object::as_name)
            .is_ok_and(|n| names.iter().any(|name| name.as_bytes() == n)),
        _ => false,
    }
}

fn remove_javascript(doc: &mut Document) {
    let js = action_ids(doc, &["JavaScript"]);
    if let Some(names) = names_dict_mut(doc) {
        names.remove(b"JavaScript");
    }
    for_each_dict(doc, |_, dict| {
        // Automatic actions of pages, fields and the document may run code without a click.
        dict.remove(b"AA");
        for key in [&b"A"[..], b"OpenAction"] {
            if dict
                .get(key)
                .is_ok_and(|v| is_action(v, &js, &["JavaScript"]))
            {
                dict.remove(key);
            }
        }
    });
}

fn remove_form_actions(doc: &mut Document) {
    let outward = action_ids(doc, &OUTWARD_ACTIONS);
    for_each_dict(doc, |_, dict| {
        let kind = dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(<[u8]>::to_vec)
            .unwrap_or_default();
        if kind == b"Widget" || kind == b"Link" {
            if kind == b"Widget" {
                dict.remove(b"AA");
            }
            if dict
                .get(b"A")
                .is_ok_and(|v| is_action(v, &outward, &OUTWARD_ACTIONS))
            {
                dict.remove(b"A");
            }
        }
    });
}

/// The catalog's `/Names` dictionary, wherever it is stored.
fn names_dict_mut(doc: &mut Document) -> Option<&mut Dictionary> {
    let reference = match doc.catalog().ok()?.get(b"Names").ok()? {
        Object::Reference(id) => Some(*id),
        _ => None,
    };
    match reference {
        Some(id) => doc.get_dictionary_mut(id).ok(),
        None => doc
            .catalog_mut()
            .ok()?
            .get_mut(b"Names")
            .ok()?
            .as_dict_mut()
            .ok(),
    }
}

fn remove_embedded_files(doc: &mut Document) {
    if let Some(names) = names_dict_mut(doc) {
        names.remove(b"EmbeddedFiles");
    }
    if let Ok(catalog) = doc.catalog_mut() {
        catalog.remove(b"AF");
    }
    for_each_dict(doc, |_, dict| {
        dict.remove(b"AF");
    });
    // File attachment annotations hold a file of their own.
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    for page in pages {
        drop_annotations(doc, page, |doc, id| {
            doc.get_dictionary(id)
                .ok()
                .and_then(|d| d.get(b"Subtype").ok())
                .and_then(|s| s.as_name().ok())
                .is_some_and(|n| n == b"FileAttachment")
        });
    }
}

/// Removes the annotations of `page` for which `drop` says so.
fn drop_annotations(
    doc: &mut Document,
    page: ObjectId,
    drop: impl Fn(&Document, ObjectId) -> bool,
) {
    let Ok(dict) = doc.get_dictionary(page) else {
        return;
    };
    let (items, array_id) = match dict.get(b"Annots") {
        Ok(Object::Array(items)) => (items.clone(), None),
        Ok(Object::Reference(id)) => match doc.get_object(*id) {
            Ok(Object::Array(items)) => (items.clone(), Some(*id)),
            _ => return,
        },
        _ => return,
    };
    let keep: Vec<Object> = items
        .iter()
        .filter(|o| !matches!(o, Object::Reference(id) if drop(doc, *id)))
        .cloned()
        .collect();
    if keep.len() == items.len() {
        return;
    }
    match array_id {
        Some(id) => {
            doc.objects.insert(id, Object::Array(keep));
        }
        None => {
            if let Ok(dict) = doc.get_dictionary_mut(page) {
                if keep.is_empty() {
                    dict.remove(b"Annots");
                } else {
                    dict.set("Annots", Object::Array(keep));
                }
            }
        }
    }
}

fn remove_thumbnails(doc: &mut Document) {
    for_each_dict(doc, |_, dict| {
        if dict
            .get(b"Type")
            .and_then(Object::as_name)
            .is_ok_and(|n| n == b"Page")
        {
            dict.remove(b"Thumb");
        }
    });
}

fn remove_private_data(doc: &mut Document) {
    for_each_dict(doc, |_, dict| {
        dict.remove(b"PieceInfo");
    });
}

fn ref_list(object: Option<&Object>) -> Vec<ObjectId> {
    match object {
        Some(Object::Array(items)) => items.iter().filter_map(|o| o.as_reference().ok()).collect(),
        Some(Object::Reference(id)) => vec![*id],
        _ => Vec::new(),
    }
}

/// The optional-content dictionary of the catalog and its default configuration.
fn oc_properties(doc: &Document) -> Option<Dictionary> {
    match doc.catalog().ok()?.get(b"OCProperties").ok()? {
        Object::Dictionary(d) => Some(d.clone()),
        Object::Reference(id) => doc.get_dictionary(*id).ok().cloned(),
        _ => None,
    }
}

/// Layers that are off in the default view.
fn hidden_layers(doc: &Document) -> HashSet<ObjectId> {
    let Some(props) = oc_properties(doc) else {
        return HashSet::new();
    };
    let config = match props.get(b"D") {
        Ok(Object::Dictionary(d)) => Some(d.clone()),
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).ok().cloned(),
        _ => None,
    };
    let Some(config) = config else {
        return HashSet::new();
    };
    let off_by_default = config
        .get(b"BaseState")
        .and_then(Object::as_name)
        .is_ok_and(|n| n == b"OFF");
    if off_by_default {
        let on: HashSet<ObjectId> = ref_list(config.get(b"ON").ok()).into_iter().collect();
        ref_list(props.get(b"OCGs").ok())
            .into_iter()
            .filter(|id| !on.contains(id))
            .collect()
    } else {
        ref_list(config.get(b"OFF").ok()).into_iter().collect()
    }
}

/// True when the optional-content group or membership dictionary `id` hides its content.
fn is_hidden(doc: &Document, id: ObjectId, off: &HashSet<ObjectId>) -> bool {
    if off.contains(&id) {
        return true;
    }
    let Ok(dict) = doc.get_dictionary(id) else {
        return false;
    };
    if dict
        .get(b"Type")
        .and_then(Object::as_name)
        .is_ok_and(|n| n == b"OCMD")
    {
        // Visible when any of its groups is on, which is the default policy.
        let groups = ref_list(dict.get(b"OCGs").ok());
        return !groups.is_empty() && groups.iter().all(|g| off.contains(g));
    }
    false
}

/// Names in the page resources (`Properties` or `XObject`) whose target is hidden.
fn hidden_names(
    doc: &Document,
    resources: &[Dictionary],
    category: &[u8],
    off: &HashSet<ObjectId>,
    through_xobject: bool,
) -> HashSet<Vec<u8>> {
    let mut names = HashSet::new();
    for res in resources {
        let table = match res.get(category) {
            Ok(Object::Dictionary(d)) => Some(d),
            Ok(Object::Reference(id)) => doc.get_dictionary(*id).ok(),
            _ => None,
        };
        let Some(table) = table else { continue };
        for (name, value) in table.iter() {
            let Object::Reference(id) = value else {
                continue;
            };
            let hidden = if through_xobject {
                // An image or form XObject points at its group with `/OC`.
                let oc = match doc.get_object(*id) {
                    Ok(Object::Stream(s)) => s.dict.get(b"OC").ok().cloned(),
                    _ => None,
                };
                matches!(oc, Some(Object::Reference(group)) if is_hidden(doc, group, off))
            } else {
                is_hidden(doc, *id, off)
            };
            if hidden {
                names.insert(name.clone());
            }
        }
    }
    names
}

fn page_resources(doc: &Document, page: ObjectId) -> Vec<Dictionary> {
    let Ok((direct, ids)) = doc.get_page_resources(page) else {
        return Vec::new();
    };
    let mut all: Vec<Dictionary> = direct.into_iter().cloned().collect();
    for id in ids {
        if let Ok(d) = doc.get_dictionary(id) {
            all.push(d.clone());
        }
    }
    all
}

/// Drops marked content and images that belong to hidden layers from one page. Returns whether
/// anything was removed.
fn strip_hidden_content(
    doc: &mut Document,
    page: ObjectId,
    off: &HashSet<ObjectId>,
) -> Result<bool, AppError> {
    let resources = page_resources(doc, page);
    let hidden_props = hidden_names(doc, &resources, b"Properties", off, false);
    let hidden_xobjects = hidden_names(doc, &resources, b"XObject", off, true);
    let mut changed = false;

    if !hidden_props.is_empty() || !hidden_xobjects.is_empty() {
        let bytes = doc
            .get_page_content_with_limit(page, MAX_STREAM_EXPANSION)
            .map_err(|_| AppError::internal("reading the page content"))?;
        let content =
            Content::decode(&bytes).map_err(|_| AppError::internal("reading the page content"))?;
        let mut kept = Vec::with_capacity(content.operations.len());
        // Depth of marked-content sections, and the depth at which a hidden one began.
        let mut depth = 0usize;
        let mut hidden_from: Option<usize> = None;
        for op in content.operations {
            match op.operator.as_str() {
                "BDC" | "BMC" => {
                    depth += 1;
                    if hidden_from.is_none()
                        && op.operator == "BDC"
                        && matches!(op.operands.first(), Some(Object::Name(tag)) if tag == b"OC")
                        && matches!(op.operands.get(1), Some(Object::Name(n)) if hidden_props.contains(n))
                    {
                        hidden_from = Some(depth);
                        changed = true;
                    }
                    if hidden_from.is_none() {
                        kept.push(op);
                    }
                }
                "EMC" => {
                    let closes_hidden = hidden_from == Some(depth);
                    depth = depth.saturating_sub(1);
                    if closes_hidden {
                        hidden_from = None;
                    } else if hidden_from.is_none() {
                        kept.push(op);
                    }
                }
                "Do" if hidden_from.is_none()
                    && matches!(op.operands.first(), Some(Object::Name(n)) if hidden_xobjects.contains(n)) =>
                {
                    changed = true;
                }
                _ if hidden_from.is_some() => {}
                _ => kept.push(op),
            }
        }
        if changed {
            let encoded = Content { operations: kept }
                .encode()
                .map_err(|_| AppError::internal("writing the page content"))?;
            let stream = doc.add_object(Stream::new(Dictionary::new(), encoded));
            if let Ok(dict) = doc.get_dictionary_mut(page) {
                dict.set("Contents", Object::Reference(stream));
            }
            forget_properties(doc, page, &hidden_props);
        }
    }

    // Annotations that belong to a hidden layer go too.
    let before = annotation_count(doc, page);
    drop_annotations(doc, page, |doc, id| {
        matches!(
            doc.get_dictionary(id).ok().and_then(|d| d.get(b"OC").ok()),
            Some(Object::Reference(group)) if is_hidden(doc, *group, off)
        )
    });
    Ok(changed || annotation_count(doc, page) != before)
}

/// Takes layer names out of a page's `Resources /Properties`, so the hidden layers they point
/// at are no longer referenced.
fn forget_properties(doc: &mut Document, page: ObjectId, names: &HashSet<Vec<u8>>) {
    let resources_ref = doc
        .get_dictionary(page)
        .ok()
        .and_then(|d| d.get(b"Resources").ok())
        .and_then(|o| o.as_reference().ok());
    let resources = match resources_ref {
        Some(id) => doc.get_dictionary_mut(id).ok(),
        None => doc
            .get_dictionary_mut(page)
            .ok()
            .and_then(|d| d.get_mut(b"Resources").ok())
            .and_then(|o| o.as_dict_mut().ok()),
    };
    let Some(resources) = resources else { return };
    let table_ref = resources
        .get(b"Properties")
        .ok()
        .and_then(|o| o.as_reference().ok());
    if table_ref.is_none() {
        if let Ok(Object::Dictionary(table)) = resources.get_mut(b"Properties") {
            for name in names {
                table.remove(name);
            }
        }
        return;
    }
    if let Some(id) = table_ref
        && let Ok(table) = doc.get_dictionary_mut(id)
    {
        for name in names {
            table.remove(name);
        }
    }
}

fn annotation_count(doc: &Document, page: ObjectId) -> usize {
    doc.get_dictionary(page)
        .ok()
        .and_then(|d| d.get(b"Annots").ok())
        .map_or(0, |a| match a {
            Object::Array(items) => items.len(),
            Object::Reference(id) => doc
                .get_object(*id)
                .ok()
                .and_then(|o| o.as_array().ok())
                .map_or(0, Vec::len),
            _ => 0,
        })
}

fn remove_hidden_layers(doc: &mut Document) -> Result<(), AppError> {
    let off = hidden_layers(doc);
    if off.is_empty() {
        return Ok(());
    }
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    for page in pages {
        strip_hidden_content(doc, page, &off)?;
    }
    // The layers themselves are listed in the catalog; take the hidden ones out.
    let Some(mut props) = oc_properties(doc) else {
        return Ok(());
    };
    let without = |list: Option<&Object>| -> Vec<Object> {
        match list {
            Some(Object::Array(items)) => items
                .iter()
                .filter(|o| !matches!(o, Object::Reference(id) if off.contains(id)))
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    };
    let remaining = without(props.get(b"OCGs").ok());
    if remaining.is_empty() {
        if let Ok(catalog) = doc.catalog_mut() {
            catalog.remove(b"OCProperties");
        }
        return Ok(());
    }
    props.set("OCGs", Object::Array(remaining));
    let config = match props.get(b"D") {
        Ok(Object::Dictionary(d)) => Some(d.clone()),
        Ok(Object::Reference(id)) => doc.get_dictionary(*id).ok().cloned(),
        _ => None,
    };
    if let Some(mut config) = config {
        for key in [&b"OFF"[..], b"ON", b"Order"] {
            if config.has(key) {
                config.set(key.to_vec(), Object::Array(without(config.get(key).ok())));
            }
        }
        config.remove(b"BaseState");
        props.set("D", Object::Dictionary(config));
    }
    if let Ok(catalog) = doc.catalog_mut() {
        catalog.set("OCProperties", Object::Dictionary(props));
    }
    Ok(())
}

/// Applies the chosen removals and drops every object nothing refers to afterwards.
pub fn sanitize(doc: &mut Document, options: &SanitizeOptions) -> Result<(), AppError> {
    // Layers first: deciding what is hidden needs the layer lists intact.
    if options.hidden_layers {
        remove_hidden_layers(doc)?;
    }
    if options.javascript {
        remove_javascript(doc);
    }
    if options.form_actions {
        remove_form_actions(doc);
    }
    if options.embedded_files {
        remove_embedded_files(doc);
    }
    if options.thumbnails {
        remove_thumbnails(doc);
    }
    if options.private_data {
        remove_private_data(doc);
    }
    if options.metadata {
        remove_info(doc);
    }
    if options.xmp {
        remove_xmp(doc);
    }
    doc.prune_objects();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::pdfs;

    fn load() -> Document {
        Document::load_mem(&pdfs::pdf_with_risky_content()).unwrap()
    }

    fn only(f: impl FnOnce(&mut SanitizeOptions)) -> SanitizeOptions {
        let mut o = SanitizeOptions {
            metadata: false,
            xmp: false,
            javascript: false,
            embedded_files: false,
            hidden_layers: false,
            form_actions: false,
            thumbnails: false,
            private_data: false,
        };
        f(&mut o);
        o
    }

    fn page(doc: &Document) -> ObjectId {
        doc.get_pages()[&1]
    }

    fn page_text(doc: &Document) -> String {
        String::from_utf8_lossy(&doc.get_page_content(page(doc))).into_owned()
    }

    /// True when any string, name or stream in the file holds `needle`.
    fn any_object_contains(doc: &Document, needle: &str) -> bool {
        fn holds(object: &Object, needle: &[u8]) -> bool {
            let has = |bytes: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
            match object {
                Object::String(bytes, _) | Object::Name(bytes) => has(bytes),
                Object::Array(items) => items.iter().any(|o| holds(o, needle)),
                Object::Dictionary(d) => d.iter().any(|(k, v)| has(k) || holds(v, needle)),
                Object::Stream(s) => {
                    s.dict.iter().any(|(k, v)| has(k) || holds(v, needle))
                        || has(&s.get_plain_content().unwrap_or_else(|_| s.content.clone()))
                }
                _ => false,
            }
        }
        doc.objects.values().any(|o| holds(o, needle.as_bytes()))
    }

    #[test]
    fn everything_goes_with_the_defaults_and_the_visible_page_survives() {
        let mut doc = load();
        sanitize(&mut doc, &SanitizeOptions::default()).unwrap();
        for needle in [
            "Secret title",
            "secret/",
            "app.alert",
            "attached secret",
            "Hidden text",
            "http://example.com",
            "page secret",
        ] {
            assert!(
                !any_object_contains(&doc, needle) && !page_text(&doc).contains(needle),
                "{needle}"
            );
        }
        let text = page_text(&doc);
        assert!(
            text.contains("Visible text") && text.contains("Shown layer"),
            "{text}"
        );
        assert!(doc.trailer.get(b"Info").is_err());
        let catalog = doc.catalog().unwrap();
        for key in [
            &b"OpenAction"[..],
            b"Names",
            b"Metadata",
            b"PieceInfo",
            b"OCProperties",
        ] {
            let gone = match catalog.get(key) {
                Err(_) => true,
                // Names may remain as an empty dictionary.
                Ok(Object::Dictionary(d)) => d.is_empty(),
                Ok(_) => false,
            };
            assert!(
                gone || key == b"OCProperties",
                "{}",
                String::from_utf8_lossy(key)
            );
        }
    }

    #[test]
    fn each_option_removes_only_its_own_thing() {
        let mut doc = load();
        sanitize(&mut doc, &only(|o| o.metadata = true)).unwrap();
        assert!(doc.trailer.get(b"Info").is_err());
        assert!(any_object_contains(&doc, "app.alert"), "scripts stay");
        assert!(
            any_object_contains(&doc, "<secret/>")
                || doc.catalog().unwrap().get(b"Metadata").is_ok(),
            "xmp stays"
        );

        let mut doc = load();
        sanitize(&mut doc, &only(|o| o.javascript = true)).unwrap();
        assert!(!any_object_contains(&doc, "app.alert"));
        assert!(any_object_contains(&doc, "attached secret"));
        assert!(doc.catalog().unwrap().get(b"OpenAction").is_err());

        let mut doc = load();
        sanitize(&mut doc, &only(|o| o.thumbnails = true)).unwrap();
        assert!(
            doc.get_dictionary(page(&doc))
                .unwrap()
                .get(b"Thumb")
                .is_err()
        );
        assert!(
            any_object_contains(&doc, "page secret"),
            "private data stays"
        );

        let mut doc = load();
        sanitize(&mut doc, &only(|o| o.private_data = true)).unwrap();
        assert!(!any_object_contains(&doc, "page secret"));
        assert!(
            doc.get_dictionary(page(&doc))
                .unwrap()
                .get(b"Thumb")
                .is_ok()
        );
    }

    #[test]
    fn attachments_leave_the_name_tree_and_the_annotation() {
        let mut doc = load();
        sanitize(&mut doc, &only(|o| o.embedded_files = true)).unwrap();
        assert!(!any_object_contains(&doc, "attached secret"));
        assert!(!any_object_contains(&doc, "FileAttachment"));
        assert!(
            any_object_contains(&doc, "http://example.com"),
            "the web link stays"
        );
    }

    #[test]
    fn form_actions_remove_links_that_open_something() {
        let mut doc = load();
        sanitize(&mut doc, &only(|o| o.form_actions = true)).unwrap();
        assert!(!any_object_contains(&doc, "http://example.com"));
        assert!(
            any_object_contains(&doc, "app.alert"),
            "only form and link actions go"
        );
    }

    #[test]
    fn hidden_layers_lose_their_content_but_visible_layers_stay() {
        let mut doc = load();
        sanitize(&mut doc, &only(|o| o.hidden_layers = true)).unwrap();
        let text = page_text(&doc);
        assert!(!text.contains("Hidden text"), "{text}");
        assert!(
            text.contains("Visible text") && text.contains("Shown layer"),
            "{text}"
        );
        assert!(
            !any_object_contains(&doc, "Hidden layer"),
            "the layer itself is gone"
        );
        assert!(any_object_contains(&doc, "Visible layer"));
        // The result still parses as content.
        assert!(Content::decode(text.as_bytes()).is_ok());
    }

    #[test]
    fn a_document_without_risks_comes_through_unchanged_in_content() {
        let mut doc = Document::load_mem(&pdfs::pdf_with_texts(&["Page 1", "Page 2"])).unwrap();
        sanitize(&mut doc, &SanitizeOptions::default()).unwrap();
        assert_eq!(doc.get_pages().len(), 2);
        assert!(page_text(&doc).contains("Page 1"));
    }
}
