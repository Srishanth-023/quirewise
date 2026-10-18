//! Tool identifiers and per-tool option structs. One module per tool.

pub mod combine;
pub mod common;
pub mod pages;
pub mod rotate;

use core::fmt;
use core::str::FromStr;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
pub use combine::{AlternateMixOptions, MergeOptions, OrganizeOptions};
pub use pages::{
    ExtractPagesOptions, InsertPagesOptions, RemovePagesOptions, SplitByBookmarksOptions,
    SplitBySizeOptions, SplitByTextOptions, SplitEveryNOptions, SplitInHalfOptions,
    SplitRangesOptions,
};
pub use rotate::RotateOptions;

/// Navigation group a tool belongs to (MASTER_PROMPT.md section 3.1).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ToolGroup {
    /// Merge, split, extract and reorder pages.
    Organize,
    /// Compress, repair, OCR and deskew.
    Optimize,
    /// Images, Office files and web pages to PDF.
    ConvertToPdf,
    /// PDF to images, Office files, text and PDF/A.
    ConvertFromPdf,
    /// Rotate, stamp, crop, annotate and edit.
    Edit,
    /// Fill, create and flatten forms.
    Forms,
    /// Protect, unlock, sign, redact and compare.
    Security,
    /// Optional local AI tools.
    Intelligence,
    /// PDF/A and accessibility validation.
    Validation,
}

/// Identifier of a tool, kebab-case on the wire (`split-by-size`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ToolId {
    /// Merge PDFs and images (Appendix A row 1).
    Merge,
    /// Alternate and mix pages of several files (row 2).
    AlternateMix,
    /// Organize pages visually (row 3).
    Organize,
    /// Split by ranges (row 4).
    SplitRanges,
    /// Split every N pages (row 5).
    SplitEveryN,
    /// Extract pages (row 6).
    ExtractPages,
    /// Remove pages (row 7).
    RemovePages,
    /// Split by bookmarks (row 8).
    SplitByBookmarks,
    /// Split pages in half (row 9).
    SplitInHalf,
    /// Split by file size (row 10).
    SplitBySize,
    /// Split by text in a region (row 11).
    SplitByText,
    /// Rotate pages (row 32).
    Rotate,
    /// Insert blank or duplicate pages (row 69).
    InsertPages,
}

impl ToolId {
    /// Every tool id the domain knows. The runtime tool list comes from the registry.
    pub const ALL: [ToolId; 13] = [
        ToolId::Merge,
        ToolId::AlternateMix,
        ToolId::Organize,
        ToolId::SplitRanges,
        ToolId::SplitEveryN,
        ToolId::ExtractPages,
        ToolId::RemovePages,
        ToolId::SplitByBookmarks,
        ToolId::SplitInHalf,
        ToolId::SplitBySize,
        ToolId::SplitByText,
        ToolId::Rotate,
        ToolId::InsertPages,
    ];

    /// The kebab-case identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::AlternateMix => "alternate-mix",
            Self::Organize => "organize",
            Self::SplitRanges => "split-ranges",
            Self::SplitEveryN => "split-every-n",
            Self::ExtractPages => "extract-pages",
            Self::RemovePages => "remove-pages",
            Self::SplitByBookmarks => "split-by-bookmarks",
            Self::SplitInHalf => "split-in-half",
            Self::SplitBySize => "split-by-size",
            Self::SplitByText => "split-by-text",
            Self::Rotate => "rotate",
            Self::InsertPages => "insert-pages",
        }
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ToolId {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|id| id.as_str() == s)
            .ok_or_else(|| DomainError::option("tool", format!("unknown tool \"{s}\"")))
    }
}

/// Options for any tool. The tag is the tool id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "tool", rename_all = "kebab-case")]
pub enum ToolOptions {
    /// Options for [`ToolId::Merge`].
    Merge(MergeOptions),
    /// Options for [`ToolId::AlternateMix`].
    AlternateMix(AlternateMixOptions),
    /// Options for [`ToolId::Organize`].
    Organize(OrganizeOptions),
    /// Options for [`ToolId::SplitRanges`].
    SplitRanges(SplitRangesOptions),
    /// Options for [`ToolId::SplitEveryN`].
    SplitEveryN(SplitEveryNOptions),
    /// Options for [`ToolId::ExtractPages`].
    ExtractPages(ExtractPagesOptions),
    /// Options for [`ToolId::RemovePages`].
    RemovePages(RemovePagesOptions),
    /// Options for [`ToolId::SplitByBookmarks`].
    SplitByBookmarks(SplitByBookmarksOptions),
    /// Options for [`ToolId::SplitInHalf`].
    SplitInHalf(SplitInHalfOptions),
    /// Options for [`ToolId::SplitBySize`].
    SplitBySize(SplitBySizeOptions),
    /// Options for [`ToolId::SplitByText`].
    SplitByText(SplitByTextOptions),
    /// Options for [`ToolId::Rotate`].
    Rotate(RotateOptions),
    /// Options for [`ToolId::InsertPages`].
    InsertPages(InsertPagesOptions),
}

impl ToolOptions {
    /// The tool these options belong to.
    pub const fn tool_id(&self) -> ToolId {
        match self {
            Self::Merge(_) => ToolId::Merge,
            Self::AlternateMix(_) => ToolId::AlternateMix,
            Self::Organize(_) => ToolId::Organize,
            Self::SplitRanges(_) => ToolId::SplitRanges,
            Self::SplitEveryN(_) => ToolId::SplitEveryN,
            Self::ExtractPages(_) => ToolId::ExtractPages,
            Self::RemovePages(_) => ToolId::RemovePages,
            Self::SplitByBookmarks(_) => ToolId::SplitByBookmarks,
            Self::SplitInHalf(_) => ToolId::SplitInHalf,
            Self::SplitBySize(_) => ToolId::SplitBySize,
            Self::SplitByText(_) => ToolId::SplitByText,
            Self::Rotate(_) => ToolId::Rotate,
            Self::InsertPages(_) => ToolId::InsertPages,
        }
    }

    /// Default options for a tool, matching Appendix A.
    pub fn defaults_for(id: ToolId) -> Self {
        match id {
            ToolId::Merge => Self::Merge(MergeOptions::default()),
            ToolId::AlternateMix => Self::AlternateMix(AlternateMixOptions::default()),
            ToolId::Organize => Self::Organize(OrganizeOptions::default()),
            ToolId::SplitRanges => Self::SplitRanges(SplitRangesOptions::default()),
            ToolId::SplitEveryN => Self::SplitEveryN(SplitEveryNOptions::default()),
            ToolId::ExtractPages => Self::ExtractPages(ExtractPagesOptions::default()),
            ToolId::RemovePages => Self::RemovePages(RemovePagesOptions::default()),
            ToolId::SplitByBookmarks => Self::SplitByBookmarks(SplitByBookmarksOptions::default()),
            ToolId::SplitInHalf => Self::SplitInHalf(SplitInHalfOptions::default()),
            ToolId::SplitBySize => Self::SplitBySize(SplitBySizeOptions::default()),
            ToolId::SplitByText => Self::SplitByText(SplitByTextOptions::default()),
            ToolId::Rotate => Self::Rotate(RotateOptions::default()),
            ToolId::InsertPages => Self::InsertPages(InsertPagesOptions::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_ids_are_kebab_case_and_parse_back() {
        for id in ToolId::ALL {
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(json, format!("\"{}\"", id.as_str()));
            assert_eq!(id.as_str().parse::<ToolId>().unwrap(), id);
            assert!(
                id.as_str()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-')
            );
        }
        assert!("nope".parse::<ToolId>().is_err());
    }

    #[test]
    fn options_are_tagged_by_tool_id() {
        let opts = ToolOptions::defaults_for(ToolId::Rotate);
        let json = serde_json::to_value(&opts).unwrap();
        assert_eq!(json["tool"], "rotate");
        let back: ToolOptions = serde_json::from_value(json).unwrap();
        assert_eq!(back, opts);
        assert_eq!(back.tool_id(), ToolId::Rotate);
    }
}
