//! Finding the places of a page that look like fields to fill in: a line after a label, an empty
//! box, a small empty square. The editor shows them as suggestions; nothing is added until the
//! person confirms.

use super::model::{PageContent, Rect, Rule, Word};
use super::words::segments;
use crate::forms::{FieldSuggestion, SuggestedKind};
use crate::tools::common::Region;

/// Most suggestions for one page.
const MAX_SUGGESTIONS: usize = 200;
/// A box whose sides are both at most this many points is a check box when it is nearly square.
const CHECKBOX_MAX: f32 = 22.0;
const CHECKBOX_MIN: f32 = 7.0;
/// Smallest and largest height of a text box, and smallest width.
const BOX_HEIGHT: (f32, f32) = (10.0, 80.0);
const BOX_MIN_WIDTH: f32 = 40.0;
/// A line this long can be written on.
const LINE_MIN_WIDTH: f32 = 50.0;
/// Height of the field made for a line, in points.
const LINE_FIELD_HEIGHT: f32 = 16.0;
/// Tolerance when matching the sides of a box, in points.
const EDGE: f32 = 2.0;

fn horizontal(rule: &Rule) -> bool {
    rule.rect.width() > rule.rect.height()
}

fn alnum(words: &[Word], inside: &Rect) -> bool {
    words.iter().any(|w| {
        w.text.chars().any(char::is_alphanumeric) && inside.contains(w.rect.mid_x(), w.rect.mid_y())
    })
}

/// The words on the same line to the left of `x`, closest last, joined.
fn label_before(words: &[Word], y_mid: f32, x: f32, size: f32) -> String {
    let mut found: Vec<&Word> = words
        .iter()
        .filter(|w| w.rect.x1 <= x + 2.0 && (w.rect.mid_y() - y_mid).abs() <= size.max(w.size))
        .filter(|w| w.text.chars().any(char::is_alphanumeric))
        .collect();
    found.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
    // The nearest run of words: stop at a wide gap.
    let mut run: Vec<&Word> = Vec::new();
    for w in found.iter().rev() {
        match run.last() {
            Some(prev) if prev.rect.x0 - w.rect.x1 > size * 1.5 => break,
            _ => run.push(w),
        }
    }
    run.reverse();
    run.iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn label_after(words: &[Word], y_mid: f32, x: f32, size: f32) -> String {
    let mut found: Vec<&Word> = words
        .iter()
        .filter(|w| w.rect.x0 >= x - 2.0 && (w.rect.mid_y() - y_mid).abs() <= size.max(w.size))
        .collect();
    found.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
    let mut run: Vec<&Word> = Vec::new();
    for w in found {
        match run.last() {
            Some(prev) if w.rect.x0 - prev.rect.x1 > size * 1.5 => break,
            _ => run.push(w),
        }
    }
    run.iter()
        .take(8)
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A field name made of a label: letters and digits, words joined by underscores.
fn name_of(label: &str, fallback: usize) -> String {
    let mut name = String::new();
    for ch in label.chars() {
        if ch.is_alphanumeric() {
            name.push(ch);
        } else if !name.ends_with('_') && !name.is_empty() {
            name.push('_');
        }
    }
    let name: String = name.trim_matches('_').chars().take(40).collect();
    if name.is_empty() {
        format!("field_{fallback}")
    } else {
        name
    }
}

struct Frame {
    rect: Rect,
    used: [usize; 4],
}

/// Boxes drawn with four lines: two horizontal ones of the same extent, joined by two vertical
/// ones at their ends.
fn frames(rules: &[Rule]) -> Vec<Frame> {
    let mut out = Vec::new();
    let horizontals: Vec<usize> = (0..rules.len())
        .filter(|i| horizontal(&rules[*i]))
        .collect();
    let verticals: Vec<usize> = (0..rules.len())
        .filter(|i| !horizontal(&rules[*i]))
        .collect();
    for (a, &top) in horizontals.iter().enumerate() {
        for &bottom in &horizontals[a + 1..] {
            let (mut t, mut b) = (&rules[top].rect, &rules[bottom].rect);
            let (mut ti, mut bi) = (top, bottom);
            if t.mid_y() > b.mid_y() {
                std::mem::swap(&mut t, &mut b);
                std::mem::swap(&mut ti, &mut bi);
            }
            let height = b.mid_y() - t.mid_y();
            if (t.x0 - b.x0).abs() > EDGE
                || (t.x1 - b.x1).abs() > EDGE
                || height < CHECKBOX_MIN
                || height > BOX_HEIGHT.1
            {
                continue;
            }
            let side = |x: f32| {
                verticals.iter().copied().find(|&v| {
                    let r = &rules[v].rect;
                    (r.mid_x() - x).abs() <= EDGE
                        && r.y0 <= t.mid_y() + EDGE
                        && r.y1 >= b.mid_y() - EDGE
                })
            };
            if let (Some(l), Some(r)) = (side(t.x0), side(t.x1)) {
                out.push(Frame {
                    rect: Rect::new(t.x0, t.mid_y(), t.x1, b.mid_y()),
                    used: [ti, bi, l, r],
                });
            }
        }
    }
    out
}

fn region_of(rect: &Rect, page: &PageContent) -> Option<Region> {
    let (w, h) = (page.width.max(1.0), page.height.max(1.0));
    let x = (rect.x0 / w).clamp(0.0, 1.0);
    let y = (rect.y0 / h).clamp(0.0, 1.0);
    let width = ((rect.x1 - rect.x0) / w).min(1.0 - x);
    let height = ((rect.y1 - rect.y0) / h).min(1.0 - y);
    (width > 0.0 && height > 0.0).then_some(Region {
        x,
        y,
        width,
        height,
    })
}

fn overlap(a: &Rect, b: &Rect) -> f32 {
    let w = (a.x1.min(b.x1) - a.x0.max(b.x0)).max(0.0);
    let h = (a.y1.min(b.y1) - a.y0.max(b.y0)).max(0.0);
    let inter = w * h;
    let union = a.width() * a.height() + b.width() * b.height() - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

/// Finds the places on a page that look like fields.
pub fn suggest_fields(page: &PageContent) -> Vec<FieldSuggestion> {
    let mut found: Vec<(Rect, SuggestedKind, String)> = Vec::new();
    let mut used = vec![false; page.rules.len()];

    // Boxes: small empty squares are check boxes, wider empty boxes are text fields.
    for frame in frames(&page.rules) {
        // The sides of any box are not lines to write on, whether or not the box is empty.
        for i in frame.used {
            used[i] = true;
        }
        let (w, h) = (frame.rect.width(), frame.rect.height());
        let inside = Rect::new(
            frame.rect.x0 + 1.0,
            frame.rect.y0 + 1.0,
            frame.rect.x1 - 1.0,
            frame.rect.y1 - 1.0,
        );
        if alnum(&page.words, &inside) {
            continue;
        }
        let small =
            w <= CHECKBOX_MAX && h <= CHECKBOX_MAX && (w - h).abs() <= 3.0 && w >= CHECKBOX_MIN;
        let size = h.max(8.0);
        if small {
            let label = label_after(&page.words, frame.rect.mid_y(), frame.rect.x1, size);
            found.push((frame.rect, SuggestedKind::Checkbox, label));
        } else if w >= BOX_MIN_WIDTH && (BOX_HEIGHT.0..=BOX_HEIGHT.1).contains(&h) {
            let label = label_before(&page.words, frame.rect.mid_y(), frame.rect.x0, size);
            found.push((frame.rect, SuggestedKind::Text, label));
        }
    }

    // Lines to write on: a long thin line that is not the side of a box, with nothing written
    // just above it.
    for (i, rule) in page.rules.iter().enumerate() {
        if used[i] || !horizontal(rule) || rule.rect.width() < LINE_MIN_WIDTH {
            continue;
        }
        let line = rule.rect;
        let field = Rect::new(
            line.x0,
            line.mid_y() - LINE_FIELD_HEIGHT,
            line.x1,
            line.mid_y(),
        );
        if alnum(&page.words, &field) {
            continue;
        }
        let label = label_before(&page.words, field.mid_y(), line.x0, LINE_FIELD_HEIGHT);
        found.push((field, SuggestedKind::Text, label));
    }

    // Runs of underscores typed as text: "Name: ________".
    for seg in segments(&page.words) {
        for w in &seg.words {
            let marks = w.text.chars().filter(|c| *c == '_').count();
            if marks >= 3 && marks * 10 >= w.text.chars().count() * 8 {
                let field = Rect::new(w.rect.x0, w.rect.y0 - 2.0, w.rect.x1, w.rect.y1 + 2.0);
                let label = label_before(&page.words, w.rect.mid_y(), w.rect.x0, w.size.max(8.0));
                found.push((field, SuggestedKind::Text, label));
            }
        }
    }

    // Keep the first of any two suggestions that cover the same place.
    let mut kept: Vec<(Rect, SuggestedKind, String)> = Vec::new();
    for item in found {
        if kept.iter().all(|k| overlap(&k.0, &item.0) < 0.5) {
            kept.push(item);
        }
    }
    kept.sort_by(|a, b| a.0.y0.total_cmp(&b.0.y0).then(a.0.x0.total_cmp(&b.0.x0)));
    kept.truncate(MAX_SUGGESTIONS);

    let mut names: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for (n, (rect, kind, label)) in kept.into_iter().enumerate() {
        let Some(region) = region_of(&rect, page) else {
            continue;
        };
        let label = label
            .trim()
            .trim_end_matches([':', '.', '_'])
            .trim()
            .to_owned();
        let mut name = name_of(&label, n + 1);
        let base = name.clone();
        let mut k = 2;
        while names.contains(&name) {
            name = format!("{base}_{k}");
            k += 1;
        }
        names.push(name.clone());
        out.push(FieldSuggestion {
            page: page.number,
            region,
            kind,
            label,
            name,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, x: f32, y: f32) -> Word {
        Word {
            text: text.to_owned(),
            rect: Rect::new(x, y, x + 6.0 * text.chars().count() as f32, y + 11.0),
            size: 11.0,
            bold: false,
            italic: false,
        }
    }

    fn bar(x0: f32, y0: f32, x1: f32, y1: f32) -> Rule {
        Rule {
            rect: Rect::new(x0, y0, x1, y1),
        }
    }

    /// The four lines of a box.
    fn outline(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Rule> {
        vec![
            bar(x0, y0 - 0.25, x1, y0 + 0.25),
            bar(x0, y1 - 0.25, x1, y1 + 0.25),
            bar(x0 - 0.25, y0, x0 + 0.25, y1),
            bar(x1 - 0.25, y0, x1 + 0.25, y1),
        ]
    }

    fn page(words: Vec<Word>, rules: Vec<Rule>) -> PageContent {
        PageContent {
            number: 2,
            width: 612.0,
            height: 792.0,
            words,
            rules,
            pictures: vec![],
        }
    }

    #[test]
    fn a_line_after_a_label_is_a_text_field_named_after_the_label() {
        let p = page(
            vec![word("Full", 72.0, 100.0), word("name:", 100.0, 100.0)],
            vec![bar(140.0, 111.0, 340.0, 112.0)],
        );
        let found = suggest_fields(&p);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, SuggestedKind::Text);
        assert_eq!(found[0].label, "Full name");
        assert_eq!(found[0].name, "Full_name");
        assert_eq!(found[0].page, 2);
        // The field sits on the line.
        let r = found[0].region;
        assert!(
            (r.x * 612.0 - 140.0).abs() < 0.5 && ((r.y + r.height) * 792.0 - 111.5).abs() < 1.0
        );
    }

    #[test]
    fn underlined_text_is_not_a_field() {
        let p = page(
            vec![
                word("Underlined", 140.0, 100.0),
                word("heading", 210.0, 100.0),
            ],
            vec![bar(140.0, 112.0, 340.0, 113.0)],
        );
        assert!(suggest_fields(&p).is_empty());
    }

    #[test]
    fn small_empty_squares_are_check_boxes_with_the_words_after_them() {
        let mut rules = outline(72.0, 200.0, 84.0, 212.0);
        rules.extend(outline(72.0, 230.0, 84.0, 242.0));
        let p = page(
            vec![
                word("Yes", 92.0, 200.0),
                word("please", 116.0, 200.0),
                word("No", 92.0, 230.0),
            ],
            rules,
        );
        let found = suggest_fields(&p);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().all(|f| f.kind == SuggestedKind::Checkbox));
        assert_eq!(found[0].label, "Yes please");
        assert_eq!(found[1].name, "No");
    }

    #[test]
    fn wide_empty_boxes_are_text_fields_and_boxes_with_text_are_not() {
        let mut rules = outline(150.0, 300.0, 400.0, 322.0);
        rules.extend(outline(150.0, 340.0, 400.0, 362.0));
        let p = page(
            vec![
                word("Address", 72.0, 305.0),
                word("Already", 160.0, 345.0),
                word("filled", 210.0, 345.0),
            ],
            rules,
        );
        let found = suggest_fields(&p);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, SuggestedKind::Text);
        assert_eq!(found[0].label, "Address");
    }

    #[test]
    fn typed_underscores_make_a_field_and_names_stay_unique() {
        let p = page(
            vec![
                word("Date:", 72.0, 100.0),
                word("__________", 110.0, 100.0),
                word("Date:", 72.0, 140.0),
                word("__________", 110.0, 140.0),
                word("ordinary", 72.0, 180.0),
                word("snake_case_word", 130.0, 180.0),
            ],
            vec![],
        );
        let found = suggest_fields(&p);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].name, "Date");
        assert_eq!(found[1].name, "Date_2");
    }

    #[test]
    fn a_page_without_candidates_gives_nothing_and_short_lines_are_ignored() {
        assert!(suggest_fields(&page(vec![word("text", 72.0, 100.0)], vec![])).is_empty());
        assert!(suggest_fields(&page(vec![], vec![bar(72.0, 100.0, 100.0, 100.5)])).is_empty());
    }
}
