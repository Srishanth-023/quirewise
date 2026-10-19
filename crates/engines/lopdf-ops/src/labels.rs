//! Page labels: the numbering shown by viewers (`i, ii, iii, 1, 2, 3, A-1`), stored as a number
//! tree in the catalog.

use application::AppError;
use domain::tools::structure::LabelRange;
use lopdf::{Dictionary, Document, Object};

use crate::outline::text_string;

fn catalog_id(doc: &Document) -> Result<lopdf::ObjectId, AppError> {
    doc.trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| AppError::internal("editing the document"))
}

/// Replaces the page labels with `ranges` (sorted by first page), or removes them when the
/// list is empty. Pages before the first range are numbered 1, 2, 3 as PDF requires.
pub fn set_page_labels(doc: &mut Document, ranges: &[LabelRange]) -> Result<(), AppError> {
    let id = catalog_id(doc)?;
    let catalog = doc
        .get_dictionary_mut(id)
        .map_err(|_| AppError::internal("editing the document"))?;
    if ranges.is_empty() {
        catalog.remove(b"PageLabels");
        return Ok(());
    }
    let mut nums: Vec<Object> = Vec::new();
    if ranges.first().is_none_or(|r| r.from_page > 1) {
        nums.push(Object::Integer(0));
        let mut implicit = Dictionary::new();
        implicit.set("S", Object::Name(b"D".to_vec()));
        nums.push(Object::Dictionary(implicit));
    }
    for range in ranges {
        let mut label = Dictionary::new();
        label.set(
            "S",
            Object::Name(range.style.label_code().as_bytes().to_vec()),
        );
        if !range.prefix.is_empty() {
            label.set("P", text_string(&range.prefix));
        }
        if range.start != 1 {
            label.set("St", Object::Integer(i64::from(range.start)));
        }
        nums.push(Object::Integer(i64::from(range.from_page) - 1));
        nums.push(Object::Dictionary(label));
    }
    let mut tree = Dictionary::new();
    tree.set("Nums", Object::Array(nums));
    doc.get_dictionary_mut(id)
        .map_err(|_| AppError::internal("editing the document"))?
        .set("PageLabels", Object::Dictionary(tree));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::stamp::NumberStyle;
    use test_support::minimal_pdf;

    fn nums(doc: &Document) -> Vec<Object> {
        let catalog = doc.catalog().unwrap();
        let tree = catalog.get(b"PageLabels").unwrap().as_dict().unwrap();
        tree.get(b"Nums").unwrap().as_array().unwrap().clone()
    }

    fn range(from_page: u32, style: NumberStyle, prefix: &str, start: u32) -> LabelRange {
        LabelRange {
            from_page,
            style,
            prefix: prefix.into(),
            start,
        }
    }

    #[test]
    fn ranges_become_a_number_tree_starting_at_page_index_zero() {
        let mut doc = Document::load_mem(&minimal_pdf(6)).unwrap();
        set_page_labels(
            &mut doc,
            &[
                range(1, NumberStyle::RomanLower, "", 1),
                range(3, NumberStyle::Arabic, "A-", 5),
            ],
        )
        .unwrap();
        let n = nums(&doc);
        assert_eq!(n.len(), 4);
        assert_eq!(n[0].as_i64().unwrap(), 0);
        let first = n[1].as_dict().unwrap();
        assert_eq!(first.get(b"S").unwrap().as_name().unwrap(), b"r");
        assert!(first.get(b"St").is_err() && first.get(b"P").is_err());
        assert_eq!(n[2].as_i64().unwrap(), 2);
        let second = n[3].as_dict().unwrap();
        assert_eq!(second.get(b"S").unwrap().as_name().unwrap(), b"D");
        assert_eq!(second.get(b"St").unwrap().as_i64().unwrap(), 5);
        assert_eq!(second.get(b"P").unwrap().as_str().unwrap(), b"A-");
    }

    #[test]
    fn pages_before_the_first_range_get_plain_numbers() {
        let mut doc = Document::load_mem(&minimal_pdf(6)).unwrap();
        set_page_labels(&mut doc, &[range(4, NumberStyle::LettersUpper, "", 1)]).unwrap();
        let n = nums(&doc);
        assert_eq!(n[0].as_i64().unwrap(), 0);
        assert_eq!(
            n[1].as_dict()
                .unwrap()
                .get(b"S")
                .unwrap()
                .as_name()
                .unwrap(),
            b"D"
        );
        assert_eq!(n[2].as_i64().unwrap(), 3);
        assert_eq!(
            n[3].as_dict()
                .unwrap()
                .get(b"S")
                .unwrap()
                .as_name()
                .unwrap(),
            b"A"
        );
    }

    #[test]
    fn an_empty_list_removes_the_labels() {
        let mut doc = Document::load_mem(&minimal_pdf(2)).unwrap();
        set_page_labels(&mut doc, &[range(1, NumberStyle::Arabic, "", 1)]).unwrap();
        assert!(doc.catalog().unwrap().get(b"PageLabels").is_ok());
        set_page_labels(&mut doc, &[]).unwrap();
        assert!(doc.catalog().unwrap().get(b"PageLabels").is_err());
    }

    #[test]
    fn non_ascii_prefixes_are_stored_as_text_strings() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        set_page_labels(&mut doc, &[range(1, NumberStyle::Arabic, "\u{00C9}-", 1)]).unwrap();
        let n = nums(&doc);
        let prefix = n[1]
            .as_dict()
            .unwrap()
            .get(b"P")
            .unwrap()
            .as_str()
            .unwrap()
            .to_vec();
        assert_eq!(&prefix[..2], &[0xFE, 0xFF]);
    }
}
