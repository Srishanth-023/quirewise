//! The report Compare writes: a summary, the text changes side by side, and a picture of every
//! page that looks different. It is laid out as items for the editor engine on blank Letter pages,
//! so no second PDF writer is needed.

use std::path::PathBuf;

use domain::tools::editor::{EditItem, ImageItem};

use super::comparediff::{Line, RowKind, TextDiff};
use super::reportkit::{Builder, Face, INK, MARGIN, MUTED, PAGE, Report, wrap};

/// Rows of text changes the report lists; the rest are counted.
pub const MAX_ROWS: usize = 1_500;
/// Pages the report lists with their look differences before it stops listing them.
const MAX_LISTED_PAGES: usize = 30;

const BODY: f32 = 8.0;
const LEADING: f32 = 10.4;
/// Characters that fit on a line of a column at the body size, with room to spare.
const COLUMN_CHARS: usize = 56;
const LEFT_TEXT: (f32, f32) = (40.0, 248.0);
const RIGHT_TEXT: (f32, f32) = (320.0, 252.0);
const LEFT_GUTTER: f32 = 12.0;
const RIGHT_GUTTER: f32 = 292.0;

const REMOVED_TEXT: &str = "#9A1B1B";
const ADDED_TEXT: &str = "#1B5E20";
const REMOVED_FILL: &str = "#FBE3E0";
const ADDED_FILL: &str = "#DDF0DD";

/// What the two documents are.
pub struct Summary {
    pub original_name: String,
    pub changed_name: String,
    pub original_pages: u32,
    pub changed_pages: u32,
    /// Pages compared, by number.
    pub compared_pages: usize,
}

/// How one page looks different, with the picture of it.
pub struct VisualPage {
    pub page: u32,
    pub percent: f32,
    pub size_differs: bool,
    /// The overlay as a PNG file.
    pub image: PathBuf,
}

impl Builder {
    fn columns_header(&mut self, original: &str, changed: &str) {
        let y = self.y;
        let face = Face {
            size: 8.0,
            bold: true,
            color: INK,
        };
        self.text(
            (LEFT_TEXT.0, LEFT_TEXT.1, y),
            &format!("Original: {original}"),
            face,
        );
        self.text(
            (RIGHT_TEXT.0, RIGHT_TEXT.1, y),
            &format!("Changed: {changed}"),
            face,
        );
        self.y += 14.0;
    }
}

fn cell(builder: &mut Builder, line: Option<&Line>, left: bool, y: f32, color: &str) -> usize {
    let Some(line) = line else {
        return 0;
    };
    let (x, width) = if left { LEFT_TEXT } else { RIGHT_TEXT };
    let lines = wrap(&line.text, COLUMN_CHARS);
    let gutter = if left { LEFT_GUTTER } else { RIGHT_GUTTER };
    let marker = Face {
        size: 6.5,
        bold: false,
        color: MUTED,
    };
    builder.text((gutter, 26.0, y), &format!("p{}", line.page), marker);
    let face = Face {
        size: BODY,
        bold: false,
        color,
    };
    for (i, text) in lines.iter().enumerate() {
        builder.text((x, width, y + i as f32 * LEADING), text, face);
    }
    lines.len()
}

fn cell_lines(line: Option<&Line>) -> usize {
    line.map_or(0, |l| wrap(&l.text, COLUMN_CHARS).len())
}

fn rows(builder: &mut Builder, summary: &Summary, diff: &TextDiff) {
    builder.line("Text changes", 13.0, true, INK);
    builder.gap(4.0);
    builder.columns_header(&summary.original_name, &summary.changed_name);
    let shown = diff.rows.len().min(MAX_ROWS);
    for row in &diff.rows[..shown] {
        if row.kind == RowKind::Gap {
            if !builder.room(LEADING) {
                builder.new_page();
                builder.columns_header(&summary.original_name, &summary.changed_name);
            }
            let y = builder.y;
            let face = Face {
                size: BODY,
                bold: false,
                color: MUTED,
            };
            builder.text((LEFT_TEXT.0, 532.0, y), "...", face);
            builder.y += LEADING;
            continue;
        }
        let height = cell_lines(row.left.as_ref())
            .max(cell_lines(row.right.as_ref()))
            .max(1) as f32
            * LEADING
            + 3.0;
        if !builder.room(height) {
            builder.new_page();
            builder.columns_header(&summary.original_name, &summary.changed_name);
        }
        let y = builder.y;
        let (left_color, right_color) = match row.kind {
            RowKind::Removed => (REMOVED_TEXT, INK),
            RowKind::Added => (INK, ADDED_TEXT),
            RowKind::Changed => (REMOVED_TEXT, ADDED_TEXT),
            _ => (MUTED, MUTED),
        };
        if matches!(row.kind, RowKind::Removed | RowKind::Changed) {
            builder.fill(
                LEFT_TEXT.0 - 2.0,
                y - 1.0,
                LEFT_TEXT.1 + 4.0,
                height,
                REMOVED_FILL,
            );
        }
        if matches!(row.kind, RowKind::Added | RowKind::Changed) {
            builder.fill(
                RIGHT_TEXT.0 - 2.0,
                y - 1.0,
                RIGHT_TEXT.1 + 4.0,
                height,
                ADDED_FILL,
            );
        }
        cell(builder, row.left.as_ref(), true, y, left_color);
        cell(builder, row.right.as_ref(), false, y, right_color);
        builder.y += height;
    }
    if diff.rows.len() > shown {
        builder.gap(4.0);
        builder.line(
            &format!(
                "{} more rows are not listed. Compare fewer pages to see them.",
                diff.rows.len() - shown
            ),
            BODY,
            false,
            MUTED,
        );
    }
}

/// Lays out the report. `text` is `None` when the text was not compared, `visual` is empty when
/// no page looks different or the look was not compared (`looked` says which).
pub fn layout(
    summary: &Summary,
    text: Option<&TextDiff>,
    looked: bool,
    visual: &[VisualPage],
) -> Report {
    let mut b = Builder::new();
    b.line("Comparison report", 20.0, true, INK);
    b.gap(6.0);
    b.line(
        &format!(
            "Original: {} ({} pages)",
            summary.original_name, summary.original_pages
        ),
        10.0,
        false,
        INK,
    );
    b.line(
        &format!(
            "Changed: {} ({} pages)",
            summary.changed_name, summary.changed_pages
        ),
        10.0,
        false,
        INK,
    );
    b.line(
        &format!("Pages compared: {}", summary.compared_pages),
        10.0,
        false,
        INK,
    );
    b.gap(6.0);
    if let Some(diff) = text {
        if diff.is_empty() {
            b.line("Text: no differences.", 10.0, true, INK);
        } else {
            b.line(
                &format!(
                    "Text: {} lines removed, {} added, {} rewritten.",
                    diff.removed, diff.added, diff.changed
                ),
                10.0,
                true,
                INK,
            );
        }
    }
    if looked {
        if visual.is_empty() {
            b.line("Look: the pages look the same.", 10.0, true, INK);
        } else {
            let worst = visual.iter().map(|v| v.percent).fold(0.0, f32::max);
            b.line(
                &format!(
                    "Look: {} pages differ, the most by {:.2}% of the page.",
                    visual.len(),
                    worst
                ),
                10.0,
                true,
                INK,
            );
            for v in visual.iter().take(MAX_LISTED_PAGES) {
                let note = if v.size_differs {
                    ", page size differs"
                } else {
                    ""
                };
                b.line(
                    &format!("   Page {}: {:.2}% changed{note}", v.page, v.percent),
                    9.0,
                    false,
                    MUTED,
                );
            }
            if visual.len() > MAX_LISTED_PAGES {
                b.line(
                    &format!("   and {} more pages", visual.len() - MAX_LISTED_PAGES),
                    9.0,
                    false,
                    MUTED,
                );
            }
        }
        b.gap(4.0);
        b.line(
            "On the page pictures, red is only in the original and blue is only in the changed file.",
            8.5,
            false,
            MUTED,
        );
    }
    if let Some(diff) = text
        && !diff.is_empty()
    {
        b.gap(12.0);
        // The side-by-side view starts on a fresh page when little room is left.
        if !b.room(80.0) {
            b.new_page();
        }
        rows(&mut b, summary, diff);
    }
    for v in visual {
        b.new_page();
        b.line(
            &format!("Page {}: {:.2}% changed", v.page, v.percent),
            12.0,
            true,
            INK,
        );
        let top = b.y + 4.0;
        let (area_w, area_h) = (PAGE.0 - 2.0 * MARGIN, PAGE.1 - top - MARGIN);
        let page = b.page;
        b.items.push(EditItem::Image(ImageItem {
            page,
            region: Builder::region(MARGIN, top, area_w, area_h),
            path: v.image.clone(),
        }));
    }
    Report {
        pages: b.page,
        items: b.items,
    }
}

#[cfg(test)]
mod tests {
    use super::super::comparediff::diff_text;
    use super::super::reportkit::printable;
    use super::*;

    fn summary() -> Summary {
        Summary {
            original_name: "old.pdf".into(),
            changed_name: "new.pdf".into(),
            original_pages: 3,
            changed_pages: 4,
            compared_pages: 4,
        }
    }

    fn lines(page: u32, texts: &[&str]) -> Vec<Line> {
        texts
            .iter()
            .map(|t| Line {
                page,
                text: (*t).to_owned(),
            })
            .collect()
    }

    fn page_of(item: &EditItem) -> u32 {
        item.page()
    }

    #[test]
    fn wrapping_breaks_at_spaces_and_cuts_long_words() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("", 5), [""]);
        assert!(
            wrap(&"word ".repeat(100), 56)
                .iter()
                .all(|l| l.chars().count() <= 56)
        );
    }

    #[test]
    fn characters_the_fonts_lack_become_question_marks() {
        assert_eq!(
            printable("caf\u{e9} \u{4e2d}\u{6587}\tx\u{7}"),
            "caf\u{e9} ?? x"
        );
    }

    #[test]
    fn a_report_without_changes_says_so() {
        let same = diff_text(&lines(1, &["a"]), &lines(1, &["a"]));
        let r = layout(&summary(), Some(&same), true, &[]);
        assert_eq!(r.pages, 1);
        let texts: Vec<String> = r
            .items
            .iter()
            .filter_map(|i| match i {
                EditItem::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts.iter().any(|t| t.contains("Text: no differences")),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("the pages look the same")),
            "{texts:?}"
        );
    }

    #[test]
    fn changes_are_shown_side_by_side_with_fills_and_page_markers() {
        let a = lines(1, &["keep", "old line"]);
        let b = lines(2, &["keep", "new line", "extra"]);
        let d = diff_text(&a, &b);
        let r = layout(&summary(), Some(&d), false, &[]);
        let mut text = Vec::new();
        let mut fills = 0;
        for item in &r.items {
            match item {
                EditItem::Text(t) => text.push((t.region.x, t.text.clone())),
                EditItem::Whiteout(_) => fills += 1,
                _ => {}
            }
        }
        assert!(
            text.iter().any(|(x, t)| *x < 0.5 && t == "old line"),
            "{text:?}"
        );
        assert!(
            text.iter().any(|(x, t)| *x > 0.5 && t == "new line"),
            "{text:?}"
        );
        assert!(text.iter().any(|(_, t)| t == "p1") && text.iter().any(|(_, t)| t == "p2"));
        // A rewritten line and an added line: left and right fills for the first, one for the
        // second.
        assert_eq!(fills, 3);
    }

    #[test]
    fn long_lists_run_over_pages_and_everything_stays_on_a_page() {
        let a: Vec<Line> = (0..400)
            .map(|i| Line {
                page: 1 + i / 40,
                text: format!("original line number {i} with some more words to fill it"),
            })
            .collect();
        let b: Vec<Line> = (0..400)
            .map(|i| Line {
                page: 1 + i / 40,
                text: format!("changed line number {i} with some more words to fill it"),
            })
            .collect();
        let d = diff_text(&a, &b);
        let r = layout(&summary(), Some(&d), false, &[]);
        assert!(r.pages > 5, "{}", r.pages);
        for item in &r.items {
            let p = page_of(item);
            assert!((1..=r.pages).contains(&p));
            if let EditItem::Text(t) = item {
                assert!(t.region.y >= 0.0 && t.region.y + t.region.height <= 1.0 + 1e-4);
                assert!(t.region.x >= 0.0 && t.region.x + t.region.width <= 1.0 + 1e-4);
            }
        }
    }

    #[test]
    fn rows_over_the_limit_are_counted() {
        let a: Vec<Line> = (0..MAX_ROWS + 10)
            .map(|i| Line {
                page: 1,
                text: format!("a{i}"),
            })
            .collect();
        let b: Vec<Line> = (0..MAX_ROWS + 10)
            .map(|i| Line {
                page: 1,
                text: format!("b{i}"),
            })
            .collect();
        let d = diff_text(&a, &b);
        let r = layout(&summary(), Some(&d), false, &[]);
        assert!(
            r.items
                .iter()
                .any(|i| matches!(i, EditItem::Text(t) if t.text.contains("10 more rows")))
        );
    }

    #[test]
    fn each_page_that_looks_different_gets_a_page_with_its_picture() {
        let visual = vec![
            VisualPage {
                page: 2,
                percent: 1.5,
                size_differs: false,
                image: PathBuf::from("a.png"),
            },
            VisualPage {
                page: 4,
                percent: 12.0,
                size_differs: true,
                image: PathBuf::from("b.png"),
            },
        ];
        let r = layout(&summary(), None, true, &visual);
        assert_eq!(r.pages, 3);
        let images: Vec<(u32, PathBuf)> = r
            .items
            .iter()
            .filter_map(|i| match i {
                EditItem::Image(im) => Some((im.page, im.path.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            images,
            [(2, PathBuf::from("a.png")), (3, PathBuf::from("b.png"))]
        );
        let texts: Vec<String> = r
            .items
            .iter()
            .filter_map(|i| match i {
                EditItem::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts.iter().any(|t| t.contains("Page 4: 12.00% changed")),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("page size differs")),
            "{texts:?}"
        );
    }
}
