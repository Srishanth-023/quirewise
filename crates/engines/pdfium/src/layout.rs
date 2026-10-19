//! Layout-preserving text: words are put in the character column their position on the page
//! says, so columns and tables keep their shape in a plain text file.

use application::AppError;
use application::Password;
use domain::PageIndex;
use pdfium_render::prelude::*;
use std::path::Path;

use crate::PdfiumEngine;
use crate::errors::map_error;
use crate::out_of_range;

/// A character with the position PDFium reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    /// The character.
    pub ch: char,
    /// Left edge of the character's origin.
    pub x: f32,
    /// Baseline height.
    pub y: f32,
    /// Font size in points.
    pub size: f32,
}

/// Most blank lines kept between two lines of text.
const MAX_BLANK_LINES: usize = 3;

fn median(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    values
        .get(values.len().saturating_sub(1) / 2)
        .copied()
        .unwrap_or(0.0)
}

/// A run of characters that sit together.
struct Word {
    x: f32,
    text: String,
}

/// Splits one line, already ordered by `x`, into words: a space, or a gap wider than a letter,
/// ends a word.
fn words(line: &[Placed]) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::new();
    let mut previous: Option<&Placed> = None;
    for g in line {
        if g.ch.is_whitespace() {
            previous = None;
            continue;
        }
        let joins = previous.is_some_and(|p| g.x - p.x <= p.size.max(g.size) * 1.1);
        match (joins, out.last_mut()) {
            (true, Some(word)) => word.text.push(g.ch),
            _ => out.push(Word {
                x: g.x,
                text: g.ch.to_string(),
            }),
        }
        previous = Some(g);
    }
    out
}

/// Lays characters out as lines of text with the spacing of the page.
pub fn layout_text(glyphs: &[Placed]) -> String {
    let visible: Vec<Placed> = glyphs
        .iter()
        .copied()
        .filter(|g| !g.ch.is_control())
        .collect();
    if visible.iter().all(|g| g.ch.is_whitespace()) {
        return String::new();
    }
    let mut sizes: Vec<f32> = visible
        .iter()
        .filter(|g| !g.ch.is_whitespace())
        .map(|g| g.size)
        .collect();
    // One column is about the width of an average letter.
    let column = (median(&mut sizes) * 0.5).max(1.0);
    let left = visible
        .iter()
        .filter(|g| !g.ch.is_whitespace())
        .map(|g| g.x)
        .fold(f32::INFINITY, f32::min);

    // Top to bottom, then every line left to right.
    let mut sorted = visible;
    sorted.sort_by(|a, b| b.y.total_cmp(&a.y));
    let mut lines: Vec<(f32, Vec<Placed>)> = Vec::new();
    for g in sorted {
        match lines.last_mut() {
            Some((y, line)) if (*y - g.y).abs() <= g.size.max(1.0) * 0.5 => line.push(g),
            _ => lines.push((g.y, vec![g])),
        }
    }
    for (_, line) in &mut lines {
        line.sort_by(|a, b| a.x.total_cmp(&b.x));
    }

    let mut pitches: Vec<f32> = lines.windows(2).map(|w| w[0].0 - w[1].0).collect();
    let pitch = median(&mut pitches);
    let mut text = String::new();
    let mut last_y: Option<f32> = None;
    for (y, line) in &lines {
        let mut row = String::new();
        for word in words(line) {
            let wanted = ((word.x - left) / column).round().max(0.0) as usize;
            let current = row.chars().count();
            let at = if current == 0 {
                wanted
            } else {
                wanted.max(current + 1)
            };
            row.extend(std::iter::repeat_n(' ', at - current));
            row.push_str(&word.text);
        }
        if row.is_empty() {
            continue;
        }
        if let Some(previous) = last_y {
            let blanks = if pitch > 0.0 {
                (((previous - y) / pitch).round() as usize).saturating_sub(1)
            } else {
                0
            };
            for _ in 0..blanks.min(MAX_BLANK_LINES) {
                text.push('\n');
            }
            text.push('\n');
        }
        text.push_str(&row);
        last_y = Some(*y);
    }
    text
}

impl PdfiumEngine {
    /// The characters of one page with their positions.
    pub(crate) fn placed_chars(
        &self,
        path: &Path,
        password: Option<&Password>,
        page: PageIndex,
    ) -> Result<Vec<Placed>, AppError> {
        let doc = self.open(path, password)?;
        let index = i32::try_from(page.get()).map_err(|_| out_of_range(page))?;
        let page = doc
            .pages()
            .get(index)
            .map_err(|e| map_error(&e, password.is_some()))?;
        let text = page.text().map_err(|e| map_error(&e, password.is_some()))?;
        Ok(text
            .chars()
            .iter()
            .filter_map(|c| {
                let ch = c.unicode_char()?;
                let x = c.origin_x().ok()?.value;
                let y = c.origin_y().ok()?.value;
                Some(Placed {
                    ch,
                    x,
                    y,
                    size: c.scaled_font_size().value,
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(x: f32, y: f32, text: &str) -> Vec<Placed> {
        // 6 points per letter at size 12, like a typical proportional font.
        text.chars()
            .enumerate()
            .map(|(i, ch)| Placed {
                ch,
                x: x + 6.0 * i as f32,
                y,
                size: 12.0,
            })
            .collect()
    }

    #[test]
    fn words_keep_their_columns_and_lines_stay_in_order() {
        let mut glyphs = run(50.0, 700.0, "Name");
        glyphs.extend(run(200.0, 700.0, "Total"));
        glyphs.extend(run(50.0, 686.0, "Pens"));
        glyphs.extend(run(200.0, 686.0, "12"));
        let text = layout_text(&glyphs);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("Name"));
        assert!(lines[1].starts_with("Pens"));
        // Both second words start in the same column.
        assert_eq!(lines[0].find("Total"), lines[1].find("12"));
        assert!(lines[0].find("Total").unwrap() > 20);
    }

    #[test]
    fn spaces_split_words_and_a_big_vertical_gap_leaves_a_blank_line() {
        let mut glyphs = run(50.0, 700.0, "one two");
        glyphs.extend(run(50.0, 686.0, "three"));
        glyphs.extend(run(50.0, 658.0, "four"));
        let text = layout_text(&glyphs);
        assert_eq!(text, "one two\nthree\n\nfour");
    }

    #[test]
    fn the_left_margin_of_the_page_is_not_kept() {
        let text = layout_text(&run(300.0, 700.0, "Right"));
        assert_eq!(text, "Right");
    }

    #[test]
    fn a_page_without_text_gives_nothing() {
        assert_eq!(layout_text(&[]), "");
        assert_eq!(layout_text(&run(10.0, 10.0, "   ")), "");
    }

    #[test]
    fn control_characters_are_dropped() {
        let mut glyphs = run(50.0, 700.0, "ab");
        glyphs.push(Placed {
            ch: '\r',
            x: 62.0,
            y: 700.0,
            size: 12.0,
        });
        assert_eq!(layout_text(&glyphs), "ab");
    }
}
