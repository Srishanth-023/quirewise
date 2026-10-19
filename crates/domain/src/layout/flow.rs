//! From the words of a page to blocks in reading order: columns, paragraphs, headings, lists,
//! tables and pictures.

use super::Detection;
use super::model::{Block, PageContent, PageLayout, PictureBox, Rect, Span, Table, Word, plain};
use super::tables::{lattice_tables, stream_tables};
use super::words::{Segment, segments};

/// Sizes of body text and of headings in a document.
#[derive(Debug, Clone, PartialEq)]
pub struct BodyStats {
    /// The size most characters have.
    pub body_size: f32,
    /// The sizes that are clearly larger than the body, largest first.
    pub heading_sizes: Vec<f32>,
}

/// Rounds a size to half a point so 11.98 and 12.02 are one size.
fn rounded(size: f32) -> f32 {
    (size * 2.0).round() / 2.0
}

/// Finds the body size (the size most characters have) and the larger sizes headings use.
pub fn body_stats(pages: &[PageContent]) -> BodyStats {
    let mut counts: Vec<(f32, usize)> = Vec::new();
    for word in pages.iter().flat_map(|p| &p.words) {
        let size = rounded(word.size);
        let chars = word.text.chars().count();
        match counts.iter_mut().find(|(s, _)| (*s - size).abs() < 0.01) {
            Some((_, n)) => *n += chars,
            None => counts.push((size, chars)),
        }
    }
    let body_size = counts
        .iter()
        .max_by_key(|(_, n)| *n)
        .map_or(11.0, |(s, _)| *s)
        .max(4.0);
    let mut heading_sizes: Vec<f32> = counts
        .iter()
        .map(|(s, _)| *s)
        .filter(|s| *s >= body_size * 1.15)
        .collect();
    heading_sizes.sort_by(|a, b| b.total_cmp(a));
    BodyStats {
        body_size,
        heading_sizes,
    }
}

/// What an item of a page is, for cutting the page into columns.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Segment(usize),
    Table(usize),
    Picture(usize),
}

#[derive(Debug, Clone, Copy)]
struct Item {
    kind: Kind,
    rect: Rect,
}

/// Cuts the items into columns and bands and returns them in reading order: a column is read from
/// top to bottom before the next one, and a full-width band before what is under it.
fn reading_order(items: &[Item], body: f32) -> Vec<usize> {
    let ids: Vec<usize> = (0..items.len()).collect();
    order(&ids, items, body)
}

fn order(ids: &[usize], items: &[Item], body: f32) -> Vec<usize> {
    if ids.len() <= 1 {
        return ids.to_vec();
    }
    if let Some((left, right)) = vertical_cut(ids, items, body) {
        let mut out = order(&left, items, body);
        out.extend(order(&right, items, body));
        return out;
    }
    if let Some((top, bottom)) = horizontal_cut(ids, items, body) {
        let mut out = order(&top, items, body);
        out.extend(order(&bottom, items, body));
        return out;
    }
    let mut sorted = ids.to_vec();
    sorted.sort_by(|a, b| {
        items[*a]
            .rect
            .y0
            .total_cmp(&items[*b].rect.y0)
            .then(items[*a].rect.x0.total_cmp(&items[*b].rect.x0))
    });
    sorted
}

/// Occupied stretches of a number line, merged, from `(start, end)` pairs.
fn occupied(mut spans: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f32, f32)> = Vec::new();
    for (a, b) in spans {
        match out.last_mut() {
            Some((_, end)) if a <= *end => *end = end.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn vertical_cut(ids: &[usize], items: &[Item], body: f32) -> Option<(Vec<usize>, Vec<usize>)> {
    let merged = occupied(
        ids.iter()
            .map(|i| (items[*i].rect.x0, items[*i].rect.x1))
            .collect(),
    );
    let gutter = (1.5 * body).max(10.0);
    let min_column = 4.0 * body;
    let mut best: Option<(f32, f32)> = None; // gap width, cut position
    for pair in merged.windows(2) {
        let (gap, cut) = (pair[1].0 - pair[0].1, (pair[0].1 + pair[1].0) / 2.0);
        let left_width = pair[0].1 - merged[0].0;
        let right_width = merged[merged.len() - 1].1 - pair[1].0;
        if gap >= gutter
            && left_width >= min_column
            && right_width >= min_column
            && best.is_none_or(|(w, _)| gap > w)
        {
            best = Some((gap, cut));
        }
    }
    let (_, cut) = best?;
    let (left, right): (Vec<usize>, Vec<usize>) =
        ids.iter().partition(|i| items[**i].rect.mid_x() < cut);
    (!left.is_empty() && !right.is_empty()).then_some((left, right))
}

fn horizontal_cut(ids: &[usize], items: &[Item], body: f32) -> Option<(Vec<usize>, Vec<usize>)> {
    let merged = occupied(
        ids.iter()
            .map(|i| (items[*i].rect.y0, items[*i].rect.y1))
            .collect(),
    );
    let band = 0.6 * body;
    let mut best: Option<(f32, f32)> = None;
    for pair in merged.windows(2) {
        let (gap, cut) = (pair[1].0 - pair[0].1, (pair[0].1 + pair[1].0) / 2.0);
        if gap >= band && best.is_none_or(|(w, _)| gap > w) {
            best = Some((gap, cut));
        }
    }
    let (_, cut) = best?;
    let (top, bottom): (Vec<usize>, Vec<usize>) =
        ids.iter().partition(|i| items[**i].rect.mid_y() < cut);
    (!top.is_empty() && !bottom.is_empty()).then_some((top, bottom))
}

// ----- lists -------------------------------------------------------------------------------

const BULLETS: [char; 12] = [
    '•', '◦', '▪', '■', '●', '○', '·', '–', '\u{F0B7}', '\u{F0A7}', '\u{2022}', '*',
];

/// A list marker at the start of `text`: whether it counts, and the marker as written.
pub fn list_marker(text: &str) -> Option<(bool, String)> {
    let first = text.split_whitespace().next()?;
    let mut chars = first.chars();
    let head = chars.next()?;
    if first.chars().count() == 1 && (BULLETS.contains(&head) || head == '-') {
        return Some((false, first.to_owned()));
    }
    // A bullet glued to the text, such as "•Item".
    if BULLETS.contains(&head) && head != '*' && head != '–' && first.chars().count() > 1 {
        return Some((false, head.to_string()));
    }
    // Numbers and letters: "1.", "2)", "a)", "(iv)".
    let core = first.trim_start_matches('(');
    let marker_end = core.find(['.', ')'])?;
    if marker_end + 1 != core.len() {
        return None;
    }
    let label = &core[..marker_end];
    let digits = !label.is_empty() && label.len() <= 3 && label.chars().all(|c| c.is_ascii_digit());
    let letter = label.len() == 1 && label.chars().all(|c| c.is_ascii_alphabetic());
    let roman = label.len() <= 6
        && !label.is_empty()
        && label.chars().all(|c| "ivxlcdmIVXLCDM".contains(c))
        && label.len() > 1;
    (digits || letter || roman).then(|| (true, first.to_owned()))
}

// ----- paragraphs --------------------------------------------------------------------------

struct Para {
    segments: Vec<usize>,
    rect: Rect,
    left: f32,
}

fn continues(para: &Para, segs: &[Segment], next: &Segment) -> bool {
    let Some(&last_index) = para.segments.last() else {
        return false;
    };
    let last = &segs[last_index];
    if next.line == last.line {
        return false;
    }
    let size = last.size.max(next.size).max(1.0);
    let gap = next.rect.y0 - last.rect.y1;
    let same_style = (next.size - last.size).abs() <= 0.6 && next.bold == last.bold;
    let aligned = (next.rect.x0 - para.left).abs() <= 1.5 * size
        || (next.rect.x0 - last.rect.x0).abs() <= 1.5 * size;
    gap >= -0.4 * size
        && gap <= 0.55 * size
        && same_style
        && aligned
        && list_marker(&next.text()).is_none()
}

/// The text of segments as styled spans: words joined by spaces, a hyphen at the end of a line
/// joined to the word on the next line.
fn spans_of(segs: &[&Segment]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut pending_join = false;
    for seg in segs {
        for (n, word) in seg.words.iter().enumerate() {
            let style = (word.bold, word.italic, rounded(word.size));
            let continues_span = spans
                .last()
                .is_some_and(|s| (s.bold, s.italic, rounded(s.size)) == style);
            let first = spans.is_empty();
            let mut text = word.text.clone();
            let join =
                pending_join && n == 0 && text.chars().next().is_some_and(char::is_lowercase);
            if join {
                // "inter-" and "national" are one word.
                if let Some(last) = spans.last_mut() {
                    last.text.pop();
                }
            } else if !first {
                text.insert(0, ' ');
            }
            if continues_span {
                if let Some(last) = spans.last_mut() {
                    last.text.push_str(&text);
                }
            } else {
                spans.push(Span {
                    text,
                    bold: word.bold,
                    italic: word.italic,
                    size: word.size,
                });
            }
            pending_join = false;
        }
        let ends_with_hyphen = seg
            .words
            .last()
            .is_some_and(|w| w.text.ends_with('-') && w.text.chars().count() > 2);
        pending_join = ends_with_hyphen;
    }
    spans
}

/// Removes the marker from the start of the spans.
fn without_marker(mut spans: Vec<Span>, marker: &str) -> Vec<Span> {
    if let Some(first) = spans.first_mut() {
        let text = first.text.trim_start();
        if let Some(rest) = text.strip_prefix(marker) {
            first.text = rest.trim_start().to_owned();
        }
    }
    spans.retain(|s| !s.text.is_empty());
    spans
}

fn heading_level(stats: &BodyStats, size: f32, bold_only: bool) -> u8 {
    if bold_only {
        return u8::try_from((stats.heading_sizes.len() + 1).min(3)).unwrap_or(3);
    }
    let rank = stats
        .heading_sizes
        .iter()
        .position(|s| (*s - rounded(size)).abs() < 0.76)
        .unwrap_or(0);
    u8::try_from((rank + 1).min(3)).unwrap_or(3)
}

fn finish_paragraph(para: &Para, segs: &[Segment], stats: &BodyStats, blocks: &mut Vec<Block>) {
    let members: Vec<&Segment> = para.segments.iter().map(|i| &segs[*i]).collect();
    let spans = spans_of(&members);
    if spans.is_empty() {
        return;
    }
    let text = plain(&spans);
    let size = members.iter().map(|s| s.size).sum::<f32>() / members.len() as f32;
    let bold = members.iter().all(|s| s.bold);
    let lines = members.len();
    let rect = para.rect;
    if let Some((ordered, marker)) = list_marker(&text) {
        blocks.push(Block::ListItem {
            ordered,
            marker: if ordered {
                marker.clone()
            } else {
                String::new()
            },
            level: 0,
            spans: without_marker(spans, &marker),
            rect,
        });
        return;
    }
    let larger = size >= stats.body_size * 1.15 && lines <= 3;
    let bold_heading = bold
        && lines == 1
        && text.chars().count() <= 90
        && !text.ends_with(['.', ',', ';', ':'])
        && size >= stats.body_size * 0.95;
    if larger || bold_heading {
        blocks.push(Block::Heading {
            level: heading_level(stats, size, !larger),
            spans,
            rect,
        });
    } else {
        blocks.push(Block::Paragraph { spans, rect });
    }
}

/// Indents of list items, in steps of about one level.
const LEVEL_STEP: f32 = 18.0;

fn set_list_levels(blocks: &mut [Block]) {
    let left = blocks
        .iter()
        .filter_map(|b| match b {
            Block::ListItem { rect, .. } => Some(rect.x0),
            _ => None,
        })
        .fold(f32::MAX, f32::min);
    for block in blocks {
        if let Block::ListItem { level, rect, .. } = block {
            *level = (((rect.x0 - left) / LEVEL_STEP).round().max(0.0) as u8).min(4);
        }
    }
}

// ----- one page ----------------------------------------------------------------------------

/// Turns the content of a page into blocks in reading order.
pub fn analyze_page(page: &PageContent, stats: &BodyStats, detection: Detection) -> PageLayout {
    let mut words: Vec<Word> = page.words.clone();
    let mut tables: Vec<Table> = Vec::new();

    if detection != Detection::Stream {
        let mut taken = vec![false; words.len()];
        for (table, consumed) in lattice_tables(&page.rules, &words) {
            for i in consumed {
                taken[i] = true;
            }
            tables.push(table);
        }
        words = words
            .into_iter()
            .zip(taken)
            .filter(|(_, t)| !t)
            .map(|(w, _)| w)
            .collect();
    }
    let mut segs = segments(&words);
    if detection != Detection::Lattice {
        let mut taken = vec![false; segs.len()];
        for (table, consumed) in stream_tables(&segs) {
            for i in consumed {
                taken[i] = true;
            }
            tables.push(table);
        }
        segs = segs
            .into_iter()
            .zip(taken)
            .filter(|(_, t)| !t)
            .map(|(s, _)| s)
            .collect();
    }

    let mut items: Vec<Item> = Vec::new();
    for (i, s) in segs.iter().enumerate() {
        items.push(Item {
            kind: Kind::Segment(i),
            rect: s.rect,
        });
    }
    for (i, t) in tables.iter().enumerate() {
        items.push(Item {
            kind: Kind::Table(i),
            rect: t.rect,
        });
    }
    for (i, p) in page.pictures.iter().enumerate() {
        items.push(Item {
            kind: Kind::Picture(i),
            rect: p.rect,
        });
    }

    let mut blocks: Vec<Block> = Vec::new();
    let mut current: Option<Para> = None;
    let flush = |current: &mut Option<Para>, blocks: &mut Vec<Block>| {
        if let Some(para) = current.take() {
            finish_paragraph(&para, &segs, stats, blocks);
        }
    };
    for index in reading_order(&items, stats.body_size) {
        match items[index].kind {
            Kind::Segment(i) => {
                let seg = &segs[i];
                match &mut current {
                    Some(para) if continues(para, &segs, seg) => {
                        para.segments.push(i);
                        para.rect = para.rect.union(&seg.rect);
                    }
                    _ => {
                        flush(&mut current, &mut blocks);
                        current = Some(Para {
                            segments: vec![i],
                            rect: seg.rect,
                            left: seg.rect.x0,
                        });
                    }
                }
            }
            Kind::Table(i) => {
                flush(&mut current, &mut blocks);
                blocks.push(Block::Table(tables[i].clone()));
            }
            Kind::Picture(i) => {
                flush(&mut current, &mut blocks);
                blocks.push(Block::Picture(PictureBox {
                    rect: page.pictures[i].rect,
                    index: page.pictures[i].index,
                }));
            }
        }
    }
    flush(&mut current, &mut blocks);
    set_list_levels(&mut blocks);
    PageLayout {
        number: page.number,
        width: page.width,
        height: page.height,
        blocks,
    }
}

/// Removes what repeats at the top or bottom of most pages: running headers, footers and page
/// numbers. Needs at least three pages to be sure.
pub fn strip_running(pages: &mut [PageContent]) {
    if pages.len() < 3 {
        return;
    }
    let key = |text: &str, rect: &Rect| {
        let digits_as_hash: String = text
            .chars()
            .map(|c| if c.is_ascii_digit() { '#' } else { c })
            .collect();
        (digits_as_hash, (rect.y0 / 4.0).round() as i32)
    };
    let mut seen: Vec<((String, i32), usize)> = Vec::new();
    for page in pages.iter() {
        let band = page.height * 0.09;
        let mut keys_on_page: Vec<(String, i32)> = Vec::new();
        for seg in segments(&page.words) {
            if seg.rect.y1 > band && seg.rect.y0 < page.height - band {
                continue;
            }
            let k = key(&seg.text(), &seg.rect);
            if !keys_on_page.contains(&k) {
                keys_on_page.push(k);
            }
        }
        for k in keys_on_page {
            match seen.iter_mut().find(|(s, _)| *s == k) {
                Some((_, n)) => *n += 1,
                None => seen.push((k, 1)),
            }
        }
    }
    let needed = (pages.len() * 6).div_ceil(10).max(2);
    let repeated: Vec<(String, i32)> = seen
        .into_iter()
        .filter(|(_, n)| *n >= needed)
        .map(|(k, _)| k)
        .collect();
    if repeated.is_empty() {
        return;
    }
    for page in pages.iter_mut() {
        let band = page.height * 0.09;
        let doomed: Vec<Rect> = segments(&page.words)
            .into_iter()
            .filter(|s| s.rect.y1 <= band || s.rect.y0 >= page.height - band)
            .filter(|s| repeated.contains(&key(&s.text(), &s.rect)))
            .map(|s| s.rect)
            .collect();
        page.words.retain(|w| {
            !doomed
                .iter()
                .any(|r| r.contains(w.rect.mid_x(), w.rect.mid_y()))
        });
    }
}

/// Analyzes a whole document: removes running headers and footers, measures the body text and
/// analyzes every page.
pub fn analyze(mut pages: Vec<PageContent>, detection: Detection) -> Vec<PageLayout> {
    strip_running(&mut pages);
    let stats = body_stats(&pages);
    pages
        .iter()
        .map(|p| analyze_page(p, &stats, detection))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::model::Rule;

    fn word(text: &str, x: f32, y: f32, size: f32, bold: bool) -> Word {
        let width = size * 0.5 * text.chars().count() as f32;
        Word {
            text: text.to_owned(),
            rect: Rect::new(x, y, x + width, y + size),
            size,
            bold,
            italic: false,
        }
    }

    /// Words of a sentence laid out left to right from `x`, `size` points high.
    fn line(text: &str, x: f32, y: f32, size: f32, bold: bool) -> Vec<Word> {
        let mut cursor = x;
        let mut words = Vec::new();
        for part in text.split(' ') {
            let w = word(part, cursor, y, size, bold);
            cursor = w.rect.x1 + size * 0.25;
            words.push(w);
        }
        words
    }

    fn page(words: Vec<Word>) -> PageContent {
        PageContent {
            number: 1,
            width: 612.0,
            height: 792.0,
            words,
            rules: vec![],
            pictures: vec![],
        }
    }

    fn kinds(layout: &PageLayout) -> Vec<String> {
        layout
            .blocks
            .iter()
            .map(|b| match b {
                Block::Heading { level, spans, .. } => format!("h{level}:{}", plain(spans)),
                Block::Paragraph { spans, .. } => format!("p:{}", plain(spans)),
                Block::ListItem {
                    ordered,
                    marker,
                    spans,
                    level,
                    ..
                } => {
                    format!(
                        "li{}{}:{}{}",
                        if *ordered { "#" } else { "-" },
                        level,
                        marker,
                        plain(spans)
                    )
                }
                Block::Table(t) => format!("table {}x{}", t.rows.len(), t.columns()),
                Block::Picture(_) => "picture".to_owned(),
            })
            .collect()
    }

    fn analyze_one(p: PageContent) -> PageLayout {
        let stats = body_stats(std::slice::from_ref(&p));
        analyze_page(&p, &stats, Detection::Auto)
    }

    #[test]
    fn a_larger_line_is_a_heading_and_lines_run_into_paragraphs() {
        let mut words = line("Annual Report", 50.0, 60.0, 24.0, false);
        words.extend(line(
            "The year began with steady growth in every region and",
            50.0,
            120.0,
            11.0,
            false,
        ));
        words.extend(line(
            "continued that way until the autumn quarter.",
            50.0,
            133.0,
            11.0,
            false,
        ));
        words.extend(line(
            "A second paragraph starts after a gap.",
            50.0,
            163.0,
            11.0,
            false,
        ));
        let layout = analyze_one(page(words));
        assert_eq!(
            kinds(&layout),
            [
                "h1:Annual Report",
                "p:The year began with steady growth in every region and continued that way until the autumn quarter.",
                "p:A second paragraph starts after a gap."
            ]
        );
    }

    #[test]
    fn heading_sizes_become_levels_largest_first() {
        let mut words = line("Title", 50.0, 40.0, 28.0, false);
        words.extend(line("Section", 50.0, 100.0, 18.0, false));
        for i in 0..6 {
            words.extend(line(
                "Body text that fills the page with ordinary words",
                50.0,
                150.0 + 13.0 * i as f32,
                11.0,
                false,
            ));
        }
        let layout = analyze_one(page(words));
        let k = kinds(&layout);
        assert_eq!(k[0], "h1:Title");
        assert_eq!(k[1], "h2:Section");
    }

    #[test]
    fn a_short_bold_line_without_a_full_stop_is_a_heading_too() {
        let mut words = line("Background", 50.0, 100.0, 11.0, true);
        words.extend(line(
            "Plain text follows the heading and continues on",
            50.0,
            120.0,
            11.0,
            false,
        ));
        words.extend(line(
            "for a second line of the paragraph here.",
            50.0,
            133.0,
            11.0,
            false,
        ));
        let layout = analyze_one(page(words));
        assert!(kinds(&layout)[0].starts_with("h"), "{:?}", kinds(&layout));
        assert!(kinds(&layout)[1].starts_with("p:Plain text"));
    }

    #[test]
    fn bullets_and_numbers_become_list_items_with_their_markers_removed() {
        let mut words = Vec::new();
        words.extend(line(
            "Intro paragraph before the list of items follows here",
            50.0,
            90.0,
            11.0,
            false,
        ));
        words.extend(line("• First point", 50.0, 120.0, 11.0, false));
        words.extend(line("• Second point", 50.0, 134.0, 11.0, false));
        words.extend(line("1. Number one", 50.0, 168.0, 11.0, false));
        words.extend(line("2. Number two", 50.0, 182.0, 11.0, false));
        let k = kinds(&analyze_one(page(words)));
        assert!(k[0].starts_with("p:Intro"));
        assert_eq!(k[1], "li-0:First point");
        assert_eq!(k[2], "li-0:Second point");
        assert_eq!(k[3], "li#0:1.Number one");
        assert_eq!(k[4], "li#0:2.Number two");
    }

    #[test]
    fn a_wrapped_list_item_stays_one_item_and_indent_gives_the_level() {
        let mut words = Vec::new();
        words.extend(line(
            "• A long item that wraps onto",
            50.0,
            100.0,
            11.0,
            false,
        ));
        words.extend(line("a second line", 62.0, 113.0, 11.0, false));
        words.extend(line("• A nested point", 74.0, 135.0, 11.0, false));
        let k = kinds(&analyze_one(page(words)));
        assert_eq!(k[0], "li-0:A long item that wraps onto a second line");
        assert_eq!(k[1], "li-1:A nested point");
    }

    #[test]
    fn two_columns_are_read_one_after_the_other() {
        let mut words = Vec::new();
        for (i, text) in [
            "Left column first line of the running text",
            "Left column second line of the running text",
            "Left column third line of the running text",
        ]
        .iter()
        .enumerate()
        {
            words.extend(line(text, 50.0, 100.0 + 13.0 * i as f32, 11.0, false));
        }
        for (i, text) in [
            "Right column first line of the running text",
            "Right column second line of the running text",
            "Right column third line of the running text",
        ]
        .iter()
        .enumerate()
        {
            words.extend(line(text, 330.0, 100.0 + 13.0 * i as f32, 11.0, false));
        }
        let k = kinds(&analyze_one(page(words)));
        assert_eq!(k.len(), 2, "{k:?}");
        assert!(
            k[0].starts_with(
                "p:Left column first line of the running text Left column second line"
            )
        );
        assert!(k[1].starts_with("p:Right column first line of the running text"));
    }

    #[test]
    fn a_full_width_title_comes_before_the_columns_under_it() {
        let mut words = line("Newsletter of the month", 50.0, 50.0, 22.0, false);
        words.extend(line(
            "Left text goes here and here",
            50.0,
            120.0,
            11.0,
            false,
        ));
        words.extend(line(
            "Right text goes here and here",
            330.0,
            120.0,
            11.0,
            false,
        ));
        let k = kinds(&analyze_one(page(words)));
        assert_eq!(k[0], "h1:Newsletter of the month");
        assert!(k[1].starts_with("p:Left"));
        assert!(k[2].starts_with("p:Right"));
    }

    #[test]
    fn a_table_with_lines_sits_between_the_paragraphs_around_it() {
        let mut words = line(
            "Before the table there is a sentence of text",
            50.0,
            80.0,
            11.0,
            false,
        );
        let mut rules = Vec::new();
        for r in 0..3 {
            let y = 120.0 + 30.0 * r as f32;
            rules.push(Rule {
                rect: Rect::new(50.0, y, 250.0, y + 1.0),
            });
        }
        for c in 0..3 {
            let x = 50.0 + 100.0 * c as f32;
            rules.push(Rule {
                rect: Rect::new(x, 120.0, x + 1.0, 180.0),
            });
        }
        for (r, (a, b)) in [("Name", "Qty"), ("Pens", "12")].iter().enumerate() {
            words.push(word(a, 60.0, 130.0 + 30.0 * r as f32, 11.0, false));
            words.push(word(b, 160.0, 130.0 + 30.0 * r as f32, 11.0, false));
        }
        words.extend(line(
            "After the table there is another sentence",
            50.0,
            220.0,
            11.0,
            false,
        ));
        let mut content = page(words);
        content.rules = rules;
        let k = kinds(&analyze_one(content));
        assert_eq!(k.len(), 3, "{k:?}");
        assert!(k[0].starts_with("p:Before"));
        assert_eq!(k[1], "table 2x2");
        assert!(k[2].starts_with("p:After"));
    }

    #[test]
    fn a_picture_takes_its_place_in_the_order() {
        let mut words = line(
            "Text above the picture is long enough to be read",
            50.0,
            60.0,
            11.0,
            false,
        );
        words.extend(line(
            "Text below the picture is long enough to be read",
            50.0,
            400.0,
            11.0,
            false,
        ));
        let mut content = page(words);
        content.pictures.push(PictureBox {
            rect: Rect::new(50.0, 120.0, 300.0, 340.0),
            index: 0,
        });
        let k = kinds(&analyze_one(content));
        assert_eq!(
            k,
            [
                "p:Text above the picture is long enough to be read",
                "picture",
                "p:Text below the picture is long enough to be read"
            ]
        );
    }

    #[test]
    fn a_hyphen_at_the_end_of_a_line_joins_the_word() {
        let mut words = line("The international commu-", 50.0, 100.0, 11.0, false);
        words.extend(line("nity agreed on the plan", 50.0, 113.0, 11.0, false));
        let k = kinds(&analyze_one(page(words)));
        assert_eq!(k[0], "p:The international community agreed on the plan");
    }

    #[test]
    fn bold_and_italic_words_make_their_own_spans() {
        let mut words = line("Plain", 50.0, 100.0, 11.0, false);
        words.push(word("bold", 85.0, 100.0, 11.0, true));
        words.push(Word {
            italic: true,
            ..word("slanted", 115.0, 100.0, 11.0, false)
        });
        let layout = analyze_one(page(words));
        let Block::Paragraph { spans, .. } = &layout.blocks[0] else {
            panic!("not a paragraph")
        };
        assert_eq!(spans.len(), 3);
        assert!(!spans[0].bold && spans[1].bold && spans[2].italic);
        assert_eq!(plain(spans), "Plain bold slanted");
    }

    #[test]
    fn running_headers_and_page_numbers_are_removed_when_most_pages_repeat_them() {
        let mut pages = Vec::new();
        for n in 1..=4u32 {
            let mut words = line("ACME Annual Report", 50.0, 20.0, 9.0, false);
            words.extend(line(
                "Body of the page with real content to keep",
                50.0,
                300.0,
                11.0,
                false,
            ));
            words.push(word(&n.to_string(), 300.0, 760.0, 9.0, false));
            pages.push(PageContent {
                number: n,
                ..page(words)
            });
        }
        let layouts = analyze(pages, Detection::Auto);
        for layout in &layouts {
            let k = kinds(layout);
            assert_eq!(k.len(), 1, "{k:?}");
            assert!(k[0].starts_with("p:Body of the page"));
        }
    }

    #[test]
    fn a_two_page_document_keeps_its_headers() {
        let pages: Vec<PageContent> = (1..=2u32)
            .map(|n| PageContent {
                number: n,
                ..page(line("Header text", 50.0, 20.0, 9.0, false))
            })
            .collect();
        let layouts = analyze(pages, Detection::Auto);
        assert_eq!(kinds(&layouts[0]).len(), 1);
    }

    #[test]
    fn list_markers_are_recognized_in_their_usual_forms() {
        assert_eq!(list_marker("• Item"), Some((false, "•".to_owned())));
        assert_eq!(list_marker("- Item"), Some((false, "-".to_owned())));
        assert_eq!(list_marker("3. Item"), Some((true, "3.".to_owned())));
        assert_eq!(list_marker("b) Item"), Some((true, "b)".to_owned())));
        assert_eq!(list_marker("(iv) Item"), Some((true, "(iv)".to_owned())));
        assert_eq!(list_marker("1.5 million euros"), None);
        assert_eq!(list_marker("Word. Sentence"), None);
        assert_eq!(list_marker("-5 degrees"), None);
        assert_eq!(list_marker(""), None);
    }

    #[test]
    fn body_stats_pick_the_most_common_size_and_the_larger_ones() {
        let mut words = line("Big title", 50.0, 10.0, 24.0, false);
        for i in 0..5 {
            words.extend(line(
                "ordinary running text of the page",
                50.0,
                100.0 + 13.0 * i as f32,
                11.0,
                false,
            ));
        }
        let stats = body_stats(&[page(words)]);
        assert_eq!(stats.body_size, 11.0);
        assert_eq!(stats.heading_sizes, [24.0]);
    }
}
