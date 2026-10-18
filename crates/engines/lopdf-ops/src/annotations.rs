//! Removing annotations from pages.

use application::AppError;
use domain::PageIndex;
use domain::tools::document::AnnotationKind;
use lopdf::{Document, Object, ObjectId};

/// The `/Subtype` of an annotation object, as text.
fn subtype(doc: &Document, id: ObjectId) -> Option<String> {
    let dict = doc.get_dictionary(id).ok()?;
    let name = dict.get(b"Subtype").ok()?.as_name().ok()?;
    Some(String::from_utf8_lossy(name).into_owned())
}

/// True when an annotation of this subtype is one of `kinds`. Form fields (`Widget`) and
/// signatures are never removed here: Flatten and Unlock handle those.
fn matches(kinds: &[AnnotationKind], subtype: &str) -> bool {
    if subtype == "Widget" {
        return false;
    }
    kinds.iter().any(|kind| match kind {
        AnnotationKind::All => true,
        AnnotationKind::Highlights => subtype == "Highlight",
        AnnotationKind::Strikeouts => subtype == "StrikeOut",
        AnnotationKind::Notes => subtype == "Text",
        AnnotationKind::Links => subtype == "Link",
    })
}

/// The ids in a page's `/Annots` array, wherever the array is stored.
fn annots(doc: &Document, page: ObjectId) -> (Vec<Object>, Option<ObjectId>) {
    let Ok(dict) = doc.get_dictionary(page) else {
        return (Vec::new(), None);
    };
    match dict.get(b"Annots") {
        Ok(Object::Array(items)) => (items.clone(), None),
        Ok(Object::Reference(id)) => match doc.get_object(*id) {
            Ok(Object::Array(items)) => (items.clone(), Some(*id)),
            _ => (Vec::new(), None),
        },
        _ => (Vec::new(), None),
    }
}

/// Removes annotations of `kinds` from `pages`. A removed note takes its popup with it, and the
/// data of removed annotations leaves the file.
pub fn remove_annotations(
    doc: &mut Document,
    kinds: &[AnnotationKind],
    pages: &[PageIndex],
) -> Result<(), AppError> {
    let page_ids = doc.get_pages();
    for index in pages {
        let Some(&page) = page_ids.get(&(index.get() + 1)) else {
            return Err(AppError::internal("finding a page to clean"));
        };
        let (items, stored_at) = annots(doc, page);
        if items.is_empty() {
            continue;
        }
        let mut keep: Vec<Object> = Vec::new();
        let mut dropped: Vec<ObjectId> = Vec::new();
        let mut popups: Vec<ObjectId> = Vec::new();
        for item in items {
            let Object::Reference(id) = item else {
                keep.push(item);
                continue;
            };
            match subtype(doc, id) {
                Some(kind) if matches(kinds, &kind) => {
                    if let Ok(dict) = doc.get_dictionary(id)
                        && let Ok(Object::Reference(popup)) = dict.get(b"Popup")
                    {
                        popups.push(*popup);
                    }
                    dropped.push(id);
                }
                _ => keep.push(Object::Reference(id)),
            }
        }
        // A popup whose parent is gone is useless; one whose parent stays must stay too.
        keep.retain(|o| !matches!(o, Object::Reference(id) if popups.contains(id)));
        if dropped.is_empty() {
            continue;
        }
        match stored_at {
            Some(array_id) => {
                if keep.is_empty() {
                    if let Ok(dict) = doc.get_dictionary_mut(page) {
                        dict.remove(b"Annots");
                    }
                } else {
                    doc.objects.insert(array_id, Object::Array(keep));
                }
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
    // Annotations nothing refers to any more, with their appearance streams, are dropped.
    doc.prune_objects();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::pdfs;

    fn load() -> Document {
        Document::load_mem(&pdfs::pdf_with_annotations()).unwrap()
    }

    /// Subtypes of the annotations on one-based page `n`.
    fn kinds_on(doc: &Document, n: u32) -> Vec<String> {
        let page = doc.get_pages()[&n];
        annots(doc, page)
            .0
            .iter()
            .filter_map(|o| o.as_reference().ok())
            .filter_map(|id| subtype(doc, id))
            .collect()
    }

    fn pages(v: &[u32]) -> Vec<PageIndex> {
        v.iter().map(|p| PageIndex::new(*p)).collect()
    }

    #[test]
    fn all_removes_everything_except_form_fields() {
        let mut doc = load();
        remove_annotations(&mut doc, &[AnnotationKind::All], &pages(&[0, 1])).unwrap();
        assert_eq!(kinds_on(&doc, 1), ["Widget"]);
        assert!(kinds_on(&doc, 2).is_empty());
        assert!(
            doc.get_dictionary(doc.get_pages()[&2])
                .unwrap()
                .get(b"Annots")
                .is_err()
        );
    }

    #[test]
    fn removes_only_the_chosen_kinds() {
        let mut doc = load();
        remove_annotations(
            &mut doc,
            &[AnnotationKind::Highlights, AnnotationKind::Strikeouts],
            &pages(&[0, 1]),
        )
        .unwrap();
        assert_eq!(kinds_on(&doc, 1), ["Link", "Text", "Popup", "Widget"]);
        assert_eq!(kinds_on(&doc, 2), ["Link", "Text"]);

        let mut doc = load();
        remove_annotations(&mut doc, &[AnnotationKind::Links], &pages(&[0, 1])).unwrap();
        assert_eq!(
            kinds_on(&doc, 1),
            ["Highlight", "StrikeOut", "Text", "Popup", "Widget"]
        );
        assert_eq!(kinds_on(&doc, 2), ["Text"]);
    }

    #[test]
    fn a_removed_note_takes_its_popup_along() {
        let mut doc = load();
        remove_annotations(&mut doc, &[AnnotationKind::Notes], &pages(&[0])).unwrap();
        assert_eq!(
            kinds_on(&doc, 1),
            ["Link", "Highlight", "StrikeOut", "Widget"]
        );
        assert_eq!(
            kinds_on(&doc, 2),
            ["Link", "Text"],
            "other pages are untouched"
        );
    }

    #[test]
    fn only_the_selected_pages_change_and_removed_data_leaves_the_file() {
        let mut doc = load();
        let before = doc.objects.len();
        remove_annotations(&mut doc, &[AnnotationKind::All], &pages(&[1])).unwrap();
        assert_eq!(kinds_on(&doc, 1).len(), 6);
        assert!(kinds_on(&doc, 2).is_empty());
        assert_eq!(
            doc.objects.len(),
            before - 2,
            "both page 2 annotations were deleted"
        );
    }
}
