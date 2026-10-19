//! Finding tables: by the lines drawn around the cells (lattice) and by the white channels between
//! columns of text (stream).

use super::model::{Cell, Rect, Rule, Table, Word};
use super::words::Segment;

/// Strokes at most this thick are lines.
const THIN: f32 = 2.5;
/// Strokes shorter than this are marks, not borders.
const MIN_LENGTH: f32 = 8.0;
/// How far apart two strokes may be and still be one line or touch.
const TOLERANCE: f32 = 2.5;

/// A border line: `at` is the position across, `from` to `to` the extent along.
#[derive(Debug, Clone, Copy)]
struct Edge {
    horizontal: bool,
    at: f32,
    from: f32,
    to: f32,
}

fn edges_of(rules: &[Rule]) -> Vec<Edge> {
    let mut edges = Vec::new();
    for rule in rules {
        let r = rule.rect;
        let (w, h) = (r.width(), r.height());
        let horizontal = |y: f32| Edge {
            horizontal: true,
            at: y,
            from: r.x0,
            to: r.x1,
        };
        let vertical = |x: f32| Edge {
            horizontal: false,
            at: x,
            from: r.y0,
            to: r.y1,
        };
        if h <= THIN && w >= MIN_LENGTH {
            edges.push(horizontal(r.mid_y()));
        } else if w <= THIN && h >= MIN_LENGTH {
            edges.push(vertical(r.mid_x()));
        } else if w >= MIN_LENGTH && h >= MIN_LENGTH {
            // The outline of a cell, drawn as one rectangle.
            edges.extend([
                horizontal(r.y0),
                horizontal(r.y1),
                vertical(r.x0),
                vertical(r.x1),
            ]);
        }
    }
    edges
}

fn crosses(h: &Edge, v: &Edge) -> bool {
    v.at >= h.from - TOLERANCE
        && v.at <= h.to + TOLERANCE
        && h.at >= v.from - TOLERANCE
        && h.at <= v.to + TOLERANCE
}

/// Groups edges that touch each other, directly or through others.
fn components(edges: &[Edge]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..edges.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..edges.len() {
        for j in i + 1..edges.len() {
            let (a, b) = (&edges[i], &edges[j]);
            let touching = if a.horizontal == b.horizontal {
                // Two pieces of one line, or two lines laid on top of each other.
                (a.at - b.at).abs() <= TOLERANCE
                    && a.from <= b.to + TOLERANCE
                    && b.from <= a.to + TOLERANCE
            } else if a.horizontal {
                crosses(a, b)
            } else {
                crosses(b, a)
            };
            if touching {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                parent[ri] = rj;
            }
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    for i in 0..edges.len() {
        let root = find(&mut parent, i);
        match roots.iter().position(|r| *r == root) {
            Some(g) => groups[g].push(i),
            None => {
                roots.push(root);
                groups.push(vec![i]);
            }
        }
    }
    groups
}

/// Sorted positions with those closer than `tolerance` merged into their average.
fn cluster(mut values: Vec<f32>, tolerance: f32) -> Vec<f32> {
    values.sort_by(f32::total_cmp);
    let mut out: Vec<(f32, u32)> = Vec::new();
    for v in values {
        match out.last_mut() {
            Some((sum, n)) if v - *sum / *n as f32 <= tolerance => {
                *sum += v;
                *n += 1;
            }
            _ => out.push((v, 1)),
        }
    }
    out.into_iter().map(|(s, n)| s / n as f32).collect()
}

/// The words whose middle is inside `rect`, as text: lines top to bottom, words left to right.
fn text_in(words: &[Word], inside: &[usize]) -> (String, bool) {
    let mut members: Vec<&Word> = inside.iter().map(|i| &words[*i]).collect();
    members.sort_by(|a, b| a.rect.mid_y().total_cmp(&b.rect.mid_y()));
    let mut lines: Vec<Vec<&Word>> = Vec::new();
    for w in members {
        match lines.last_mut() {
            Some(line)
                if (w.rect.mid_y() - line[0].rect.mid_y()).abs() <= 0.5 * w.size.max(1.0) =>
            {
                line.push(w)
            }
            _ => lines.push(vec![w]),
        }
    }
    let mut text = Vec::new();
    for mut line in lines {
        line.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
        text.push(
            line.iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    let bold = !inside.is_empty() && inside.iter().all(|i| words[*i].bold);
    (text.join(" "), bold)
}

/// Tables drawn with lines, with the indexes of the words they hold.
pub fn lattice_tables(rules: &[Rule], words: &[Word]) -> Vec<(Table, Vec<usize>)> {
    let edges = edges_of(rules);
    let mut found = Vec::new();
    for group in components(&edges) {
        let members: Vec<&Edge> = group.iter().map(|i| &edges[*i]).collect();
        let xs = cluster(
            members
                .iter()
                .filter(|e| !e.horizontal)
                .map(|e| e.at)
                .collect(),
            TOLERANCE * 1.5,
        );
        let ys = cluster(
            members
                .iter()
                .filter(|e| e.horizontal)
                .map(|e| e.at)
                .collect(),
            TOLERANCE * 1.5,
        );
        if xs.len() < 2 || ys.len() < 2 {
            continue;
        }
        let (cols, rows) = (xs.len() - 1, ys.len() - 1);
        // A frame around the page or a lone box is not a table.
        if cols < 2 || rows * cols < 4 {
            continue;
        }
        let bounds = Rect::new(xs[0], ys[0], xs[cols], ys[rows]);
        let mut consumed = Vec::new();
        let mut table_rows = Vec::with_capacity(rows);
        for r in 0..rows {
            let mut cells = Vec::with_capacity(cols);
            for c in 0..cols {
                let cell = Rect::new(xs[c], ys[r], xs[c + 1], ys[r + 1]);
                let inside: Vec<usize> = words
                    .iter()
                    .enumerate()
                    .filter(|(_, w)| cell.contains(w.rect.mid_x(), w.rect.mid_y()))
                    .map(|(i, _)| i)
                    .collect();
                let (text, bold) = text_in(words, &inside);
                consumed.extend(inside);
                cells.push(Cell { text, bold });
            }
            table_rows.push(cells);
        }
        found.push((
            Table {
                rect: bounds,
                rows: table_rows,
                column_edges: xs[..cols].to_vec(),
                row_edges: ys[..rows].to_vec(),
            },
            consumed,
        ));
    }
    found.sort_by(|a, b| a.0.rect.y0.total_cmp(&b.0.rect.y0));
    found
}

/// Longest cell text, in characters, that still looks like a table cell rather than a column of
/// running text.
const MAX_CELL_CHARS: f32 = 40.0;
/// Fewest filled cells, as a fraction, for a grid of text to count as a table.
const MIN_FILL: f32 = 0.6;

/// Tables made of aligned columns of text, with the indexes of the segments they hold.
pub fn stream_tables(segments: &[Segment]) -> Vec<(Table, Vec<usize>)> {
    // Rows: the segments of one line.
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by(|a, b| {
        segments[*a]
            .rect
            .mid_y()
            .total_cmp(&segments[*b].rect.mid_y())
            .then(segments[*a].rect.x0.total_cmp(&segments[*b].rect.x0))
    });
    for i in order {
        match rows.last_mut() {
            Some(row) if segments[row[0]].line == segments[i].line => row.push(i),
            _ => rows.push(vec![i]),
        }
    }
    let mut found = Vec::new();
    let mut start = 0;
    while start < rows.len() {
        if rows[start].len() < 2 {
            start += 1;
            continue;
        }
        // The run of following rows that also have several segments and sit close below.
        let mut end = start + 1;
        while end < rows.len() && rows[end].len() >= 2 {
            let above = rows[end - 1]
                .iter()
                .map(|i| segments[*i].rect.y1)
                .fold(f32::MIN, f32::max);
            let below = rows[end]
                .iter()
                .map(|i| segments[*i].rect.y0)
                .fold(f32::MAX, f32::min);
            let size = segments[rows[end][0]].size.max(1.0);
            if below - above > 1.6 * size {
                break;
            }
            end += 1;
        }
        if end - start >= 2
            && let Some(found_table) = table_from_rows(segments, &rows[start..end])
        {
            found.push(found_table);
        }
        start = end;
    }
    found
}

fn table_from_rows(segments: &[Segment], rows: &[Vec<usize>]) -> Option<(Table, Vec<usize>)> {
    let all: Vec<usize> = rows.iter().flatten().copied().collect();
    let sizes = all.iter().map(|i| segments[*i].size).sum::<f32>() / all.len() as f32;
    // White channels: x ranges that no segment of any row covers.
    let left = all
        .iter()
        .map(|i| segments[*i].rect.x0)
        .fold(f32::MAX, f32::min);
    let right = all
        .iter()
        .map(|i| segments[*i].rect.x1)
        .fold(f32::MIN, f32::max);
    let width = (right - left).ceil() as usize + 2;
    let mut covered = vec![false; width];
    for i in &all {
        let r = segments[*i].rect;
        let from = ((r.x0 - left).floor().max(0.0)) as usize;
        let to = (((r.x1 - left).ceil()) as usize).min(width - 1);
        for slot in &mut covered[from..=to] {
            *slot = true;
        }
    }
    let min_channel = (sizes * 0.9).max(8.0) as usize;
    let mut columns: Vec<(f32, f32)> = Vec::new();
    let mut x = 0;
    while x < width {
        if !covered[x] {
            x += 1;
            continue;
        }
        let begin = x;
        // Extend through covered space and through channels narrower than a column gap.
        let mut last = x;
        let mut probe = x;
        while probe < width {
            if covered[probe] {
                last = probe;
                probe += 1;
            } else {
                let gap = covered[probe..].iter().take_while(|c| !**c).count();
                if gap >= min_channel || probe + gap >= width {
                    break;
                }
                probe += gap;
            }
        }
        columns.push((left + begin as f32, left + last as f32));
        x = last + 1;
    }
    if columns.len() < 2 {
        return None;
    }
    // Every segment must sit inside one column, or the rows are not a grid.
    let column_of = |s: &Segment| {
        columns
            .iter()
            .position(|(a, b)| s.rect.x0 >= *a - 1.0 && s.rect.x1 <= *b + 1.0)
    };
    let mut grid: Vec<Vec<Vec<usize>>> = vec![vec![Vec::new(); columns.len()]; rows.len()];
    for (r, row) in rows.iter().enumerate() {
        for i in row {
            let c = column_of(&segments[*i])?;
            grid[r][c].push(*i);
        }
    }
    let cells: usize = grid.iter().flatten().count();
    let filled = grid.iter().flatten().filter(|c| !c.is_empty()).count();
    let chars: usize = grid
        .iter()
        .flatten()
        .flatten()
        .map(|i| segments[*i].text().chars().count())
        .sum();
    if (filled as f32) < MIN_FILL * cells as f32
        || (chars as f32 / filled.max(1) as f32) > MAX_CELL_CHARS
    {
        return None;
    }
    let table_rows: Vec<Vec<Cell>> = grid
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| Cell {
                    text: cell
                        .iter()
                        .map(|i| segments[*i].text())
                        .collect::<Vec<_>>()
                        .join(" "),
                    bold: !cell.is_empty() && cell.iter().all(|i| segments[*i].bold),
                })
                .collect()
        })
        .collect();
    let rect = all
        .iter()
        .map(|i| segments[*i].rect)
        .reduce(|a, b| a.union(&b))?;
    let row_edges = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|i| segments[*i].rect.y0)
                .fold(f32::MAX, f32::min)
        })
        .collect();
    Some((
        Table {
            rect,
            rows: table_rows,
            column_edges: columns.iter().map(|(a, _)| *a).collect(),
            row_edges,
        },
        all,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::words::segments;

    fn bar(x0: f32, y0: f32, x1: f32, y1: f32) -> Rule {
        Rule {
            rect: Rect::new(x0, y0, x1, y1),
        }
    }

    fn word(text: &str, x: f32, y: f32) -> Word {
        Word {
            text: text.to_owned(),
            rect: Rect::new(x, y, x + 6.0 * text.chars().count() as f32, y + 12.0),
            size: 12.0,
            bold: false,
            italic: false,
        }
    }

    /// A grid of thin bars: `cols` by `rows` cells of 100 by 30 points from (50, 100).
    fn grid(cols: usize, rows: usize) -> Vec<Rule> {
        let mut rules = Vec::new();
        for r in 0..=rows {
            let y = 100.0 + 30.0 * r as f32;
            rules.push(bar(50.0, y - 0.5, 50.0 + 100.0 * cols as f32, y + 0.5));
        }
        for c in 0..=cols {
            let x = 50.0 + 100.0 * c as f32;
            rules.push(bar(x - 0.5, 100.0, x + 0.5, 100.0 + 30.0 * rows as f32));
        }
        rules
    }

    #[test]
    fn a_grid_of_lines_becomes_a_table_with_the_words_in_its_cells() {
        let words = vec![
            word("Name", 60.0, 110.0),
            word("Qty", 160.0, 110.0),
            word("Pens", 60.0, 140.0),
            word("12", 160.0, 140.0),
            word("Paper", 60.0, 170.0),
            word("7", 160.0, 170.0),
            word("Outside", 60.0, 300.0),
        ];
        let tables = lattice_tables(&grid(2, 3), &words);
        assert_eq!(tables.len(), 1);
        let (table, consumed) = &tables[0];
        assert_eq!((table.rows.len(), table.columns()), (3, 2));
        let text: Vec<Vec<&str>> = table
            .rows
            .iter()
            .map(|r| r.iter().map(|c| c.text.as_str()).collect())
            .collect();
        assert_eq!(text, [["Name", "Qty"], ["Pens", "12"], ["Paper", "7"]]);
        assert_eq!(consumed.len(), 6, "the word below the table is not taken");
        assert!((table.rect.width() - 200.0).abs() < 1.0);
        assert_eq!(table.column_edges.len(), 2);
    }

    #[test]
    fn cells_drawn_as_rectangles_make_a_table_too() {
        let mut rules = Vec::new();
        for r in 0..2 {
            for c in 0..2 {
                rules.push(bar(
                    50.0 + 100.0 * c as f32,
                    100.0 + 30.0 * r as f32,
                    150.0 + 100.0 * c as f32,
                    130.0 + 30.0 * r as f32,
                ));
            }
        }
        let tables = lattice_tables(&rules, &[word("a", 60.0, 110.0), word("d", 160.0, 140.0)]);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].0.rows[0][0].text, "a");
        assert_eq!(tables[0].0.rows[1][1].text, "d");
    }

    #[test]
    fn a_frame_a_lone_box_and_underlines_are_not_tables() {
        // A rectangle around the page.
        assert!(lattice_tables(&[bar(20.0, 20.0, 580.0, 770.0)], &[]).is_empty());
        // One row of one cell.
        assert!(lattice_tables(&grid(1, 1), &[]).is_empty());
        // A heading underline.
        assert!(lattice_tables(&[bar(50.0, 100.0, 500.0, 101.0)], &[]).is_empty());
    }

    #[test]
    fn two_separate_grids_are_two_tables() {
        let mut rules = grid(2, 2);
        rules.extend(grid(2, 2).into_iter().map(|r| Rule {
            rect: Rect::new(r.rect.x0, r.rect.y0 + 300.0, r.rect.x1, r.rect.y1 + 300.0),
        }));
        assert_eq!(lattice_tables(&rules, &[]).len(), 2);
    }

    fn row(y: f32, cells: &[(&str, f32)]) -> Vec<Word> {
        cells.iter().map(|(t, x)| word(t, *x, y)).collect()
    }

    #[test]
    fn aligned_columns_of_short_text_become_a_stream_table() {
        let mut words = Vec::new();
        words.extend(row(
            100.0,
            &[("Item", 50.0), ("Price", 250.0), ("Stock", 400.0)],
        ));
        words.extend(row(
            116.0,
            &[("Pens", 50.0), ("1.20", 250.0), ("300", 400.0)],
        ));
        words.extend(row(
            132.0,
            &[("Paper", 50.0), ("4.75", 250.0), ("45", 400.0)],
        ));
        let segs = segments(&words);
        let tables = stream_tables(&segs);
        assert_eq!(tables.len(), 1);
        let (table, taken) = &tables[0];
        assert_eq!((table.rows.len(), table.columns()), (3, 3));
        assert_eq!(table.rows[1][1].text, "1.20");
        assert_eq!(taken.len(), 9);
    }

    #[test]
    fn right_aligned_numbers_still_fall_into_their_columns() {
        let mut words = Vec::new();
        // Numbers of different lengths end at the same right edge.
        words.extend(row(100.0, &[("Total", 50.0), ("5", 394.0)]));
        words.extend(row(116.0, &[("Subtotal", 50.0), ("120", 382.0)]));
        words.extend(row(132.0, &[("Tax", 50.0), ("1,250", 370.0)]));
        let tables = stream_tables(&segments(&words));
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].0.rows[2][1].text, "1,250");
    }

    #[test]
    fn two_columns_of_running_text_are_not_a_table() {
        let left = "The committee reviewed the proposal and asked for changes";
        let right = "Members agreed to meet again before the end of the month";
        let mut words = Vec::new();
        for i in 0..4 {
            let y = 100.0 + 16.0 * i as f32;
            words.push(word(left, 50.0, y));
            words.push(word(right, 330.0, y));
        }
        // Wide text: each side is one long segment.
        assert!(stream_tables(&segments(&words)).is_empty());
    }

    #[test]
    fn a_single_row_of_segments_is_not_a_table() {
        let words = row(100.0, &[("a", 50.0), ("b", 250.0)]);
        assert!(stream_tables(&segments(&words)).is_empty());
    }
}
