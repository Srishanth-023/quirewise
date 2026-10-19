//! Options of Compare (row 55): what differs between two versions of a document.
//!
//! The two files are given in order, the original first and the changed one second. Pages are
//! compared by number: page 3 of one against page 3 of the other.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::page_range::{Duplicates, PageRange, PageSelection};
use crate::units::Dpi;

/// Most a pixel's color may differ before it counts as changed: half the range.
pub const MAX_TOLERANCE_PERCENT: f32 = 50.0;
/// Smallest and largest resolution pages are compared at.
pub const DPI_RANGE: (u32, u32) = (36, 200);

/// What to compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CompareMode {
    /// The text and how the pages look (default).
    #[default]
    Both,
    /// Only the text.
    Text,
    /// Only how the pages look.
    Visual,
}

impl CompareMode {
    /// True when the text is compared.
    pub fn text(self) -> bool {
        matches!(self, Self::Both | Self::Text)
    }

    /// True when the pages are compared as pictures.
    pub fn visual(self) -> bool {
        matches!(self, Self::Both | Self::Visual)
    }
}

/// Options for Compare.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CompareOptions {
    /// What to compare. Default both the text and the look of the pages.
    pub mode: CompareMode,
    /// How much the color of a pixel may differ, in percent of the full range, before the pixel
    /// counts as changed. Default 2.
    pub tolerance_percent: f32,
    /// Treat runs of spaces and tabs as one space, and ignore blank lines. Default on.
    pub ignore_whitespace: bool,
    /// Pages to compare, by number in both files: `all`, `even`, `odd` or a range such as
    /// `1,3-5`. Default `all`.
    pub pages: PageRange,
    /// Resolution of the pictures the pages are compared at. Default 100.
    pub dpi: u32,
}

impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            mode: CompareMode::Both,
            tolerance_percent: 2.0,
            ignore_whitespace: true,
            pages: PageRange::all(),
            dpi: 100,
        }
    }
}

impl CompareOptions {
    /// Checks the numbers.
    pub fn validate(&self) -> Result<(), DomainError> {
        if !self.tolerance_percent.is_finite()
            || !(0.0..=MAX_TOLERANCE_PERCENT).contains(&self.tolerance_percent)
        {
            return Err(DomainError::option(
                "tolerance_percent",
                format!("use a number from 0 to {MAX_TOLERANCE_PERCENT}"),
            ));
        }
        if !(DPI_RANGE.0..=DPI_RANGE.1).contains(&self.dpi) {
            return Err(DomainError::option(
                "dpi",
                format!("use a resolution from {} to {}", DPI_RANGE.0, DPI_RANGE.1),
            ));
        }
        Ok(())
    }

    /// The pages to compare, against the page count of the longer file.
    pub fn select_pages(&self, page_count: u32) -> Result<PageSelection, DomainError> {
        self.pages.resolve(page_count, Duplicates::Reject)
    }

    /// The resolution as a validated value.
    pub fn resolution(&self) -> Result<Dpi, DomainError> {
        Dpi::new(self.dpi)
    }

    /// The largest color difference, 0 to 255, that is not a change.
    pub fn tolerance_levels(&self) -> u8 {
        (self.tolerance_percent / 100.0 * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_valid_and_compare_everything() {
        let o = CompareOptions::default();
        assert!(o.validate().is_ok());
        assert!(o.mode.text() && o.mode.visual());
        assert_eq!(o.tolerance_levels(), 5);
    }

    #[test]
    fn the_modes_say_what_they_compare() {
        assert!(CompareMode::Text.text() && !CompareMode::Text.visual());
        assert!(!CompareMode::Visual.text() && CompareMode::Visual.visual());
    }

    #[test]
    fn bad_numbers_are_refused() {
        for tolerance in [-1.0, 51.0, f32::NAN, f32::INFINITY] {
            let o = CompareOptions {
                tolerance_percent: tolerance,
                ..CompareOptions::default()
            };
            assert!(o.validate().is_err(), "{tolerance}");
        }
        for dpi in [0, 35, 201] {
            let o = CompareOptions {
                dpi,
                ..CompareOptions::default()
            };
            assert!(o.validate().is_err(), "{dpi}");
        }
    }

    #[test]
    fn pages_are_resolved_against_the_longer_file() {
        let o = CompareOptions {
            pages: PageRange::parse("2-3").unwrap(),
            ..CompareOptions::default()
        };
        assert_eq!(o.select_pages(3).unwrap().len(), 2);
        assert!(o.select_pages(2).is_err());
    }

    #[test]
    fn options_read_from_json_with_defaults() {
        let o: CompareOptions =
            serde_json::from_str(r#"{"mode":"text","tolerance_percent":0}"#).unwrap();
        assert_eq!(o.mode, CompareMode::Text);
        assert_eq!(o.tolerance_levels(), 0);
        assert!(o.ignore_whitespace);
        assert!(serde_json::from_str::<CompareOptions>(r#"{"nope":1}"#).is_err());
    }
}
