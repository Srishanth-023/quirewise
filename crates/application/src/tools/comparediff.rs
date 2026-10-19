//! What Compare finds: the text changes between two documents as rows for a side-by-side view,
//! and the pixel differences between two renderings of a page.

use similar::{Algorithm, DiffOp, capture_diff_slices};

use crate::ports::RenderedImage;

/// One line of text and the page it is on, from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub page: u32,
    pub text: String,
}

/// How a row of the side-by-side view reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// Unchanged text shown for context.
    Context,
    /// A line that is only in the original.
    Removed,
    /// A line that is only in the changed document.
    Added,
    /// A line of the original that became a line of the changed document.
    Changed,
    /// Unchanged lines that are left out.
    Gap,
}

/// One row: what is on the left (original) and on the right (changed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: RowKind,
    pub left: Option<Line>,
    pub right: Option<Line>,
}

/// The text changes of two documents.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextDiff {
    /// Changed rows with a line of context on each side.
    pub rows: Vec<Row>,
    /// Lines only in the original.
    pub removed: usize,
    /// Lines only in the changed document.
    pub added: usize,
    /// Lines that were rewritten.
    pub changed: usize,
}

impl TextDiff {
    /// True when the texts are the same.
    pub fn is_empty(&self) -> bool {
        self.removed == 0 && self.added == 0 && self.changed == 0
    }
}

/// Lines of text for comparing: with `ignore_whitespace`, runs of white space become one space
/// and blank lines go.
pub fn lines_of(page: u32, text: &str, ignore_whitespace: bool) -> Vec<Line> {
    text.lines()
        .map(|line| {
            if ignore_whitespace {
                line.split_whitespace().collect::<Vec<_>>().join(" ")
            } else {
                line.trim_end().to_owned()
            }
        })
        .filter(|line| !ignore_whitespace || !line.is_empty())
        .map(|text| Line { page, text })
        .collect()
}

/// Compares two documents line by line.
pub fn diff_text(original: &[Line], changed: &[Line]) -> TextDiff {
    let a: Vec<&str> = original.iter().map(|l| l.text.as_str()).collect();
    let b: Vec<&str> = changed.iter().map(|l| l.text.as_str()).collect();
    let ops = capture_diff_slices(Algorithm::Myers, &a, &b);

    let mut out = TextDiff::default();
    let last = ops.len().saturating_sub(1);
    for (n, op) in ops.iter().enumerate() {
        match *op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                // A line of context next to a change; the rest of a long run is a gap.
                let before = n > 0;
                let after = n < last;
                let keep_head = usize::from(before);
                let keep_tail = usize::from(after);
                if len <= keep_head + keep_tail + 1 {
                    for i in 0..len {
                        push_context(&mut out, original, changed, old_index + i, new_index + i);
                    }
                } else {
                    for i in 0..keep_head {
                        push_context(&mut out, original, changed, old_index + i, new_index + i);
                    }
                    out.rows.push(Row {
                        kind: RowKind::Gap,
                        left: None,
                        right: None,
                    });
                    for i in len - keep_tail..len {
                        push_context(&mut out, original, changed, old_index + i, new_index + i);
                    }
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                for i in 0..old_len {
                    out.removed += 1;
                    out.rows.push(Row {
                        kind: RowKind::Removed,
                        left: original.get(old_index + i).cloned(),
                        right: None,
                    });
                }
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                for i in 0..new_len {
                    out.added += 1;
                    out.rows.push(Row {
                        kind: RowKind::Added,
                        left: None,
                        right: changed.get(new_index + i).cloned(),
                    });
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                for i in 0..old_len.max(new_len) {
                    let left = (i < old_len)
                        .then(|| original.get(old_index + i).cloned())
                        .flatten();
                    let right = (i < new_len)
                        .then(|| changed.get(new_index + i).cloned())
                        .flatten();
                    let kind = match (&left, &right) {
                        (Some(_), Some(_)) => {
                            out.changed += 1;
                            RowKind::Changed
                        }
                        (Some(_), None) => {
                            out.removed += 1;
                            RowKind::Removed
                        }
                        _ => {
                            out.added += 1;
                            RowKind::Added
                        }
                    };
                    out.rows.push(Row { kind, left, right });
                }
            }
        }
    }
    if out.is_empty() {
        out.rows.clear();
    }
    out
}

fn push_context(out: &mut TextDiff, original: &[Line], changed: &[Line], old: usize, new: usize) {
    out.rows.push(Row {
        kind: RowKind::Context,
        left: original.get(old).cloned(),
        right: changed.get(new).cloned(),
    });
}

// ----- pixels --------------------------------------------------------------------------------

/// The difference between two renderings of a page.
pub struct PixelDiff {
    /// Pixels that changed.
    pub changed: u64,
    /// Pixels compared.
    pub total: u64,
    /// The pages are not the same size.
    pub size_differs: bool,
    /// The pages as one picture: unchanged parts faded, ink only in the original red, ink only in
    /// the changed page blue.
    pub overlay: RenderedImage,
}

impl PixelDiff {
    /// The share of pixels that changed, in percent.
    pub fn percent(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.changed as f64 / self.total as f64 * 100.0) as f32
        }
    }
}

const WHITE: [u8; 3] = [255, 255, 255];
const ONLY_IN_ORIGINAL: [u8; 3] = [211, 47, 47];
const ONLY_IN_CHANGED: [u8; 3] = [21, 101, 192];

fn pixel(image: Option<&RenderedImage>, x: usize, y: usize) -> [u8; 3] {
    let Some(image) = image else {
        return WHITE;
    };
    if x >= image.width as usize || y >= image.height as usize {
        return WHITE;
    }
    let at = (y * image.width as usize + x) * 4;
    [image.rgba[at], image.rgba[at + 1], image.rgba[at + 2]]
}

fn brightness(p: [u8; 3]) -> u32 {
    u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114
}

/// Compares two renderings. A page that is missing counts as blank paper. The pages are put on
/// one canvas by their top left corners. A pixel has changed when any color channel differs by
/// more than `tolerance`, 0 to 255.
pub fn diff_pixels(
    original: Option<&RenderedImage>,
    changed: Option<&RenderedImage>,
    tolerance: u8,
) -> PixelDiff {
    let size = |i: Option<&RenderedImage>| i.map_or((0, 0), |i| (i.width, i.height));
    let (aw, ah) = size(original);
    let (bw, bh) = size(changed);
    let (w, h) = (aw.max(bw) as usize, ah.max(bh) as usize);
    let mut rgba = vec![255u8; w * h * 4];
    let mut count = 0u64;
    for y in 0..h {
        for x in 0..w {
            let a = pixel(original, x, y);
            let b = pixel(changed, x, y);
            let differs = a.iter().zip(&b).any(|(p, q)| p.abs_diff(*q) > tolerance);
            let shown = if differs {
                count += 1;
                if brightness(a) <= brightness(b) {
                    ONLY_IN_ORIGINAL
                } else {
                    ONLY_IN_CHANGED
                }
            } else {
                // The page, faded to a quarter of its strength.
                let base = if original.is_some() { a } else { b };
                [0, 1, 2].map(|i| (255 - (255 - u16::from(base[i])) / 4) as u8)
            };
            let at = (y * w + x) * 4;
            rgba[at..at + 3].copy_from_slice(&shown);
        }
    }
    PixelDiff {
        changed: count,
        total: (w * h) as u64,
        size_differs: (aw, ah) != (bw, bh) && original.is_some() && changed.is_some(),
        overlay: RenderedImage {
            width: w as u32,
            height: h as u32,
            rgba,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(page: u32, text: &str) -> Vec<Line> {
        lines_of(page, text, true)
    }

    fn texts(rows: &[Row]) -> Vec<(RowKind, String, String)> {
        rows.iter()
            .map(|r| {
                (
                    r.kind,
                    r.left.as_ref().map(|l| l.text.clone()).unwrap_or_default(),
                    r.right.as_ref().map(|l| l.text.clone()).unwrap_or_default(),
                )
            })
            .collect()
    }

    #[test]
    fn whitespace_is_ignored_by_default_and_kept_when_asked() {
        let a = lines_of(1, "a  b \n\n  c\td\n", true);
        assert_eq!(
            a.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            ["a b", "c d"]
        );
        let b = lines_of(1, "a  b \n\nc\n", false);
        assert_eq!(
            b.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            ["a  b", "", "c"]
        );
    }

    #[test]
    fn equal_texts_have_no_rows() {
        let a = lines(1, "one\ntwo\nthree");
        let d = diff_text(&a, &a);
        assert!(d.is_empty() && d.rows.is_empty());
    }

    #[test]
    fn additions_removals_and_rewrites_are_told_apart() {
        let a = lines(1, "keep\nold line\ngone\nkeep too");
        let b = lines(1, "keep\nnew line\nkeep too\nbrand new");
        let d = diff_text(&a, &b);
        assert_eq!((d.changed, d.removed, d.added), (1, 1, 1));
        let rows = texts(&d.rows);
        assert!(
            rows.contains(&(RowKind::Changed, "old line".into(), "new line".into())),
            "{rows:?}"
        );
        assert!(
            rows.contains(&(RowKind::Removed, "gone".into(), String::new())),
            "{rows:?}"
        );
        assert!(
            rows.contains(&(RowKind::Added, String::new(), "brand new".into())),
            "{rows:?}"
        );
    }

    #[test]
    fn a_long_unchanged_stretch_is_cut_to_a_line_of_context_each_side() {
        let a = lines(1, "first\nu1\nu2\nu3\nu4\nu5\nlast");
        let b = lines(1, "FIRST\nu1\nu2\nu3\nu4\nu5\nLAST");
        let d = diff_text(&a, &b);
        let kinds: Vec<RowKind> = d.rows.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [
                RowKind::Changed,
                RowKind::Context,
                RowKind::Gap,
                RowKind::Context,
                RowKind::Changed
            ]
        );
        assert_eq!(d.rows[1].left.as_ref().unwrap().text, "u1");
        assert_eq!(d.rows[3].left.as_ref().unwrap().text, "u5");
    }

    #[test]
    fn lines_remember_their_pages() {
        let mut a = lines(1, "a\nb");
        a.extend(lines(2, "c"));
        let mut b = lines(1, "a\nb");
        b.extend(lines(3, "C"));
        let d = diff_text(&a, &b);
        let row = d.rows.iter().find(|r| r.kind == RowKind::Changed).unwrap();
        assert_eq!(row.left.as_ref().unwrap().page, 2);
        assert_eq!(row.right.as_ref().unwrap().page, 3);
    }

    fn page(color: [u8; 3], width: u32, height: u32) -> RenderedImage {
        let mut rgba = Vec::new();
        for _ in 0..width * height {
            rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
        }
        RenderedImage {
            width,
            height,
            rgba,
        }
    }

    fn set(image: &mut RenderedImage, x: usize, y: usize, color: [u8; 3]) {
        let at = (y * image.width as usize + x) * 4;
        image.rgba[at..at + 3].copy_from_slice(&color);
    }

    #[test]
    fn identical_pages_have_no_changes_and_a_faded_overlay() {
        let a = page([0, 0, 0], 4, 4);
        let d = diff_pixels(Some(&a), Some(&a), 5);
        assert_eq!((d.changed, d.total), (0, 16));
        assert_eq!(d.percent(), 0.0);
        // Black faded to a quarter is a dark gray's opposite: 255 - 255 / 4.
        assert_eq!(d.overlay.rgba[..3], [192, 192, 192]);
    }

    #[test]
    fn ink_only_in_one_page_is_red_or_blue() {
        let mut a = page([255, 255, 255], 4, 4);
        let mut b = page([255, 255, 255], 4, 4);
        set(&mut a, 1, 1, [0, 0, 0]); // only in the original
        set(&mut b, 2, 2, [0, 0, 0]); // only in the changed page
        let d = diff_pixels(Some(&a), Some(&b), 5);
        assert_eq!(d.changed, 2);
        assert!((d.percent() - 12.5).abs() < 1e-4);
        let at = |x: usize, y: usize| d.overlay.rgba[(y * 4 + x) * 4..(y * 4 + x) * 4 + 3].to_vec();
        assert_eq!(at(1, 1), ONLY_IN_ORIGINAL);
        assert_eq!(at(2, 2), ONLY_IN_CHANGED);
        assert_eq!(at(0, 0), [255, 255, 255]);
    }

    #[test]
    fn the_tolerance_decides_what_is_a_change() {
        let a = page([100, 100, 100], 2, 2);
        let b = page([104, 100, 100], 2, 2);
        assert_eq!(diff_pixels(Some(&a), Some(&b), 5).changed, 0);
        assert_eq!(diff_pixels(Some(&a), Some(&b), 3).changed, 4);
        assert_eq!(diff_pixels(Some(&a), Some(&b), 0).changed, 4);
    }

    #[test]
    fn a_missing_page_counts_as_blank_paper_and_sizes_may_differ() {
        let mut a = page([255, 255, 255], 4, 4);
        set(&mut a, 0, 0, [0, 0, 0]);
        let d = diff_pixels(Some(&a), None, 5);
        assert_eq!(d.changed, 1);
        assert!(!d.size_differs);
        let small = page([255, 255, 255], 2, 2);
        let d = diff_pixels(Some(&a), Some(&small), 5);
        assert!(d.size_differs);
        assert_eq!((d.overlay.width, d.overlay.height), (4, 4));
        // The part the smaller page does not cover is blank paper, which is not a change.
        assert_eq!(d.changed, 1);
    }
}
