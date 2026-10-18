//! Rotate options (Appendix A row 32): 90, 180 or 270 degrees on all, even, odd or selected pages.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::common::RightAngle;
use crate::error::DomainError;
use crate::page_range::{Duplicates, PageRange, PageSelection};

/// Options for the rotate tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RotateOptions {
    /// Rotation angle, clockwise. Default 90.
    pub angle: RightAngle,
    /// Pages to rotate: `all`, `even`, `odd` or a range such as `1,3-5`. Default `all`.
    pub pages: PageRange,
}

impl Default for RotateOptions {
    fn default() -> Self {
        Self {
            angle: RightAngle::Deg90,
            pages: PageRange::all(),
        }
    }
}

impl RotateOptions {
    /// Resolves the page selection against the document's page count.
    pub fn select_pages(&self, page_count: u32) -> Result<PageSelection, DomainError> {
        self.pages.resolve(page_count, Duplicates::Reject)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn defaults_match_appendix_a() {
        let d = RotateOptions::default();
        assert_eq!(d.angle.degrees(), 90);
        assert_eq!(d.pages.to_string(), "all");
    }

    #[test]
    fn partial_json_fills_defaults_and_unknown_fields_fail() {
        let o: RotateOptions = serde_json::from_str(r#"{"angle":"180"}"#).unwrap();
        assert_eq!(o.angle, RightAngle::Deg180);
        assert_eq!(o.pages, PageRange::all());
        let empty: RotateOptions = serde_json::from_str("{}").unwrap();
        assert_eq!(
            empty.angle,
            RightAngle::Deg90,
            "container default applies, not the type default"
        );
        assert!(serde_json::from_str::<RotateOptions>(r#"{"angel":"90"}"#).is_err());
        assert!(serde_json::from_str::<RotateOptions>(r#"{"angle":"45"}"#).is_err());
    }

    #[test]
    fn selection_is_checked_against_the_page_count() {
        let o = RotateOptions {
            pages: PageRange::parse("2-4").unwrap(),
            ..RotateOptions::default()
        };
        assert_eq!(o.select_pages(10).unwrap().len(), 3);
        assert_eq!(
            o.select_pages(3).unwrap_err().code(),
            ErrorCode::RangeOutOfBounds
        );
    }
}
