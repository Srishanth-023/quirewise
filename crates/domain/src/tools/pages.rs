//! Options for the simple page-selection tools: extract, remove, split by ranges, every N pages,
//! by size, in half, by bookmarks, by text, and insert blank or duplicate pages.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::common::{PageSizeChoice, Region};
use crate::error::DomainError;
use crate::page_range::{Duplicates, PageRange, PageSelection};
use crate::planners::{CutDirection, InsertKind, InsertPosition};
use crate::units::{Bytes, PageIndex};

/// Extract pages (Appendix A row 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ExtractPagesOptions {
    /// Pages to extract: a range such as `1,3-5,last`. Default `all`.
    pub pages: PageRange,
    /// One output file with all selected pages (default), or one file per page.
    pub one_file: bool,
}

impl Default for ExtractPagesOptions {
    fn default() -> Self {
        Self {
            pages: PageRange::all(),
            one_file: true,
        }
    }
}

impl ExtractPagesOptions {
    /// The output groups: one group with every selected page, or one group per page.
    pub fn groups(&self, page_count: u32) -> Result<Vec<Vec<PageIndex>>, DomainError> {
        let selection = self.pages.resolve(page_count, Duplicates::Reject)?;
        Ok(if self.one_file {
            vec![selection.pages().to_vec()]
        } else {
            selection.pages().iter().map(|p| vec![*p]).collect()
        })
    }
}

/// Remove pages (Appendix A row 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RemovePagesOptions {
    /// Pages to remove: a range such as `2,4-6`. Default `1`.
    pub pages: PageRange,
}

impl Default for RemovePagesOptions {
    fn default() -> Self {
        Self {
            pages: PageRange::parse("1").unwrap_or_else(|_| PageRange::all()),
        }
    }
}

impl RemovePagesOptions {
    /// The pages that stay, in order. Removing every page is an error: nothing would be left.
    pub fn kept_pages(&self, page_count: u32) -> Result<Vec<PageIndex>, DomainError> {
        let removed = self.pages.resolve(page_count, Duplicates::Reject)?;
        let kept: Vec<PageIndex> = (0..page_count)
            .map(PageIndex::new)
            .filter(|p| !removed.contains(*p))
            .collect();
        if kept.is_empty() {
            return Err(DomainError::option(
                "pages",
                "this would remove every page; keep at least one",
            ));
        }
        Ok(kept)
    }
}

/// Split by ranges (Appendix A row 4).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SplitRangesOptions {
    /// Ranges, each comma-separated element becoming one file: `1-3,4-6,7-`. Default `all`.
    pub ranges: PageRange,
    /// Put all selected ranges into a single file instead. Default off.
    pub merge_into_one: bool,
}

impl SplitRangesOptions {
    /// The output groups.
    pub fn groups(&self, page_count: u32) -> Result<Vec<Vec<PageIndex>>, DomainError> {
        // The whole expression is checked first so overlapping ranges are reported once.
        let all: PageSelection = self.ranges.resolve(page_count, Duplicates::Reject)?;
        if self.merge_into_one {
            return Ok(vec![all.pages().to_vec()]);
        }
        self.ranges
            .items()
            .iter()
            .map(|item| {
                PageRange::from_items(vec![*item])
                    .resolve(page_count, Duplicates::Reject)
                    .map(|s| s.pages().to_vec())
            })
            .collect()
    }
}

/// Split every N pages (Appendix A row 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SplitEveryNOptions {
    /// Pages per file, at least 1. Default 1.
    pub n: u32,
}

impl Default for SplitEveryNOptions {
    fn default() -> Self {
        Self { n: 1 }
    }
}

impl SplitEveryNOptions {
    /// Rejects an N of zero.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.n == 0 {
            return Err(DomainError::option("n", "must be at least 1"));
        }
        Ok(())
    }
}

/// Orientation of the cut for Split in half.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SplitDirection {
    /// Cut down the middle: left and right halves (default).
    #[default]
    Vertical,
    /// Cut across the middle: top and bottom halves.
    Horizontal,
}

impl From<SplitDirection> for CutDirection {
    fn from(d: SplitDirection) -> Self {
        match d {
            SplitDirection::Vertical => Self::Vertical,
            SplitDirection::Horizontal => Self::Horizontal,
        }
    }
}

/// Split in half (Appendix A row 9).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SplitInHalfOptions {
    /// Direction of the cut. Default vertical.
    pub direction: SplitDirection,
    /// Leave the first page whole. Default off.
    pub skip_first: bool,
    /// Leave the last page whole. Default off.
    pub skip_last: bool,
    /// Put the right half before the left half, for right-to-left scans. Default off.
    pub right_to_left: bool,
}

/// Split by size (Appendix A row 10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SplitBySizeOptions {
    /// Largest output file in megabytes (1 MB is 1,048,576 bytes). Default 10.
    pub max_size_mb: u32,
}

impl Default for SplitBySizeOptions {
    fn default() -> Self {
        Self { max_size_mb: 10 }
    }
}

impl SplitBySizeOptions {
    /// The limit in bytes. Must be between 1 MB and 2,048 MB.
    pub fn limit(&self) -> Result<Bytes, DomainError> {
        if !(1..=2048).contains(&self.max_size_mb) {
            return Err(DomainError::option(
                "max_size_mb",
                "must be between 1 and 2048",
            ));
        }
        Ok(Bytes::from_mb(u64::from(self.max_size_mb)))
    }
}

/// Split by bookmarks (Appendix A row 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SplitByBookmarksOptions {
    /// Outline level to split at, where 1 is the top level. Default 1.
    pub level: u32,
    /// Name each file after its bookmark title. Default on.
    pub name_from_bookmark: bool,
}

impl Default for SplitByBookmarksOptions {
    fn default() -> Self {
        Self {
            level: 1,
            name_from_bookmark: true,
        }
    }
}

impl SplitByBookmarksOptions {
    /// Rejects level 0.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.level == 0 {
            return Err(DomainError::option("level", "must be at least 1"));
        }
        Ok(())
    }
}

/// Split by text (Appendix A row 11).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SplitByTextOptions {
    /// The region to read on every page, drawn on a page preview. Required.
    pub region: Option<Region>,
    /// Start a new file only on pages whose region text starts with this. Default none.
    pub starts_with: Option<String>,
}

impl SplitByTextOptions {
    /// The region, validated. Splitting by text needs one.
    pub fn region(&self) -> Result<Region, DomainError> {
        let region = self
            .region
            .ok_or_else(|| DomainError::option("region", "draw the area to read on a page"))?;
        region.validate()?;
        Ok(region)
    }
}

/// What Insert pages adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum InsertMode {
    /// New blank pages (default).
    #[default]
    Blank,
    /// Copies of the selected pages.
    Duplicate,
}

/// Where blank pages go relative to the selected page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum InsertWhere {
    /// Before the selected page.
    Before,
    /// After the selected page (default).
    #[default]
    After,
}

/// Insert blank or duplicate pages (Appendix A row 69).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct InsertPagesOptions {
    /// Blank pages or copies. Default blank.
    pub mode: InsertMode,
    /// The pages to insert around: a range such as `1,5`. Default `1`.
    pub pages: PageRange,
    /// Before or after each selected page. Default after.
    pub position: InsertWhere,
    /// How many to insert at each place, 1 to 100. Default 1.
    pub count: u32,
    /// Size of blank pages. Default the same as the neighboring page.
    pub page_size: PageSizeChoice,
}

impl Default for InsertPagesOptions {
    fn default() -> Self {
        Self {
            mode: InsertMode::Blank,
            pages: PageRange::parse("1").unwrap_or_else(|_| PageRange::all()),
            position: InsertWhere::After,
            count: 1,
            page_size: PageSizeChoice::SameAsNeighbor,
        }
    }
}

impl InsertPagesOptions {
    /// Validates the count and resolves the selected pages.
    pub fn selection(&self, page_count: u32) -> Result<Vec<u32>, DomainError> {
        if !(1..=100).contains(&self.count) {
            return Err(DomainError::option("count", "must be between 1 and 100"));
        }
        let selected = self.pages.resolve(page_count, Duplicates::Reject)?;
        Ok(selected.pages().iter().map(|p| p.get()).collect())
    }

    /// The planner kind for these options.
    pub fn kind(&self) -> InsertKind {
        match self.mode {
            InsertMode::Blank => InsertKind::Blank,
            InsertMode::Duplicate => InsertKind::Duplicate,
        }
    }

    /// The planner position for these options.
    pub fn where_(&self) -> InsertPosition {
        match self.position {
            InsertWhere::Before => InsertPosition::Before,
            InsertWhere::After => InsertPosition::After,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn range(s: &str) -> PageRange {
        PageRange::parse(s).unwrap()
    }

    fn numbers(groups: &[Vec<PageIndex>]) -> Vec<Vec<u32>> {
        groups
            .iter()
            .map(|g| g.iter().map(|p| p.get() + 1).collect())
            .collect()
    }

    #[test]
    fn extract_groups_one_file_or_one_per_page() {
        let one = ExtractPagesOptions {
            pages: range("1,3,5-6"),
            one_file: true,
        };
        assert_eq!(numbers(&one.groups(8).unwrap()), vec![vec![1, 3, 5, 6]]);
        let each = ExtractPagesOptions {
            pages: range("2-3"),
            one_file: false,
        };
        assert_eq!(numbers(&each.groups(8).unwrap()), vec![vec![2], vec![3]]);
        assert_eq!(
            one.groups(4).unwrap_err().code(),
            ErrorCode::RangeOutOfBounds
        );
        assert_eq!(ExtractPagesOptions::default().pages.to_string(), "all");
    }

    #[test]
    fn remove_keeps_the_rest_and_refuses_to_empty_the_file() {
        let o = RemovePagesOptions {
            pages: range("2,4-5"),
        };
        let kept: Vec<u32> = o
            .kept_pages(6)
            .unwrap()
            .iter()
            .map(|p| p.get() + 1)
            .collect();
        assert_eq!(kept, vec![1, 3, 6]);
        let all = RemovePagesOptions {
            pages: range("all"),
        };
        assert_eq!(
            all.kept_pages(3).unwrap_err().code(),
            ErrorCode::OptionInvalid
        );
        assert_eq!(RemovePagesOptions::default().pages.to_string(), "1");
    }

    #[test]
    fn split_ranges_make_one_file_per_element_or_one_merged() {
        let o = SplitRangesOptions {
            ranges: range("1-2,4,6-"),
            merge_into_one: false,
        };
        assert_eq!(
            numbers(&o.groups(7).unwrap()),
            vec![vec![1, 2], vec![4], vec![6, 7]]
        );
        let merged = SplitRangesOptions {
            merge_into_one: true,
            ..o.clone()
        };
        assert_eq!(
            numbers(&merged.groups(7).unwrap()),
            vec![vec![1, 2, 4, 6, 7]]
        );
        let overlap = SplitRangesOptions {
            ranges: range("1-3,3-4"),
            merge_into_one: false,
        };
        assert_eq!(
            overlap.groups(5).unwrap_err().code(),
            ErrorCode::RangeSyntax
        );
    }

    #[test]
    fn simple_option_defaults_match_appendix_a() {
        assert_eq!(SplitEveryNOptions::default().n, 1);
        assert_eq!(
            SplitBySizeOptions::default().limit().unwrap().get(),
            10 * 1024 * 1024
        );
        let half = SplitInHalfOptions::default();
        assert_eq!(half.direction, SplitDirection::Vertical);
        assert!(!half.skip_first && !half.skip_last && !half.right_to_left);
        let bm = SplitByBookmarksOptions::default();
        assert_eq!((bm.level, bm.name_from_bookmark), (1, true));
        assert!(SplitByTextOptions::default().starts_with.is_none());
        let ins = InsertPagesOptions::default();
        assert_eq!(
            (ins.mode, ins.position, ins.count),
            (InsertMode::Blank, InsertWhere::After, 1)
        );
        assert_eq!(ins.page_size, PageSizeChoice::SameAsNeighbor);
    }

    #[test]
    fn invalid_values_are_rejected() {
        assert!(SplitEveryNOptions { n: 0 }.validate().is_err());
        assert!(SplitBySizeOptions { max_size_mb: 0 }.limit().is_err());
        assert!(SplitBySizeOptions { max_size_mb: 5000 }.limit().is_err());
        assert!(
            SplitByBookmarksOptions {
                level: 0,
                name_from_bookmark: true
            }
            .validate()
            .is_err()
        );
        assert!(SplitByTextOptions::default().region().is_err());
        let bad_region = SplitByTextOptions {
            region: Some(Region {
                x: 0.9,
                y: 0.0,
                width: 0.5,
                height: 0.1,
            }),
            starts_with: None,
        };
        assert!(bad_region.region().is_err());
        assert!(
            InsertPagesOptions {
                count: 0,
                ..InsertPagesOptions::default()
            }
            .selection(3)
            .is_err()
        );
        assert!(
            InsertPagesOptions {
                count: 101,
                ..InsertPagesOptions::default()
            }
            .selection(3)
            .is_err()
        );
    }

    #[test]
    fn insert_selection_resolves_pages() {
        let o = InsertPagesOptions {
            pages: range("2,last"),
            ..InsertPagesOptions::default()
        };
        assert_eq!(o.selection(5).unwrap(), vec![1, 4]);
        assert_eq!(
            o.selection(1).unwrap_err().code(),
            ErrorCode::RangeOutOfBounds
        );
    }

    #[test]
    fn unknown_fields_are_refused() {
        assert!(serde_json::from_str::<SplitEveryNOptions>(r#"{"m":3}"#).is_err());
        let ok: SplitEveryNOptions = serde_json::from_str(r#"{"n":3}"#).unwrap();
        assert_eq!(ok.n, 3);
    }
}
