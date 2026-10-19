//! Finding what Redact is asked to remove in the text of a page: words, patterns and kinds of
//! text found by their shape. A match is mapped back to the places of its characters, so only the
//! characters that matched are covered, not the whole word they are in.

use std::path::Path;

use domain::ErrorCode;
use domain::PageIndex;
use domain::layout::{CharBox, PageChars, Rect};
use domain::tools::common::Region;
use domain::tools::redact::{RedactOptions, RedactPreset};
use regex::{Regex, RegexBuilder};

use crate::error::AppError;
use crate::ports::LayoutReader;
use crate::runtime::{CancellationToken, Password};

/// Largest compiled size of one pattern, so a hostile pattern cannot take much memory.
const PATTERN_SIZE_LIMIT: usize = 1 << 20;
/// Gap, as a fraction of the font size, that makes two characters two words.
const WORD_GAP: f32 = 0.28;
/// Space added above and below a covered line, in points.
const VERTICAL_PAD: f32 = 0.5;

/// What the options ask to find on a page.
pub struct Matcher {
    plain: Vec<Regex>,
    presets: Vec<(RedactPreset, Regex)>,
}

fn compile(pattern: &str, case_sensitive: bool) -> Result<Regex, AppError> {
    RegexBuilder::new(pattern)
        .case_insensitive(!case_sensitive)
        .size_limit(PATTERN_SIZE_LIMIT)
        .nest_limit(60)
        .build()
        .map_err(|e| {
            let first = e.to_string();
            let first = first.lines().last().unwrap_or("").trim().to_owned();
            AppError::new(
                ErrorCode::OptionInvalid,
                format!("The pattern \"{pattern}\" is not valid: {first}"),
            )
        })
}

fn preset_regex(kind: RedactPreset) -> &'static str {
    match kind {
        RedactPreset::Email => {
            r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9](?:[A-Za-z0-9\-]*[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9\-]*[A-Za-z0-9])?)*\.[A-Za-z]{2,}"
        }
        RedactPreset::Phone => r"\+?\(?\d[\d ().\-\u{a0}]{5,}\d",
        RedactPreset::Iban => r"\b[A-Z]{2}[0-9]{2}(?: ?[A-Z0-9]{4}){2,7}(?: ?[A-Z0-9]{1,3})?\b",
        RedactPreset::CardNumber => r"\b\d(?:[ \-]?\d){12,18}\b",
    }
}

fn digits(text: &str) -> Vec<u32> {
    text.chars().filter_map(|c| c.to_digit(10)).collect()
}

fn luhn(number: &[u32]) -> bool {
    let sum: u32 = number
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| {
            if i % 2 == 1 {
                let doubled = d * 2;
                if doubled > 9 { doubled - 9 } else { doubled }
            } else {
                *d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// The check digits of an IBAN: moving the first four characters to the end and reading letters
/// as numbers (A is 10) leaves a remainder of 1 when divided by 97.
fn iban_ok(text: &str) -> bool {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if !(15..=34).contains(&compact.len()) {
        return false;
    }
    let rotated = format!("{}{}", &compact[4..], &compact[..4]);
    let mut remainder = 0u32;
    for c in rotated.chars() {
        let Some(value) = c.to_digit(36) else {
            return false;
        };
        // A letter is two digits (A is 10), a digit is one.
        let scale = if value >= 10 { 100 } else { 10 };
        remainder = (remainder * scale + value) % 97;
    }
    remainder == 1
}

/// Whether a candidate made of digits and the signs between them is a telephone number rather
/// than a date, an amount or a plain number.
fn phone_ok(text: &str) -> bool {
    let count = digits(text).len();
    if !(7..=15).contains(&count) {
        return false;
    }
    let has_sign = text.starts_with('+') || text.contains(['(', ')', ' ', '-', '.', '\u{a0}']);
    if !has_sign {
        return false;
    }
    // 2026-10-19, 19.10.2026 and 10/19/26 are dates.
    let groups: Vec<&str> = text
        .split(|c: char| !c.is_ascii_digit())
        .filter(|g| !g.is_empty())
        .collect();
    let short = |g: &str| g.len() <= 2;
    let date_like = groups.len() == 3
        && !text.starts_with('+')
        && ((groups[0].len() == 4 && short(groups[1]) && short(groups[2]))
            || (groups[2].len() == 4 && short(groups[0]) && short(groups[1]))
            || groups.iter().all(|g| short(g)));
    !date_like
}

fn valid(kind: RedactPreset, text: &str) -> bool {
    match kind {
        RedactPreset::Email => true,
        RedactPreset::Phone => phone_ok(text),
        RedactPreset::Iban => iban_ok(text),
        RedactPreset::CardNumber => {
            let d = digits(text);
            (13..=19).contains(&d.len()) && luhn(&d)
        }
    }
}

impl Matcher {
    /// Compiles what the options ask to find. Patterns that are not valid are refused.
    pub fn compile(options: &RedactOptions) -> Result<Self, AppError> {
        let mut plain = Vec::new();
        for term in &options.terms {
            // A space in a phrase matches any run of spaces and breaks between the words.
            let escaped: Vec<String> = term.split_whitespace().map(regex::escape).collect();
            plain.push(compile(&escaped.join(r"\s+"), options.case_sensitive)?);
        }
        for pattern in &options.patterns {
            plain.push(compile(pattern, options.case_sensitive)?);
        }
        let mut presets = Vec::new();
        for kind in &options.presets {
            presets.push((*kind, compile(preset_regex(*kind), true)?));
        }
        Ok(Self { plain, presets })
    }

    /// The byte ranges of `text` that match, in order and without overlaps. Empty matches are
    /// ignored.
    pub fn find(&self, text: &str) -> Vec<(usize, usize)> {
        let mut found: Vec<(usize, usize)> = Vec::new();
        for regex in &self.plain {
            found.extend(
                regex
                    .find_iter(text)
                    .filter(|m| !m.as_str().trim().is_empty())
                    .map(|m| (m.start(), m.end())),
            );
        }
        for (kind, regex) in &self.presets {
            found.extend(
                regex
                    .find_iter(text)
                    .filter(|m| valid(*kind, m.as_str()))
                    .map(|m| (m.start(), m.end())),
            );
        }
        found.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (start, end) in found {
            match merged.last_mut() {
                Some(last) if start <= last.1 => last.1 = last.1.max(end),
                _ => merged.push((start, end)),
            }
        }
        merged
    }
}

/// How a character follows the one before it in the content: on a new line (or column, for text
/// that runs up or down the page), or on the same one with a gap, in points, along the run.
fn follow(p: &CharBox, c: &CharBox) -> (bool, f32) {
    let size = c.size.max(p.size).max(1.0);
    let step_x = c.rect.mid_x() - p.rect.mid_x();
    let step_y = c.rect.mid_y() - p.rect.mid_y();
    if step_x.abs() >= step_y.abs() {
        // Text along the page: another line is another height.
        (step_y.abs() > 0.5 * size, c.rect.x0 - p.rect.x1)
    } else {
        // Text up or down the page: another column is another width.
        let gap = if step_y > 0.0 {
            c.rect.y0 - p.rect.y1
        } else {
            p.rect.y0 - c.rect.y1
        };
        (step_x.abs() > 0.5 * size, gap)
    }
}

/// The text of a page, rebuilt from its characters, with the character each letter came from.
pub struct PageText {
    /// The text. Words that had no space between them in the content are separated, and so are
    /// lines.
    pub text: String,
    /// Where each element starts in `text`, in bytes.
    starts: Vec<usize>,
    /// The index in the page's characters of each element, when it is one of them.
    owners: Vec<Option<usize>>,
}

impl PageText {
    /// Rebuilds the text of a page.
    pub fn of(page: &PageChars) -> Self {
        let mut text = String::new();
        let mut starts = Vec::new();
        let mut owners = Vec::new();
        let mut push = |ch: char, owner: Option<usize>| {
            starts.push(text.len());
            owners.push(owner);
            text.push(ch);
        };
        let mut previous: Option<&CharBox> = None;
        for (i, c) in page.chars.iter().enumerate() {
            let blank = c.ch.is_whitespace() || c.ch.is_control();
            if let Some(p) = previous
                && !blank
            {
                let size = c.size.max(p.size).max(1.0);
                let (line_change, gap) = follow(p, c);
                if line_change || gap < -size {
                    push('\n', None);
                } else if gap > WORD_GAP * size {
                    push(' ', None);
                }
            }
            if blank {
                // Line breaks and spaces of the content stay as they are, but a control
                // character is not text.
                push(if c.ch.is_control() { '\n' } else { c.ch }, Some(i));
            } else {
                push(c.ch, Some(i));
            }
            previous = if blank { None } else { Some(c) };
        }
        Self {
            text,
            starts,
            owners,
        }
    }

    /// The places the characters of `ranges` (byte ranges of the text) cover, one region for each
    /// stretch of one line, as fractions of the displayed page.
    pub fn regions(&self, page: &PageChars, ranges: &[(usize, usize)]) -> Vec<Region> {
        let mut out = Vec::new();
        for (start, end) in ranges {
            let first = self.starts.partition_point(|s| *s < *start);
            let last = self.starts.partition_point(|s| *s < *end);
            let mut group: Option<Rect> = None;
            let mut before: Option<&CharBox> = None;
            let flush = |group: &mut Option<Rect>, out: &mut Vec<Region>| {
                if let Some(rect) = group.take()
                    && let Some(region) = to_region(rect, page.width, page.height)
                {
                    out.push(region);
                }
            };
            for owner in &self.owners[first..last] {
                let Some(i) = owner else {
                    continue;
                };
                let c = &page.chars[*i];
                if c.ch.is_whitespace() || c.ch.is_control() {
                    continue;
                }
                let joins = before.is_some_and(|p| {
                    let (line_change, gap) = follow(p, c);
                    !line_change && gap >= -c.size.max(p.size).max(1.0)
                });
                match &mut group {
                    Some(rect) if joins => *rect = rect.union(&c.rect),
                    _ => {
                        flush(&mut group, &mut out);
                        group = Some(c.rect);
                    }
                }
                before = Some(c);
            }
            flush(&mut group, &mut out);
        }
        out
    }
}

/// The places on the pages the options search that match what they ask for, page by page.
/// `on_page` is called after each page is read.
pub fn find_matches(
    layout: &dyn LayoutReader,
    path: &Path,
    password: Option<&Password>,
    options: &RedactOptions,
    page_count: u32,
    cancel: &CancellationToken,
    on_page: &mut dyn FnMut(),
) -> Result<Vec<(PageIndex, Vec<Region>)>, AppError> {
    let matcher = Matcher::compile(options)?;
    let mut out = Vec::new();
    for page in options.search_pages(page_count)?.pages() {
        cancel.check()?;
        let chars = layout.page_chars(path, password, *page)?;
        let text = PageText::of(&chars);
        let ranges = matcher.find(&text.text);
        if !ranges.is_empty() {
            out.push((*page, text.regions(&chars, &ranges)));
        }
        on_page();
    }
    Ok(out)
}

fn to_region(rect: Rect, width: f32, height: f32) -> Option<Region> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let x0 = (rect.x0 / width).clamp(0.0, 1.0);
    let x1 = (rect.x1 / width).clamp(0.0, 1.0);
    let y0 = ((rect.y0 - VERTICAL_PAD) / height).clamp(0.0, 1.0);
    let y1 = ((rect.y1 + VERTICAL_PAD) / height).clamp(0.0, 1.0);
    (x1 > x0 && y1 > y0).then_some(Region {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::tools::redact::RedactOptions;

    fn options(f: impl FnOnce(&mut RedactOptions)) -> RedactOptions {
        let mut o = RedactOptions::default();
        f(&mut o);
        o
    }

    fn found(options: &RedactOptions, text: &str) -> Vec<String> {
        Matcher::compile(options)
            .unwrap()
            .find(text)
            .into_iter()
            .map(|(a, b)| text[a..b].to_owned())
            .collect()
    }

    #[test]
    fn words_match_without_regard_to_case_and_across_line_breaks() {
        let o = options(|o| o.terms = vec!["john smith".into()]);
        assert_eq!(
            found(&o, "From John Smith, and JOHN\n  smith too"),
            ["John Smith", "JOHN\n  smith"]
        );
    }

    #[test]
    fn case_can_be_made_to_matter_and_special_signs_are_literal() {
        let o = options(|o| {
            o.terms = vec!["a.b".into()];
            o.case_sensitive = true;
        });
        assert_eq!(found(&o, "a.b aXb A.B"), ["a.b"]);
    }

    #[test]
    fn patterns_are_regular_expressions_and_bad_ones_are_refused() {
        let o = options(|o| o.patterns = vec![r"INV-\d{4}".into()]);
        assert_eq!(found(&o, "INV-1001 and INV-22"), ["INV-1001"]);
        let bad = options(|o| o.patterns = vec!["(unclosed".into()]);
        let Err(error) = Matcher::compile(&bad) else {
            panic!("a bad pattern was accepted");
        };
        assert_eq!(error.code, ErrorCode::OptionInvalid);
        assert!(error.message.contains("(unclosed"));
    }

    #[test]
    fn emails_are_found() {
        let o = options(|o| o.presets = vec![RedactPreset::Email]);
        assert_eq!(
            found(
                &o,
                "Write to ada.lovelace+x@example.co.uk, or bob@host.org."
            ),
            ["ada.lovelace+x@example.co.uk", "bob@host.org"]
        );
        assert!(found(&o, "no address @ here").is_empty());
    }

    #[test]
    fn phone_numbers_need_signs_and_are_not_dates_or_plain_numbers() {
        let o = options(|o| o.presets = vec![RedactPreset::Phone]);
        assert_eq!(
            found(&o, "Call +1 (555) 123-4567 or 020 7946 0958 now"),
            ["+1 (555) 123-4567", "020 7946 0958"]
        );
        assert!(found(&o, "Date 2026-10-19 and 19.10.2026 and 10/19/26").is_empty());
        assert!(found(&o, "Order 12345678 shipped").is_empty());
        assert!(found(&o, "Pages 12 - 14").is_empty());
    }

    #[test]
    fn ibans_and_cards_must_pass_their_checks() {
        let iban = options(|o| o.presets = vec![RedactPreset::Iban]);
        assert_eq!(
            found(&iban, "Pay GB82 WEST 1234 5698 7654 32 today"),
            ["GB82 WEST 1234 5698 7654 32"]
        );
        assert!(found(&iban, "Pay GB83 WEST 1234 5698 7654 32 today").is_empty());
        assert_eq!(
            found(&iban, "DE89370400440532013000"),
            ["DE89370400440532013000"]
        );
        let card = options(|o| o.presets = vec![RedactPreset::CardNumber]);
        assert_eq!(
            found(&card, "Card 4111 1111 1111 1111, exp 12/30"),
            ["4111 1111 1111 1111"]
        );
        assert_eq!(found(&card, "4111-1111-1111-1111"), ["4111-1111-1111-1111"]);
        assert!(found(&card, "Card 4111 1111 1111 1112").is_empty());
        assert!(found(&card, "Reference 1234567").is_empty());
    }

    #[test]
    fn overlapping_matches_are_merged() {
        let o = options(|o| {
            o.terms = vec!["john".into(), "john smith".into()];
            o.patterns = vec!["smith jr".into()];
        });
        assert_eq!(found(&o, "John Smith Jr"), ["John Smith Jr"]);
    }

    fn word(x: f32, y: f32, text: &str) -> Vec<CharBox> {
        text.chars()
            .enumerate()
            .map(|(i, ch)| CharBox {
                ch,
                rect: Rect::new(x + 6.0 * i as f32, y, x + 6.0 * (i + 1) as f32, y + 10.0),
                size: 10.0,
                bold: false,
                italic: false,
            })
            .collect()
    }

    fn page(chars: Vec<CharBox>) -> PageChars {
        PageChars {
            width: 600.0,
            height: 800.0,
            chars,
        }
    }

    #[test]
    fn words_and_lines_that_the_content_does_not_separate_are_separated() {
        let mut chars = word(10.0, 100.0, "Hello");
        chars.extend(word(50.0, 100.0, "SECRET"));
        chars.extend(word(10.0, 130.0, "next"));
        let p = page(chars);
        assert_eq!(PageText::of(&p).text, "Hello SECRET\nnext");
    }

    #[test]
    fn a_match_is_covered_by_the_boxes_of_its_own_characters() {
        let mut chars = word(10.0, 100.0, "Hi");
        chars.push(CharBox {
            ch: ' ',
            rect: Rect::new(22.0, 100.0, 28.0, 110.0),
            size: 10.0,
            bold: false,
            italic: false,
        });
        chars.extend(word(28.0, 100.0, "SECRETS"));
        let p = page(chars);
        let text = PageText::of(&p);
        let matcher = Matcher::compile(&options(|o| o.terms = vec!["secret".into()])).unwrap();
        let ranges = matcher.find(&text.text);
        let regions = text.regions(&p, &ranges);
        assert_eq!(regions.len(), 1);
        let r = regions[0];
        // Six characters of 6 points from x = 28; the S at the end is not covered.
        assert!((r.x - 28.0 / 600.0).abs() < 1e-4);
        assert!((r.width - 36.0 / 600.0).abs() < 1e-4);
        assert!((r.y - 99.5 / 800.0).abs() < 1e-4);
        assert!((r.height - 11.0 / 800.0).abs() < 1e-4);
    }

    #[test]
    fn a_match_over_two_lines_gives_a_region_for_each_line() {
        let mut chars = word(10.0, 100.0, "John");
        chars.extend(word(10.0, 130.0, "Smith"));
        let p = page(chars);
        let text = PageText::of(&p);
        let matcher = Matcher::compile(&options(|o| o.terms = vec!["john smith".into()])).unwrap();
        let regions = text.regions(&p, &matcher.find(&text.text));
        assert_eq!(regions.len(), 2);
        assert!(regions[0].y < regions[1].y);
    }
}
