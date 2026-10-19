//! Lines of text with their font sizes, the raw material for heading detection.

use application::AppError;
use application::Password;
use application::ports::HeadingFinder;
use domain::PageIndex;
use domain::tools::structure::TextLine;
use pdfium_render::prelude::*;
use std::path::Path;

use crate::PdfiumEngine;
use crate::errors::map_error;

/// Most pages read for headings: a bound for books of thousands of pages.
const MAX_PAGES: i32 = 5000;

/// One character with the facts line grouping needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    /// The character.
    pub ch: char,
    /// Font size in points.
    pub size: f32,
    /// Baseline height on the page.
    pub y: f32,
}

/// Groups characters, given in reading order, into lines. A new line starts at a line break,
/// when the size changes noticeably, or when the baseline moves by more than half the size.
pub fn group_lines(glyphs: &[Glyph]) -> Vec<(String, f32)> {
    let mut lines: Vec<(String, f32)> = Vec::new();
    let mut text = String::new();
    let mut size = 0.0f32;
    let mut baseline = 0.0f32;
    let flush = |text: &mut String, size: f32, lines: &mut Vec<(String, f32)>| {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            lines.push((trimmed.to_owned(), size));
        }
        text.clear();
    };
    for g in glyphs {
        if g.ch == '\n' || g.ch == '\r' {
            flush(&mut text, size, &mut lines);
            continue;
        }
        if g.ch.is_whitespace() && text.is_empty() {
            continue;
        }
        let spaced = g.ch.is_whitespace();
        if !text.is_empty()
            && !spaced
            && ((g.size - size).abs() > 0.5 || (g.y - baseline).abs() > size.max(1.0) * 0.5)
        {
            flush(&mut text, size, &mut lines);
        }
        if text.is_empty() {
            size = g.size;
            baseline = g.y;
        }
        text.push(g.ch);
    }
    flush(&mut text, size, &mut lines);
    lines
}

impl HeadingFinder for PdfiumEngine {
    fn text_lines(
        &self,
        path: &Path,
        password: Option<&Password>,
    ) -> Result<Vec<TextLine>, AppError> {
        let doc = self.open(path, password)?;
        let mut out = Vec::new();
        for (index, page) in doc.pages().iter().enumerate() {
            if i32::try_from(index).unwrap_or(i32::MAX) >= MAX_PAGES {
                break;
            }
            let text = page.text().map_err(|e| map_error(&e, password.is_some()))?;
            let glyphs: Vec<Glyph> = text
                .chars()
                .iter()
                .filter_map(|c| {
                    let ch = c.unicode_char()?;
                    let size = c.scaled_font_size().value;
                    let y = c.origin_y().map_or(0.0, |y| y.value);
                    Some(Glyph { ch, size, y })
                })
                .collect();
            let page_index = PageIndex::new(u32::try_from(index).unwrap_or(u32::MAX));
            for (line, size) in group_lines(&glyphs) {
                out.push(TextLine {
                    page: page_index,
                    text: line,
                    size_pt: size,
                });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyphs(text: &str, size: f32, y: f32) -> Vec<Glyph> {
        text.chars().map(|ch| Glyph { ch, size, y }).collect()
    }

    #[test]
    fn a_run_of_characters_on_one_baseline_is_one_line() {
        let lines = group_lines(&glyphs("Hello world", 12.0, 700.0));
        assert_eq!(lines, vec![("Hello world".to_owned(), 12.0)]);
    }

    #[test]
    fn line_breaks_baseline_moves_and_size_changes_start_new_lines() {
        let mut all = glyphs("Title", 24.0, 700.0);
        all.extend(glyphs("Body one", 11.0, 700.0));
        all.extend(glyphs("\r\n", 11.0, 700.0));
        all.extend(glyphs("Body two", 11.0, 686.0));
        all.extend(glyphs("Body three", 11.0, 672.0));
        let lines = group_lines(&all);
        assert_eq!(
            lines,
            vec![
                ("Title".to_owned(), 24.0),
                ("Body one".to_owned(), 11.0),
                ("Body two".to_owned(), 11.0),
                ("Body three".to_owned(), 11.0),
            ]
        );
    }

    #[test]
    fn spaces_never_split_a_line_and_blank_text_makes_no_line() {
        let mut all = glyphs("A ", 12.0, 100.0);
        all.push(Glyph {
            ch: ' ',
            size: 30.0,
            y: 400.0,
        });
        all.extend(glyphs("B", 12.0, 100.0));
        assert_eq!(group_lines(&all), vec![("A  B".to_owned(), 12.0)]);
        assert!(group_lines(&glyphs("   \r\n  ", 12.0, 1.0)).is_empty());
        assert!(group_lines(&[]).is_empty());
    }

    #[test]
    fn small_size_differences_do_not_split_a_line() {
        let mut all = glyphs("Mostly ", 11.0, 50.0);
        all.extend(glyphs("same", 11.3, 50.0));
        assert_eq!(group_lines(&all).len(), 1);
    }
}
