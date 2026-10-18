//! Rotate options (Appendix A row 32): 90, 180 or 270 degrees on all, even, odd or selected pages.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::page_range::{Duplicates, PageRange, PageSelection};

/// Clockwise rotation applied to the selected pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub enum Rotation {
    /// 90 degrees clockwise (default).
    #[default]
    #[serde(rename = "90")]
    Deg90,
    /// 180 degrees.
    #[serde(rename = "180")]
    Deg180,
    /// 270 degrees clockwise (90 counter-clockwise).
    #[serde(rename = "270")]
    Deg270,
}

impl Rotation {
    /// The angle in degrees.
    pub const fn degrees(self) -> u16 {
        match self {
            Self::Deg90 => 90,
            Self::Deg180 => 180,
            Self::Deg270 => 270,
        }
    }
}

/// Options for the rotate tool.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RotateOptions {
    /// Rotation angle. Default 90.
    pub angle: Rotation,
    /// Pages to rotate: `all`, `even`, `odd` or a range such as `1,3-5`. Default `all`.
    pub pages: PageRange,
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
        assert_eq!(o.angle, Rotation::Deg180);
        assert_eq!(o.pages, PageRange::all());
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
