//! Options for tools that combine several documents: merge, alternate and mix, organize.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::common::RightAngle;
use crate::error::DomainError;
use crate::page_range::{Duplicates, PageRange};
use crate::units::PageIndex;

/// What to do with form fields while merging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FormHandling {
    /// Keep fields fillable (default).
    #[default]
    Keep,
    /// Flatten fields into the page so they can no longer be edited.
    Flatten,
}

/// Settings for one input of a merge. Entry `i` applies to input `i`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MergeFileOptions {
    /// Pages to take from this file. Default `all`.
    pub pages: PageRange,
    /// Extra rotation applied to every page taken from this file. Default none.
    pub rotation: RightAngle,
}

/// Merge (Appendix A row 1). Files and images are merged in the order they are listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MergeOptions {
    /// Per-file settings, matching the input order. Missing entries use the defaults.
    pub per_file: Vec<MergeFileOptions>,
    /// Add a bookmark for each file. Default on.
    pub bookmark_per_file: bool,
    /// Insert a table of contents page listing the files. Default off.
    pub table_of_contents: bool,
    /// Scale all pages to the size of the first page. Default off.
    pub normalize_page_size: bool,
    /// Keep or flatten form fields. Default keep.
    pub forms: FormHandling,
}

impl Default for MergeOptions {
    fn default() -> Self {
        Self {
            per_file: Vec::new(),
            bookmark_per_file: true,
            table_of_contents: false,
            normalize_page_size: false,
            forms: FormHandling::Keep,
        }
    }
}

impl MergeOptions {
    /// Settings for input `index`, falling back to the defaults.
    pub fn file(&self, index: usize) -> MergeFileOptions {
        self.per_file.get(index).cloned().unwrap_or_default()
    }

    /// The pages taken from input `index` (a document with `page_count` pages). Pages may repeat.
    pub fn pages_for(&self, index: usize, page_count: u32) -> Result<Vec<PageIndex>, DomainError> {
        self.file(index)
            .pages
            .resolve(page_count, Duplicates::Allow)
            .map(|s| s.pages().to_vec())
    }

    /// Rejects more per-file entries than inputs.
    pub fn validate(&self, inputs: usize) -> Result<(), DomainError> {
        if self.per_file.len() > inputs {
            return Err(DomainError::option(
                "per_file",
                "there are more per-file settings than files",
            ));
        }
        Ok(())
    }
}

/// Settings for one input of alternate and mix.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AlternateFileOptions {
    /// Read this file from its last page backwards, to merge the two passes of a duplex scan.
    /// Default off.
    pub reverse: bool,
}

/// Alternate and mix (Appendix A row 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AlternateMixOptions {
    /// Pages taken from each file per turn, 1 to 1000. Default 1.
    pub pages_per_turn: u32,
    /// Per-file settings, matching the input order.
    pub per_file: Vec<AlternateFileOptions>,
}

impl Default for AlternateMixOptions {
    fn default() -> Self {
        Self {
            pages_per_turn: 1,
            per_file: Vec::new(),
        }
    }
}

impl AlternateMixOptions {
    /// Checks the turn size and the number of inputs (at least two).
    pub fn validate(&self, inputs: usize) -> Result<(), DomainError> {
        if !(1..=1000).contains(&self.pages_per_turn) {
            return Err(DomainError::option(
                "pages_per_turn",
                "must be between 1 and 1000",
            ));
        }
        if inputs < 2 {
            return Err(DomainError::option(
                "files",
                "add at least two files to mix",
            ));
        }
        if self.per_file.len() > inputs {
            return Err(DomainError::option(
                "per_file",
                "there are more per-file settings than files",
            ));
        }
        Ok(())
    }

    /// The `reverse` flag of every input, padded with false.
    pub fn reverse_flags(&self, inputs: usize) -> Vec<bool> {
        (0..inputs)
            .map(|i| self.per_file.get(i).is_some_and(|f| f.reverse))
            .collect()
    }
}

/// One page of the organized result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OrganizedPage {
    /// A page taken from an input file.
    Page {
        /// Index of the input file.
        source: u32,
        /// Zero-based page of that file.
        page: u32,
        /// Rotation added to the page.
        #[serde(default)]
        rotate: RightAngle,
    },
    /// A new blank page.
    Blank {
        /// Width in points.
        width_pt: f32,
        /// Height in points.
        height_pt: f32,
    },
}

/// Organize (Appendix A row 3): the final page list built in the organizer.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct OrganizeOptions {
    /// The pages of the result, in order. Empty means keep the first file as it is.
    pub pages: Vec<OrganizedPage>,
}

/// Most pages an organized result may have.
pub const MAX_ORGANIZED_PAGES: usize = 100_000;

impl OrganizeOptions {
    /// Checks every entry against the page counts of the inputs.
    pub fn validate(&self, page_counts: &[u32]) -> Result<(), DomainError> {
        if self.pages.len() > MAX_ORGANIZED_PAGES {
            return Err(DomainError::option("pages", "too many pages in the result"));
        }
        for entry in &self.pages {
            match entry {
                OrganizedPage::Page { source, page, .. } => {
                    let count = page_counts.get(*source as usize).ok_or_else(|| {
                        DomainError::option("pages", "a page refers to a file that is not open")
                    })?;
                    if page >= count {
                        return Err(DomainError::RangeOutOfBounds {
                            detail: format!("Page {} is outside file {}.", page + 1, source + 1),
                            valid: format!("1-{count}"),
                        });
                    }
                }
                OrganizedPage::Blank {
                    width_pt,
                    height_pt,
                } => {
                    let fine = |v: f32| v.is_finite() && (1.0..=14_400.0).contains(&v);
                    if !fine(*width_pt) || !fine(*height_pt) {
                        return Err(DomainError::option(
                            "pages",
                            "a blank page needs a size between 1 and 14,400 points",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn merge_defaults_match_appendix_a() {
        let d = MergeOptions::default();
        assert!(d.bookmark_per_file && !d.table_of_contents && !d.normalize_page_size);
        assert_eq!(d.forms, FormHandling::Keep);
        let f = d.file(7);
        assert_eq!(
            (f.pages.to_string().as_str(), f.rotation),
            ("all", RightAngle::Deg0)
        );
    }

    #[test]
    fn merge_pages_and_per_file_validation() {
        let o = MergeOptions {
            per_file: vec![MergeFileOptions {
                pages: PageRange::parse("2,2").unwrap(),
                rotation: RightAngle::Deg90,
            }],
            ..MergeOptions::default()
        };
        assert_eq!(
            o.pages_for(0, 3).unwrap().len(),
            2,
            "duplicates are allowed in merge"
        );
        assert_eq!(o.pages_for(1, 3).unwrap().len(), 3);
        assert_eq!(
            o.pages_for(0, 1).unwrap_err().code(),
            ErrorCode::RangeOutOfBounds
        );
        assert!(o.validate(1).is_ok());
        assert!(o.validate(0).is_err());
    }

    #[test]
    fn alternate_mix_checks_turn_and_input_count() {
        let d = AlternateMixOptions::default();
        assert_eq!(d.pages_per_turn, 1);
        assert!(d.validate(2).is_ok());
        assert!(d.validate(1).is_err());
        assert!(
            AlternateMixOptions {
                pages_per_turn: 0,
                ..d.clone()
            }
            .validate(2)
            .is_err()
        );
        assert!(
            AlternateMixOptions {
                pages_per_turn: 1001,
                ..d.clone()
            }
            .validate(2)
            .is_err()
        );
        let rev = AlternateMixOptions {
            per_file: vec![
                AlternateFileOptions { reverse: false },
                AlternateFileOptions { reverse: true },
            ],
            ..d
        };
        assert_eq!(rev.reverse_flags(3), vec![false, true, false]);
    }

    #[test]
    fn organize_validates_entries() {
        let page = |source, page| OrganizedPage::Page {
            source,
            page,
            rotate: RightAngle::Deg0,
        };
        let ok = OrganizeOptions {
            pages: vec![
                page(0, 1),
                page(1, 0),
                OrganizedPage::Blank {
                    width_pt: 612.0,
                    height_pt: 792.0,
                },
            ],
        };
        assert!(ok.validate(&[2, 1]).is_ok());
        assert_eq!(
            OrganizeOptions {
                pages: vec![page(0, 2)]
            }
            .validate(&[2])
            .unwrap_err()
            .code(),
            ErrorCode::RangeOutOfBounds
        );
        assert_eq!(
            OrganizeOptions {
                pages: vec![page(1, 0)]
            }
            .validate(&[2])
            .unwrap_err()
            .code(),
            ErrorCode::OptionInvalid
        );
        let bad = OrganizeOptions {
            pages: vec![OrganizedPage::Blank {
                width_pt: 0.0,
                height_pt: 10.0,
            }],
        };
        assert!(bad.validate(&[]).is_err());
    }

    #[test]
    fn organized_pages_use_a_kind_tag_and_default_rotation() {
        let json = r#"{"pages":[{"kind":"page","source":0,"page":3},{"kind":"blank","width_pt":100,"height_pt":200}]}"#;
        let o: OrganizeOptions = serde_json::from_str(json).unwrap();
        assert_eq!(
            o.pages[0],
            OrganizedPage::Page {
                source: 0,
                page: 3,
                rotate: RightAngle::Deg0
            }
        );
        assert!(serde_json::from_str::<OrganizeOptions>(r#"{"pages":[{"kind":"nope"}]}"#).is_err());
    }
}
