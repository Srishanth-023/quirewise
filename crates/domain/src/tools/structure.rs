//! Options of the structure tools: Bookmarks editor (row 49), Rename by content (row 50),
//! Attachments (row 65) and Page labels (row 66).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::common::Region;
use crate::PageIndex;
use crate::error::DomainError;
use crate::naming::OutputNamingPattern;
use crate::stamp::NumberStyle;

const MAX_TITLE_CHARS: usize = 500;
const MAX_BOOKMARKS: usize = 20_000;
const MAX_LEVEL: u32 = 32;

/// One bookmark.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BookmarkEntry {
    /// The text in the bookmark panel.
    pub title: String,
    /// The page it jumps to, counting from 1.
    pub page: u32,
    /// Nesting depth, 1 for the top level.
    pub level: u32,
}

/// Options for the Bookmarks editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct BookmarksOptions {
    /// The complete bookmark list of the result, replacing the current one.
    pub bookmarks: Vec<BookmarkEntry>,
    /// Build the list from the large text of the document instead. Default off.
    pub auto_generate: bool,
    /// Deepest level of generated bookmarks, 1 to 6. Default 3.
    pub auto_max_level: u8,
}

impl Default for BookmarksOptions {
    fn default() -> Self {
        Self {
            bookmarks: Vec::new(),
            auto_generate: false,
            auto_max_level: 3,
        }
    }
}

/// Checks a bookmark list against the document and returns it with zero-based pages.
pub fn check_bookmarks(
    entries: &[BookmarkEntry],
    page_count: u32,
) -> Result<Vec<(String, PageIndex, u32)>, DomainError> {
    if entries.len() > MAX_BOOKMARKS {
        return Err(DomainError::option(
            "bookmarks",
            format!("at most {MAX_BOOKMARKS} bookmarks"),
        ));
    }
    let mut previous = 0u32;
    let mut out = Vec::with_capacity(entries.len());
    for (i, entry) in entries.iter().enumerate() {
        let row = i + 1;
        let title = entry.title.trim();
        if title.is_empty() {
            return Err(DomainError::option(
                "bookmarks",
                format!("bookmark {row} has no title"),
            ));
        }
        if title.chars().count() > MAX_TITLE_CHARS {
            return Err(DomainError::option(
                "bookmarks",
                format!("bookmark {row}: the title is longer than {MAX_TITLE_CHARS} characters"),
            ));
        }
        if entry.page == 0 || entry.page > page_count {
            return Err(DomainError::option(
                "bookmarks",
                format!(
                    "bookmark {row} points at page {}, but the document has {page_count} pages",
                    entry.page
                ),
            ));
        }
        if entry.level == 0 || entry.level > MAX_LEVEL || entry.level > previous + 1 {
            return Err(DomainError::option(
                "bookmarks",
                format!(
                    "bookmark {row} is nested too deep: a bookmark can be at most one level below the one before it"
                ),
            ));
        }
        previous = entry.level;
        out.push((
            title.to_owned(),
            PageIndex::new(entry.page - 1),
            entry.level,
        ));
    }
    Ok(out)
}

impl BookmarksOptions {
    /// Checks the options. A manual list needs entries; generated lists need a sensible depth.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.auto_generate {
            return if (1..=6).contains(&self.auto_max_level) {
                Ok(())
            } else {
                Err(DomainError::option(
                    "auto_max_level",
                    "must be between 1 and 6",
                ))
            };
        }
        if self.bookmarks.is_empty() {
            return Err(DomainError::option(
                "bookmarks",
                "add at least one bookmark, or turn on auto-generate",
            ));
        }
        Ok(())
    }
}

/// A line of text read from a page, with its font size.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    /// The page, zero-based.
    pub page: PageIndex,
    /// The text of the line.
    pub text: String,
    /// Font size in points.
    pub size_pt: f32,
}

/// Picks the headings out of the lines of a document: lines set clearly larger than the body
/// text, deepest level for the smallest size. Text that repeats on many pages (running heads)
/// is ignored.
pub fn detect_headings(lines: &[TextLine], page_count: u32, max_level: u8) -> Vec<BookmarkEntry> {
    // Sizes in half points, so 11.98 and 12.02 count as one size.
    let half = |size: f32| (size * 2.0).round() as i64;
    let mut weight: HashMap<i64, usize> = HashMap::new();
    for line in lines {
        *weight.entry(half(line.size_pt)).or_default() += line.text.chars().count();
    }
    let Some((&body, _)) = weight.iter().max_by_key(|(size, chars)| (**chars, **size)) else {
        return Vec::new();
    };
    // A heading is at least 15% and one point larger than the body text.
    let floor = (body as f32 / 2.0 * 1.15).max(body as f32 / 2.0 + 1.0);

    let mut pages_of_text: HashMap<&str, std::collections::HashSet<u32>> = HashMap::new();
    for line in lines {
        pages_of_text
            .entry(line.text.trim())
            .or_default()
            .insert(line.page.get());
    }
    let repeats = |text: &str| {
        let pages = pages_of_text.get(text).map_or(0, |p| p.len());
        pages >= 3 && pages * 5 >= page_count as usize * 2
    };

    let heading = |line: &&TextLine| {
        let text = line.text.trim();
        let chars = text.chars().count();
        line.size_pt >= floor
            && (2..=120).contains(&chars)
            && text.chars().any(char::is_alphabetic)
            && !repeats(text)
    };
    let candidates: Vec<&TextLine> = lines.iter().filter(heading).collect();
    let mut sizes: Vec<i64> = candidates.iter().map(|l| half(l.size_pt)).collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes.dedup();

    candidates
        .into_iter()
        .filter_map(|line| {
            let level = sizes.iter().position(|s| *s == half(line.size_pt))? + 1;
            (level <= usize::from(max_level)).then(|| BookmarkEntry {
                title: line.text.trim().to_owned(),
                page: line.page.get() + 1,
                level: u32::try_from(level).unwrap_or(1),
            })
        })
        .collect()
}

/// Options for Rename by content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RenameByContentOptions {
    /// The box that holds the text to read, drawn on the page preview.
    pub region: Option<Region>,
    /// The page to read, counting from 1. Default 1.
    pub page: u32,
    /// The new name, with `{text}` for the text read and `{filename}`, `{date}` and the other
    /// naming tokens. Default `{text}`.
    pub pattern: String,
}

impl Default for RenameByContentOptions {
    fn default() -> Self {
        Self {
            region: None,
            page: 1,
            pattern: "{text}".to_owned(),
        }
    }
}

impl RenameByContentOptions {
    /// Checks the options and returns the region and the parsed pattern.
    pub fn checked(&self) -> Result<(Region, OutputNamingPattern), DomainError> {
        let Some(region) = self.region else {
            return Err(DomainError::option(
                "region",
                "draw a box around the text to use as the new name",
            ));
        };
        region.validate()?;
        if self.page == 0 {
            return Err(DomainError::option("page", "pages count from 1"));
        }
        if !self.pattern.contains("{text}") {
            return Err(DomainError::option(
                "pattern",
                "the pattern must contain {text}, or every file would get the same name",
            ));
        }
        Ok((region, OutputNamingPattern::parse(&self.pattern)?))
    }
}

/// What Attachments does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AttachmentAction {
    /// Write a text file that lists the attachments (default).
    #[default]
    List,
    /// Attach files to the document.
    Add,
    /// Save attachments as files.
    Extract,
    /// Remove attachments from the document.
    Delete,
}

/// Options for Attachments.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AttachmentsOptions {
    /// What to do. Default list.
    pub action: AttachmentAction,
    /// Paths of the files to attach, for add.
    pub files: Vec<String>,
    /// Names of the attachments to extract (none means all) or to delete.
    pub names: Vec<String>,
}

impl AttachmentsOptions {
    /// Checks the options for the chosen action.
    pub fn validate(&self) -> Result<(), DomainError> {
        match self.action {
            AttachmentAction::List => Ok(()),
            AttachmentAction::Extract => Ok(()),
            AttachmentAction::Add => {
                if self.files.iter().all(|f| f.trim().is_empty()) {
                    Err(DomainError::option("files", "choose the files to attach"))
                } else {
                    Ok(())
                }
            }
            AttachmentAction::Delete => {
                if self.names.iter().all(|n| n.trim().is_empty()) {
                    Err(DomainError::option(
                        "names",
                        "name the attachments to remove",
                    ))
                } else {
                    Ok(())
                }
            }
        }
    }

    /// The non-blank names.
    pub fn name_list(&self) -> Vec<&str> {
        self.names
            .iter()
            .map(|n| n.trim())
            .filter(|n| !n.is_empty())
            .collect()
    }

    /// The non-blank file paths.
    pub fn file_list(&self) -> Vec<&str> {
        self.files
            .iter()
            .map(|f| f.trim())
            .filter(|f| !f.is_empty())
            .collect()
    }
}

/// One run of pages that share a numbering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct LabelRange {
    /// The first page of the run, counting from 1.
    pub from_page: u32,
    /// How the numbers are written. Default arabic.
    pub style: NumberStyle,
    /// Text before each number, such as `A-`. Default none.
    pub prefix: String,
    /// The number of the first page of the run. Default 1.
    pub start: u32,
}

impl Default for LabelRange {
    fn default() -> Self {
        Self {
            from_page: 1,
            style: NumberStyle::Arabic,
            prefix: String::new(),
            start: 1,
        }
    }
}

/// Options for Page labels.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PageLabelsOptions {
    /// The runs, each starting at a page. Pages before the first run are labeled 1, 2, 3.
    pub ranges: Vec<LabelRange>,
    /// Remove all page labels instead. Default off.
    pub remove_all: bool,
}

impl PageLabelsOptions {
    /// Checks the runs against the page count and returns them in page order.
    pub fn checked(&self, page_count: u32) -> Result<Vec<LabelRange>, DomainError> {
        if self.remove_all {
            return Ok(Vec::new());
        }
        if self.ranges.is_empty() {
            return Err(DomainError::option(
                "ranges",
                "add a range, or turn on remove all",
            ));
        }
        let mut ranges = self.ranges.clone();
        ranges.sort_by_key(|r| r.from_page);
        for (i, r) in ranges.iter().enumerate() {
            if r.from_page == 0 || r.from_page > page_count {
                return Err(DomainError::option(
                    "ranges",
                    format!(
                        "a range starts at page {}, but the document has {page_count} pages",
                        r.from_page
                    ),
                ));
            }
            if i > 0 && ranges[i - 1].from_page == r.from_page {
                return Err(DomainError::option(
                    "ranges",
                    format!("two ranges start at page {}", r.from_page),
                ));
            }
            if r.prefix.chars().count() > 64 {
                return Err(DomainError::option(
                    "ranges",
                    "a prefix is longer than 64 characters",
                ));
            }
            if r.start > 1_000_000 {
                return Err(DomainError::option(
                    "ranges",
                    "a start number is above 1000000",
                ));
            }
        }
        Ok(ranges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn bm(title: &str, page: u32, level: u32) -> BookmarkEntry {
        BookmarkEntry {
            title: title.into(),
            page,
            level,
        }
    }

    #[test]
    fn bookmarks_are_checked_and_converted_to_zero_based_pages() {
        let list = [bm("Intro", 1, 1), bm("Scope", 2, 2), bm("Method", 5, 1)];
        let out = check_bookmarks(&list, 5).unwrap();
        assert_eq!(
            out,
            vec![
                ("Intro".to_owned(), PageIndex::new(0), 1),
                ("Scope".to_owned(), PageIndex::new(1), 2),
                ("Method".to_owned(), PageIndex::new(4), 1)
            ]
        );
    }

    #[test]
    fn bad_bookmarks_say_which_row_is_wrong() {
        let reason = |list: &[BookmarkEntry]| match check_bookmarks(list, 3).unwrap_err() {
            DomainError::OptionInvalid { reason, .. } => reason,
            other => panic!("{other:?}"),
        };
        assert!(reason(&[bm("  ", 1, 1)]).contains("bookmark 1 has no title"));
        assert!(reason(&[bm("a", 1, 1), bm("b", 4, 1)]).contains("bookmark 2 points at page 4"));
        assert!(reason(&[bm("a", 0, 1)]).contains("page 0"));
        assert!(reason(&[bm("a", 1, 2)]).contains("nested too deep"));
        assert!(reason(&[bm("a", 1, 1), bm("b", 1, 3)]).contains("bookmark 2"));
        assert!(reason(&[bm("a", 1, 0)]).contains("nested"));
    }

    #[test]
    fn a_manual_list_needs_entries_and_an_automatic_one_needs_a_sane_depth() {
        assert_eq!(
            BookmarksOptions::default().validate().unwrap_err().code(),
            ErrorCode::OptionInvalid
        );
        let auto = BookmarksOptions {
            auto_generate: true,
            ..BookmarksOptions::default()
        };
        assert!(auto.validate().is_ok());
        let deep = BookmarksOptions {
            auto_generate: true,
            auto_max_level: 9,
            ..BookmarksOptions::default()
        };
        assert!(deep.validate().is_err());
    }

    fn line(page: u32, text: &str, size: f32) -> TextLine {
        TextLine {
            page: PageIndex::new(page),
            text: text.into(),
            size_pt: size,
        }
    }

    #[test]
    fn headings_are_the_larger_lines_with_the_biggest_size_on_top() {
        let lines = vec![
            line(0, "Annual Report", 24.0),
            line(
                0,
                "This body text runs on and on for a long while, filling the page.",
                11.0,
            ),
            line(
                0,
                "Another long line of body text that sets the common size.",
                11.0,
            ),
            line(1, "Results", 16.0),
            line(
                1,
                "More body text goes here, long enough to dominate the count.",
                11.0,
            ),
            line(2, "Costs", 16.0),
            line(
                2,
                "Body again, once more with feeling and a good number of words.",
                11.0,
            ),
        ];
        let found = detect_headings(&lines, 3, 3);
        assert_eq!(
            found,
            vec![
                bm("Annual Report", 1, 1),
                bm("Results", 2, 2),
                bm("Costs", 3, 2)
            ]
        );
        let shallow = detect_headings(&lines, 3, 1);
        assert_eq!(shallow, vec![bm("Annual Report", 1, 1)]);
    }

    #[test]
    fn running_heads_page_numbers_and_tiny_size_differences_are_not_headings() {
        let mut lines = Vec::new();
        for page in 0..6 {
            lines.push(line(page, "ACME Confidential", 14.0));
            lines.push(line(
                page,
                "Plain body text that is quite long and fills the line out.",
                11.0,
            ));
            lines.push(line(
                page,
                "Another body line so the body size is clearly eleven points.",
                11.0,
            ));
            lines.push(line(page, &format!("{}", page + 1), 14.0));
            lines.push(line(page, "Almost body", 11.5));
        }
        lines.push(line(2, "Chapter Two", 14.0));
        let found = detect_headings(&lines, 6, 3);
        assert_eq!(found, vec![bm("Chapter Two", 3, 1)]);
        assert!(detect_headings(&[], 1, 3).is_empty());
    }

    #[test]
    fn rename_needs_a_region_a_page_and_the_text_token() {
        let region = Region {
            x: 0.1,
            y: 0.1,
            width: 0.3,
            height: 0.05,
        };
        let ok = RenameByContentOptions {
            region: Some(region),
            ..RenameByContentOptions::default()
        };
        assert!(ok.checked().is_ok());
        for bad in [
            RenameByContentOptions::default(),
            RenameByContentOptions {
                region: Some(region),
                page: 0,
                ..RenameByContentOptions::default()
            },
            RenameByContentOptions {
                region: Some(region),
                pattern: "{filename}".into(),
                ..RenameByContentOptions::default()
            },
            RenameByContentOptions {
                region: Some(region),
                pattern: "{text} {nope}".into(),
                ..RenameByContentOptions::default()
            },
        ] {
            assert_eq!(
                bad.checked().unwrap_err().code(),
                ErrorCode::OptionInvalid,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn each_attachment_action_asks_for_what_it_needs() {
        assert!(AttachmentsOptions::default().validate().is_ok());
        let add = AttachmentsOptions {
            action: AttachmentAction::Add,
            ..AttachmentsOptions::default()
        };
        assert!(add.validate().is_err());
        let add = AttachmentsOptions {
            files: vec![" C:\\a.txt ".into(), "".into()],
            ..add
        };
        assert!(add.validate().is_ok());
        assert_eq!(add.file_list(), vec!["C:\\a.txt"]);
        let delete = AttachmentsOptions {
            action: AttachmentAction::Delete,
            ..AttachmentsOptions::default()
        };
        assert!(delete.validate().is_err());
        let delete = AttachmentsOptions {
            names: vec!["a.txt".into(), " ".into()],
            ..delete
        };
        assert!(delete.validate().is_ok());
        assert_eq!(delete.name_list(), vec!["a.txt"]);
        let extract = AttachmentsOptions {
            action: AttachmentAction::Extract,
            ..AttachmentsOptions::default()
        };
        assert!(extract.validate().is_ok());
    }

    #[test]
    fn label_ranges_are_sorted_and_checked() {
        let r = |from_page, prefix: &str| LabelRange {
            from_page,
            prefix: prefix.into(),
            ..LabelRange::default()
        };
        let options = PageLabelsOptions {
            ranges: vec![r(5, "B-"), r(1, "A-")],
            remove_all: false,
        };
        let sorted = options.checked(10).unwrap();
        assert_eq!(sorted[0].from_page, 1);
        assert_eq!(sorted[1].prefix, "B-");
        for bad in [
            PageLabelsOptions {
                ranges: vec![r(11, "")],
                remove_all: false,
            },
            PageLabelsOptions {
                ranges: vec![r(0, "")],
                remove_all: false,
            },
            PageLabelsOptions {
                ranges: vec![r(2, ""), r(2, "x")],
                remove_all: false,
            },
            PageLabelsOptions {
                ranges: vec![],
                remove_all: false,
            },
            PageLabelsOptions {
                ranges: vec![r(1, &"p".repeat(65))],
                remove_all: false,
            },
        ] {
            assert_eq!(
                bad.checked(10).unwrap_err().code(),
                ErrorCode::OptionInvalid,
                "{bad:?}"
            );
        }
        let clear = PageLabelsOptions {
            ranges: vec![],
            remove_all: true,
        };
        assert!(clear.checked(10).unwrap().is_empty());
    }
}
