//! Translate PDF: boxes of a page are covered and new text is written in them, in an installed
//! font that has the characters, at a size that makes it fit.

use application::AppError;
use application::ports::TextReplacement;
use domain::tools::editor::TextAlign;
use std::collections::BTreeMap;

use crate::fonts::Face;
use crate::paint::{Painter, Shared};
use lopdf::Document;

/// The smallest size the text is shrunk to.
const FLOOR_PT: f32 = 5.0;
/// The color of the patch under the new text.
const PATCH: &str = "#FFFFFF";
/// The color of the new text.
const INK: &str = "#000000";

/// Covers each box and writes its text. One font is chosen for the whole document, so a page
/// does not change typeface in the middle.
pub fn replace_text(doc: &mut Document, items: &[TextReplacement]) -> Result<(), AppError> {
    if items.is_empty() {
        return Ok(());
    }
    let all: String = items
        .iter()
        .map(|i| i.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let regular = Face::covering(&all, false, false)?;
    let bold = Face::covering(&all, true, false).unwrap_or_else(|_| regular.clone());
    let mut by_page: BTreeMap<u32, Vec<&TextReplacement>> = BTreeMap::new();
    for item in items {
        by_page.entry(item.page.get()).or_default().push(item);
    }
    let mut shared = Shared::default();
    for (page, items) in by_page {
        let mut painter = Painter::new(doc, page)?;
        for item in items {
            painter.patch(&item.region, PATCH)?;
            if item.text.trim().is_empty() {
                continue;
            }
            let face = if item.bold { &bold } else { &regular };
            let size = painter.fit_size(&item.region, &item.text, face, item.size_pt, FLOOR_PT);
            painter.text_face(
                doc,
                &mut shared,
                &item.region,
                &item.text,
                face,
                size,
                INK,
                TextAlign::Left,
            )?;
        }
        painter.finish(doc)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::PageIndex;
    use domain::tools::common::Region;
    use test_support::minimal_pdf;

    fn item(page: u32, text: &str, size: f32) -> TextReplacement {
        TextReplacement {
            page: PageIndex::new(page),
            region: Region {
                x: 0.1,
                y: 0.1,
                width: 0.5,
                height: 0.1,
            },
            text: text.to_owned(),
            size_pt: size,
            bold: false,
        }
    }

    fn page_content(doc: &Document, index: u32) -> String {
        let id = *doc.get_pages().get(&(index + 1)).unwrap();
        String::from_utf8_lossy(&doc.get_page_content(id)).into_owned()
    }

    #[test]
    fn latin_text_is_written_with_a_standard_font_over_a_patch() {
        let mut doc = Document::load_mem(&minimal_pdf(2)).unwrap();
        replace_text(&mut doc, &[item(1, "Hello", 12.0)]).unwrap();
        let content = page_content(&doc, 1);
        assert!(content.contains("(Hello) Tj"), "{content}");
        assert!(content.contains("1 1 1 rg"), "the patch: {content}");
        assert!(!page_content(&doc, 0).contains("Hello"));
    }

    #[test]
    fn long_text_shrinks_to_fit_its_box_but_not_below_the_floor() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        let long = "word ".repeat(200);
        replace_text(&mut doc, &[item(0, long.trim(), 24.0)]).unwrap();
        let content = page_content(&doc, 0);
        let size: f32 = content
            .lines()
            .find_map(|l| l.strip_suffix(" Tf"))
            .and_then(|l| l.split_whitespace().last())
            .and_then(|n| n.parse().ok())
            .unwrap();
        assert!((FLOOR_PT..24.0).contains(&size), "{size}");
    }

    #[test]
    fn nothing_changes_for_an_empty_list() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        let before = page_content(&doc, 0);
        replace_text(&mut doc, &[]).unwrap();
        assert_eq!(page_content(&doc, 0), before);
    }
}
