//! Pure planning rules for the Organize tools: how pages are grouped, interleaved, packed and
//! duplicated. No file or engine is involved, so every rule is tested with plain numbers.

use crate::tools::common::FractionBox;

/// A run of consecutive pages: zero-based, `start` included and `end` excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageSpan {
    /// First page.
    pub start: u32,
    /// One past the last page.
    pub end: u32,
}

impl PageSpan {
    /// Number of pages in the span.
    pub const fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    /// True when the span holds no pages.
    pub const fn is_empty(self) -> bool {
        self.end <= self.start
    }

    /// The page indices of the span.
    pub fn pages(self) -> impl Iterator<Item = u32> {
        self.start..self.end
    }
}

/// Splits `page_count` pages into runs of `n` (the last run may be shorter). `n` of 0 is treated
/// as 1 so the function is total; option validation rejects 0 before this is reached.
pub fn every_n(page_count: u32, n: u32) -> Vec<PageSpan> {
    let n = n.max(1);
    let mut spans = Vec::new();
    let mut start = 0;
    while start < page_count {
        let end = start.saturating_add(n).min(page_count);
        spans.push(PageSpan { start, end });
        start = end;
    }
    spans
}

/// Takes `per_turn` pages from each document in turn until all are used. A document that runs
/// out is skipped. `reverse[i]` reads document `i` from its last page backwards, which merges the
/// two passes of a duplex scan. Returns `(document, page)` pairs.
pub fn interleave(counts: &[u32], per_turn: u32, reverse: &[bool]) -> Vec<(usize, u32)> {
    let per_turn = per_turn.max(1);
    let mut next = vec![0u32; counts.len()];
    let total: u64 = counts.iter().map(|c| u64::from(*c)).sum();
    let mut out = Vec::with_capacity(usize::try_from(total).unwrap_or(0));
    while out.len() < usize::try_from(total).unwrap_or(usize::MAX) {
        for (doc, &count) in counts.iter().enumerate() {
            let taken = next.get(doc).copied().unwrap_or(0);
            let take = per_turn.min(count.saturating_sub(taken));
            for k in 0..take {
                let position = taken + k;
                let page = if reverse.get(doc).copied().unwrap_or(false) {
                    count - 1 - position
                } else {
                    position
                };
                out.push((doc, page));
            }
            if let Some(slot) = next.get_mut(doc) {
                *slot = taken + take;
            }
        }
    }
    out
}

/// Packs pages greedily into runs whose estimated sizes stay within `limit`. A page whose own
/// estimate exceeds the limit gets a run to itself; callers report that as a warning.
pub fn pack_by_size(estimates: &[u64], limit: u64) -> Vec<PageSpan> {
    let mut spans = Vec::new();
    let mut start = 0u32;
    let mut sum = 0u64;
    for (i, &size) in estimates.iter().enumerate() {
        let index = u32::try_from(i).unwrap_or(u32::MAX);
        if index > start && sum.saturating_add(size) > limit {
            spans.push(PageSpan { start, end: index });
            start = index;
            sum = 0;
        }
        sum = sum.saturating_add(size);
    }
    let end = u32::try_from(estimates.len()).unwrap_or(u32::MAX);
    if end > start {
        spans.push(PageSpan { start, end });
    }
    spans
}

/// Splits a run in two halves, for the verify loop that shrinks a run that came out too big.
pub fn halve(span: PageSpan) -> Option<(PageSpan, PageSpan)> {
    (span.len() > 1).then(|| {
        let mid = span.start + span.len() / 2;
        (
            PageSpan {
                start: span.start,
                end: mid,
            },
            PageSpan {
                start: mid,
                end: span.end,
            },
        )
    })
}

/// Groups pages by the text found in a region on each page.
///
/// Without `starts_with`, a new group begins whenever the region text changes; a page with no
/// text there continues the previous group. With `starts_with`, a group begins only on pages whose
/// text starts with that prefix (ignoring case). Returns each span with the text of its first
/// page, which names the output file.
pub fn group_by_text(texts: &[String], starts_with: Option<&str>) -> Vec<(PageSpan, String)> {
    let prefix = starts_with
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty());
    let mut starts: Vec<u32> = Vec::new();
    let mut previous = String::new();
    for (i, text) in texts.iter().enumerate() {
        let text = text.trim();
        let index = u32::try_from(i).unwrap_or(u32::MAX);
        // A group starts when the text changes, and with a filter only if it also matches.
        let changed = !text.is_empty() && text != previous;
        let begins = changed
            && prefix
                .as_ref()
                .is_none_or(|p| text.to_lowercase().starts_with(p));
        if (i == 0 || begins) && starts.last() != Some(&index) {
            starts.push(index);
        }
        if !text.is_empty() {
            previous = text.to_owned();
        }
    }
    spans_from_starts(&starts, texts.len())
        .into_iter()
        .map(|span| {
            let label = texts
                .get(span.start as usize)
                .map_or(String::new(), |t| t.trim().to_owned());
            (span, label)
        })
        .collect()
}

fn spans_from_starts(starts: &[u32], page_count: usize) -> Vec<PageSpan> {
    let end_all = u32::try_from(page_count).unwrap_or(u32::MAX);
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| PageSpan {
            start,
            end: starts.get(i + 1).copied().unwrap_or(end_all),
        })
        .collect()
}

/// A bookmark as `(title, zero-based page, level)`, where level 1 is the top level.
pub type OutlineTriple = (String, u32, u32);

/// Splits at bookmarks of one outline `level`. Pages before the first such bookmark form a leading
/// run titled by `lead_title`. Bookmarks that point at the same page start one run, keeping the
/// first title. Bookmarks beyond the last page are ignored.
pub fn bookmark_groups(
    items: &[OutlineTriple],
    level: u32,
    page_count: u32,
    lead_title: &str,
) -> Vec<(PageSpan, String)> {
    let mut picked: Vec<(u32, &str)> = items
        .iter()
        .filter(|(_, page, l)| *l == level && *page < page_count)
        .map(|(title, page, _)| (*page, title.as_str()))
        .collect();
    picked.sort_by_key(|(page, _)| *page);
    picked.dedup_by_key(|(page, _)| *page);

    let mut groups = Vec::new();
    let first = picked.first().map_or(page_count, |(p, _)| *p);
    if first > 0 && page_count > 0 {
        groups.push((
            PageSpan {
                start: 0,
                end: first,
            },
            lead_title.to_owned(),
        ));
    }
    for (i, (page, title)) in picked.iter().enumerate() {
        let end = picked.get(i + 1).map_or(page_count, |(p, _)| *p);
        groups.push((PageSpan { start: *page, end }, (*title).to_owned()));
    }
    groups
}

/// Direction of the cut for Split in half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutDirection {
    /// Cut along a vertical line: left and right halves.
    Vertical,
    /// Cut along a horizontal line: top and bottom halves.
    Horizontal,
}

/// One output page of Split in half: a source page and the crop that selects its half.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HalfStep {
    /// Zero-based source page.
    pub page: u32,
    /// Crop for this output page, or `None` to keep the page whole (skipped pages).
    pub crop: Option<FractionBox>,
}

/// Plans Split in half. Each page becomes two pages unless skipped. For a vertical cut the order
/// is left then right, or right then left with `right_to_left`; a horizontal cut is top then
/// bottom.
pub fn half_plan(
    page_count: u32,
    direction: CutDirection,
    skip_first: bool,
    skip_last: bool,
    right_to_left: bool,
) -> Vec<HalfStep> {
    let (first, second) = match direction {
        CutDirection::Vertical => {
            let left = FractionBox {
                left: 0.0,
                bottom: 0.0,
                right: 0.5,
                top: 1.0,
            };
            let right = FractionBox {
                left: 0.5,
                bottom: 0.0,
                right: 1.0,
                top: 1.0,
            };
            if right_to_left {
                (right, left)
            } else {
                (left, right)
            }
        }
        CutDirection::Horizontal => (
            FractionBox {
                left: 0.0,
                bottom: 0.5,
                right: 1.0,
                top: 1.0,
            },
            FractionBox {
                left: 0.0,
                bottom: 0.0,
                right: 1.0,
                top: 0.5,
            },
        ),
    };
    let mut steps = Vec::new();
    for page in 0..page_count {
        let skipped = (skip_first && page == 0) || (skip_last && page + 1 == page_count);
        if skipped {
            steps.push(HalfStep { page, crop: None });
        } else {
            steps.push(HalfStep {
                page,
                crop: Some(first),
            });
            steps.push(HalfStep {
                page,
                crop: Some(second),
            });
        }
    }
    steps
}

/// What to insert for Insert blank or duplicate pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertKind {
    /// New blank pages.
    Blank,
    /// Copies of the selected page.
    Duplicate,
}

/// Where blank pages go relative to the selected page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertPosition {
    /// Before the selected page.
    Before,
    /// After the selected page.
    After,
}

/// One output page of an insertion plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertStep {
    /// An original page, in place.
    Original(u32),
    /// A blank page sized like the given neighbor page.
    Blank {
        /// The page whose size the blank page takes.
        neighbor: u32,
    },
    /// A copy of the given page.
    Copy(u32),
}

/// Plans the output page order for inserting `count` blank pages or copies around each selected
/// page. `selected` holds zero-based pages and must be in range.
pub fn insert_plan(
    page_count: u32,
    selected: &[u32],
    kind: InsertKind,
    position: InsertPosition,
    count: u32,
) -> Vec<InsertStep> {
    let mut steps = Vec::new();
    for page in 0..page_count {
        let hit = selected.contains(&page);
        let extras = |steps: &mut Vec<InsertStep>| {
            for _ in 0..count {
                steps.push(match kind {
                    InsertKind::Blank => InsertStep::Blank { neighbor: page },
                    InsertKind::Duplicate => InsertStep::Copy(page),
                });
            }
        };
        if hit && kind == InsertKind::Blank && position == InsertPosition::Before {
            extras(&mut steps);
        }
        steps.push(InsertStep::Original(page));
        if hit && (kind == InsertKind::Duplicate || position == InsertPosition::After) {
            extras(&mut steps);
        }
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn span(start: u32, end: u32) -> PageSpan {
        PageSpan { start, end }
    }

    #[test]
    fn every_n_covers_all_pages_in_order() {
        assert_eq!(every_n(7, 3), vec![span(0, 3), span(3, 6), span(6, 7)]);
        assert_eq!(every_n(3, 1), vec![span(0, 1), span(1, 2), span(2, 3)]);
        assert_eq!(every_n(2, 10), vec![span(0, 2)]);
        assert!(every_n(0, 3).is_empty());
        assert_eq!(every_n(2, 0).len(), 2);
    }

    #[test]
    fn interleave_alternates_and_handles_unequal_lengths() {
        assert_eq!(
            interleave(&[3, 3], 1, &[]),
            vec![(0, 0), (1, 0), (0, 1), (1, 1), (0, 2), (1, 2)]
        );
        assert_eq!(
            interleave(&[2, 4], 1, &[]),
            vec![(0, 0), (1, 0), (0, 1), (1, 1), (1, 2), (1, 3)]
        );
        assert_eq!(interleave(&[4, 4], 2, &[]).len(), 8);
        assert_eq!(
            &interleave(&[4, 4], 2, &[])[..4],
            &[(0, 0), (0, 1), (1, 0), (1, 1)]
        );
    }

    #[test]
    fn interleave_reverse_merges_duplex_scans() {
        // Front sides in order, back sides scanned in reverse: 1a 2b' ... gives a b a b.
        let out = interleave(&[3, 3], 1, &[false, true]);
        assert_eq!(out, vec![(0, 0), (1, 2), (0, 1), (1, 1), (0, 2), (1, 0)]);
    }

    #[test]
    fn pack_by_size_is_greedy_and_isolates_big_pages() {
        assert_eq!(
            pack_by_size(&[4, 4, 4, 4], 10),
            vec![span(0, 2), span(2, 4)]
        );
        assert_eq!(
            pack_by_size(&[4, 20, 4], 10),
            vec![span(0, 1), span(1, 2), span(2, 3)]
        );
        assert_eq!(pack_by_size(&[1, 1, 1], 100), vec![span(0, 3)]);
        assert!(pack_by_size(&[], 10).is_empty());
    }

    #[test]
    fn halving_splits_a_run_until_one_page() {
        assert_eq!(halve(span(0, 5)), Some((span(0, 2), span(2, 5))));
        assert_eq!(halve(span(3, 4)), None);
    }

    #[test]
    fn text_groups_start_when_the_region_text_changes() {
        let t = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let g = group_by_text(&t(&["INV-1", "INV-1", "", "INV-2", "INV-3"]), None);
        assert_eq!(
            g,
            vec![
                (span(0, 3), "INV-1".to_owned()),
                (span(3, 4), "INV-2".to_owned()),
                (span(4, 5), "INV-3".to_owned()),
            ]
        );
    }

    #[test]
    fn text_prefix_filter_starts_groups_only_on_matches() {
        let t = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let g = group_by_text(
            &t(&["cover", "Chapter 1", "x", "chapter 2", "y"]),
            Some("chapter"),
        );
        assert_eq!(
            g,
            vec![
                (span(0, 1), "cover".to_owned()),
                (span(1, 3), "Chapter 1".to_owned()),
                (span(3, 5), "chapter 2".to_owned()),
            ]
        );
    }

    #[test]
    fn a_prefix_filter_does_not_split_repeated_text() {
        let t = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let g = group_by_text(&t(&["INV-1", "INV-2", "INV-2", "INV-3"]), Some("inv-2"));
        assert_eq!(g.len(), 2, "{g:?}");
        assert_eq!((g[1].0.start, g[1].0.end), (1, 4));
    }

    #[test]
    fn bookmark_groups_pick_a_level_and_keep_a_lead_run() {
        let items: Vec<OutlineTriple> = vec![
            ("Intro".into(), 2, 1),
            ("Sub".into(), 3, 2),
            ("Part".into(), 5, 1),
            ("Part again".into(), 5, 1),
            ("Out of range".into(), 99, 1),
        ];
        let g = bookmark_groups(&items, 1, 8, "Start");
        assert_eq!(
            g,
            vec![
                (span(0, 2), "Start".to_owned()),
                (span(2, 5), "Intro".to_owned()),
                (span(5, 8), "Part".to_owned()),
            ]
        );
        assert_eq!(bookmark_groups(&items, 2, 8, "Start").len(), 2);
        assert_eq!(
            bookmark_groups(&[], 1, 4, "Start"),
            vec![(span(0, 4), "Start".to_owned())]
        );
    }

    #[test]
    fn half_plans_order_and_skip() {
        let plan = half_plan(3, CutDirection::Vertical, true, true, false);
        assert_eq!(plan.len(), 2 + 1 + 1);
        assert_eq!(
            plan[0],
            HalfStep {
                page: 0,
                crop: None
            }
        );
        assert_eq!(plan[1].crop.unwrap().left, 0.0);
        assert_eq!(plan[2].crop.unwrap().left, 0.5);
        assert_eq!(
            plan[3],
            HalfStep {
                page: 2,
                crop: None
            }
        );

        let rtl = half_plan(1, CutDirection::Vertical, false, false, true);
        assert_eq!(rtl[0].crop.unwrap().left, 0.5);
        let horizontal = half_plan(1, CutDirection::Horizontal, false, false, false);
        assert_eq!(horizontal[0].crop.unwrap().bottom, 0.5, "top half first");
    }

    #[test]
    fn insert_plans_place_blanks_and_copies() {
        use InsertStep::{Blank, Copy, Original};
        let after = insert_plan(3, &[1], InsertKind::Blank, InsertPosition::After, 2);
        assert_eq!(
            after,
            vec![
                Original(0),
                Original(1),
                Blank { neighbor: 1 },
                Blank { neighbor: 1 },
                Original(2)
            ]
        );
        let before = insert_plan(2, &[0], InsertKind::Blank, InsertPosition::Before, 1);
        assert_eq!(
            before,
            vec![Blank { neighbor: 0 }, Original(0), Original(1)]
        );
        let dup = insert_plan(2, &[1], InsertKind::Duplicate, InsertPosition::After, 2);
        assert_eq!(dup, vec![Original(0), Original(1), Copy(1), Copy(1)]);
    }

    proptest! {
        #[test]
        fn every_n_partitions_exactly(pages in 0u32..500, n in 1u32..50) {
            let spans = every_n(pages, n);
            let total: u32 = spans.iter().map(|s| s.len()).sum();
            prop_assert_eq!(total, pages);
            prop_assert!(spans.iter().all(|s| !s.is_empty() && s.len() <= n));
            prop_assert!(spans.windows(2).all(|w| w[0].end == w[1].start));
        }

        #[test]
        fn interleave_uses_every_page_once(
            counts in proptest::collection::vec(0u32..20, 1..5),
            per in 1u32..5,
            rev in proptest::collection::vec(any::<bool>(), 0..5),
        ) {
            let out = interleave(&counts, per, &rev);
            prop_assert_eq!(out.len() as u32, counts.iter().sum::<u32>());
            for (doc, count) in counts.iter().enumerate() {
                let mut pages: Vec<u32> = out.iter().filter(|(d, _)| *d == doc).map(|(_, p)| *p).collect();
                pages.sort_unstable();
                prop_assert_eq!(pages, (0..*count).collect::<Vec<_>>());
            }
        }

        #[test]
        fn packing_never_loses_or_reorders_pages(
            sizes in proptest::collection::vec(0u64..100, 0..60),
            limit in 1u64..200,
        ) {
            let spans = pack_by_size(&sizes, limit);
            prop_assert_eq!(spans.iter().map(|s| s.len()).sum::<u32>() as usize, sizes.len());
            prop_assert!(spans.windows(2).all(|w| w[0].end == w[1].start));
            for s in &spans {
                let sum: u64 = s.pages().map(|p| sizes[p as usize]).sum();
                prop_assert!(sum <= limit || s.len() == 1);
            }
        }
    }
}
