//! Characters into words, and words into lines and line segments.

use super::model::{CharBox, Rect, Word};

/// Gap, as a fraction of the font size, above which two characters are two words.
const WORD_GAP: f32 = 0.28;

/// Groups characters, given in content order, into words. A space, a line change or a gap wider
/// than a word space ends a word.
pub fn words_from_chars(chars: &[CharBox]) -> Vec<Word> {
    let mut words: Vec<Word> = Vec::new();
    let mut current: Option<(Word, u32, u32, u32)> = None; // word, characters, bold, italic
    let mut previous: Option<&CharBox> = None;
    let flush = |current: &mut Option<(Word, u32, u32, u32)>, words: &mut Vec<Word>| {
        if let Some((mut word, count, bold, italic)) = current.take() {
            word.bold = bold * 2 > count;
            word.italic = italic * 2 > count;
            words.push(word);
        }
    };
    for c in chars {
        if c.ch.is_whitespace() || c.ch.is_control() {
            flush(&mut current, &mut words);
            previous = None;
            continue;
        }
        if let Some(p) = previous {
            let size = c.size.max(p.size).max(1.0);
            let same_line = (c.rect.mid_y() - p.rect.mid_y()).abs() <= 0.5 * size;
            let gap = c.rect.x0 - p.rect.x1;
            if !same_line || gap > WORD_GAP * size || gap < -size {
                flush(&mut current, &mut words);
            }
        }
        match &mut current {
            Some((word, count, bold, italic)) => {
                word.text.push(c.ch);
                word.rect = word.rect.union(&c.rect);
                word.size = word.size.max(c.size);
                *count += 1;
                *bold += u32::from(c.bold);
                *italic += u32::from(c.italic);
            }
            None => {
                current = Some((
                    Word {
                        text: c.ch.to_string(),
                        rect: c.rect,
                        size: c.size,
                        bold: c.bold,
                        italic: c.italic,
                    },
                    1,
                    u32::from(c.bold),
                    u32::from(c.italic),
                ));
            }
        }
        previous = Some(c);
    }
    flush(&mut current, &mut words);
    words
}

/// Words that sit on one line and are close enough to read as one piece of text.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// The words, left to right.
    pub words: Vec<Word>,
    /// Their box.
    pub rect: Rect,
    /// The usual size of the text in it.
    pub size: f32,
    /// Bold throughout.
    pub bold: bool,
    /// Index of the line it is on, top to bottom.
    pub line: usize,
}

impl Segment {
    /// The text with a space between words.
    pub fn text(&self) -> String {
        self.words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Gap, as a multiple of the font size, above which words on one line are separate segments:
/// columns, table cells, a bullet and its far-off text.
const SEGMENT_GAP: f32 = 1.5;

/// Puts words on lines (by the height of their middle) and splits every line at wide gaps.
pub fn segments(words: &[Word]) -> Vec<Segment> {
    let mut order: Vec<&Word> = words.iter().collect();
    order.sort_by(|a, b| a.rect.mid_y().total_cmp(&b.rect.mid_y()));
    // Lines: a word joins the line whose middle it is close to.
    let mut lines: Vec<(f32, f32, Vec<&Word>)> = Vec::new(); // middle, size, words
    for word in order {
        let size = word.size.max(1.0);
        match lines.last_mut() {
            Some((mid, line_size, members))
                if (word.rect.mid_y() - *mid).abs() <= 0.5 * size.max(*line_size) =>
            {
                members.push(word);
                // The middle follows the words so a slow drift stays on the line.
                *mid = members.iter().map(|w| w.rect.mid_y()).sum::<f32>() / members.len() as f32;
                *line_size = line_size.max(size);
            }
            _ => lines.push((word.rect.mid_y(), size, vec![word])),
        }
    }
    let mut out = Vec::new();
    for (index, (_, _, mut members)) in lines.into_iter().enumerate() {
        members.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
        let mut current: Vec<Word> = Vec::new();
        for word in members {
            if let Some(last) = current.last() {
                let gap = word.rect.x0 - last.rect.x1;
                if gap > SEGMENT_GAP * word.size.max(last.size) {
                    out.push(make_segment(std::mem::take(&mut current), index));
                }
            }
            current.push(word.clone());
        }
        if !current.is_empty() {
            out.push(make_segment(current, index));
        }
    }
    out
}

fn make_segment(words: Vec<Word>, line: usize) -> Segment {
    let rect = words
        .iter()
        .map(|w| w.rect)
        .reduce(|a, b| a.union(&b))
        .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
    let chars: usize = words
        .iter()
        .map(|w| w.text.chars().count())
        .sum::<usize>()
        .max(1);
    // The size most characters have, weighted by length.
    let size = words
        .iter()
        .map(|w| w.size * w.text.chars().count() as f32)
        .sum::<f32>()
        / chars as f32;
    let bold_chars: usize = words
        .iter()
        .filter(|w| w.bold)
        .map(|w| w.text.chars().count())
        .sum();
    Segment {
        rect,
        size,
        bold: bold_chars * 2 > chars,
        line,
        words,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(text: &str, x: f32, y: f32, size: f32) -> Vec<CharBox> {
        let width = size * 0.5;
        text.chars()
            .enumerate()
            .map(|(i, ch)| CharBox {
                ch,
                rect: Rect::new(
                    x + width * i as f32,
                    y,
                    x + width * (i as f32 + 1.0),
                    y + size,
                ),
                size,
                bold: false,
                italic: false,
            })
            .collect()
    }

    fn word(text: &str, x: f32, y: f32, size: f32) -> Word {
        let width = size * 0.5 * text.chars().count() as f32;
        Word {
            text: text.to_owned(),
            rect: Rect::new(x, y, x + width, y + size),
            size,
            bold: false,
            italic: false,
        }
    }

    #[test]
    fn spaces_gaps_and_line_changes_end_words() {
        let mut all = chars("Hello world", 50.0, 100.0, 12.0);
        all.extend(chars("Next", 50.0, 120.0, 12.0));
        let words = words_from_chars(&all);
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, ["Hello", "world", "Next"]);
        assert!((words[0].rect.x1 - 80.0).abs() < 0.01);
    }

    #[test]
    fn a_wide_gap_without_a_space_still_splits_and_style_is_by_majority() {
        let mut a = chars("ab", 10.0, 10.0, 10.0);
        a.extend(chars("cd", 40.0, 10.0, 10.0));
        for c in &mut a[..2] {
            c.bold = true;
        }
        let words = words_from_chars(&a);
        assert_eq!(words.len(), 2);
        assert!(words[0].bold && !words[1].bold);
    }

    #[test]
    fn lines_are_found_by_height_and_split_at_gutters() {
        let words = vec![
            word("Left", 50.0, 100.0, 12.0),
            word("column", 90.0, 100.0, 12.0),
            word("Right", 300.0, 100.0, 12.0),
            word("Below", 50.0, 120.0, 12.0),
        ];
        let segs = segments(&words);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].text(), "Left column");
        assert_eq!(segs[1].text(), "Right");
        assert_eq!(segs[0].line, segs[1].line);
        assert_ne!(segs[0].line, segs[2].line);
    }

    #[test]
    fn a_slightly_raised_word_stays_on_its_line() {
        let words = vec![word("x", 50.0, 100.0, 12.0), word("2", 58.0, 97.0, 8.0)];
        let segs = segments(&words);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text(), "x 2");
    }
}
