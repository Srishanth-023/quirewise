//! Options of Redact (row 54): what to remove from a document for good.
//!
//! What to remove is given three ways that can be mixed: areas drawn on pages, words to look for
//! and patterns to look for. Areas are fractions of the page as it is displayed, measured from the
//! top left, like the regions of the other tools. Pages count from 1.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::page_range::{Duplicates, PageRange, PageSelection};
use crate::stamp::Rgb;
use crate::tools::ai::AiOptions;
use crate::tools::common::Region;

/// Most areas one run may carry.
pub const MAX_AREAS: usize = 5_000;
/// Most words, patterns and presets of one kind a run may carry.
pub const MAX_TERMS: usize = 500;
/// Longest word or pattern, in characters.
pub const MAX_TERM_CHARS: usize = 500;

/// Kinds of text that are found by their shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RedactPreset {
    /// E-mail addresses.
    Email,
    /// Telephone numbers: seven or more digits, with the usual signs between them.
    Phone,
    /// International bank account numbers whose check digits are right.
    Iban,
    /// Payment card numbers of 13 to 19 digits that pass the Luhn check.
    CardNumber,
}

/// A rectangle of one page to remove.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RedactArea {
    /// The page, from 1.
    pub page: u32,
    /// The rectangle, as fractions of the displayed page.
    pub region: Region,
}

/// A place on a page where the search of Redact found something.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RedactMatch {
    /// The page, from 1.
    pub page: u32,
    /// The characters that matched, as fractions of the displayed page.
    pub region: Region,
}

/// Options for Redact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RedactOptions {
    /// Rectangles drawn on pages.
    pub areas: Vec<RedactArea>,
    /// Words or phrases to find. Case does not matter unless `case_sensitive` is on.
    pub terms: Vec<String>,
    /// Kinds of text to find by their shape.
    pub presets: Vec<RedactPreset>,
    /// Patterns to find, as regular expressions.
    pub patterns: Vec<String>,
    /// Match the case of `terms` and `patterns`. Default off.
    pub case_sensitive: bool,
    /// Pages the search covers: `all`, `even`, `odd` or a range such as `1,3-5`. Areas always
    /// apply to their own page. Default `all`.
    pub pages: PageRange,
    /// The color that covers what was removed, as `#RRGGBB`. Default black.
    pub fill_color: String,
    /// Clear the document information and the XMP packet. Default on.
    pub strip_metadata: bool,
    /// Also remove what the local model finds: names, addresses and other personal details
    /// that no pattern describes. The model can miss things, so read the preview. Default off.
    pub ai_suggestions: bool,
    /// The model for `ai_suggestions`.
    pub ai: AiOptions,
}

impl Default for RedactOptions {
    fn default() -> Self {
        Self {
            areas: Vec::new(),
            terms: Vec::new(),
            presets: Vec::new(),
            patterns: Vec::new(),
            case_sensitive: false,
            pages: PageRange::all(),
            fill_color: "#000000".to_owned(),
            strip_metadata: true,
            ai_suggestions: false,
            ai: AiOptions::default(),
        }
    }
}

impl RedactOptions {
    /// True when something is to be searched for.
    pub fn searches(&self) -> bool {
        !self.terms.is_empty() || !self.presets.is_empty() || !self.patterns.is_empty()
    }

    /// True when the pages are read for something: the words, patterns and presets, or the
    /// local model.
    pub fn reads_pages(&self) -> bool {
        self.searches() || self.ai_suggestions
    }

    /// The pages the search covers.
    pub fn search_pages(&self, count: u32) -> Result<PageSelection, DomainError> {
        self.pages.resolve(count, Duplicates::Reject)
    }

    /// The color that covers removed content.
    pub fn fill(&self) -> Result<Rgb, DomainError> {
        Rgb::parse(&self.fill_color)
            .map_err(|_| DomainError::option("fill_color", "use a color such as #000000"))
    }

    /// Checks everything against a document of `count` pages. Something to remove is needed.
    pub fn validate(&self, count: u32) -> Result<(), DomainError> {
        if self.areas.is_empty() && !self.reads_pages() {
            return Err(DomainError::option(
                "areas",
                "draw an area, or enter words or a pattern to remove",
            ));
        }
        if self.ai_suggestions {
            self.ai.validate()?;
        }
        if self.areas.len() > MAX_AREAS {
            return Err(DomainError::option("areas", "too many areas"));
        }
        for area in &self.areas {
            if area.page == 0 || area.page > count {
                return Err(DomainError::option(
                    "areas",
                    format!("page {} is not in the document", area.page),
                ));
            }
            area.region.validate()?;
        }
        for (field, list) in [("terms", &self.terms), ("patterns", &self.patterns)] {
            if list.len() > MAX_TERMS {
                return Err(DomainError::option(field, "too many entries"));
            }
            for entry in list {
                if entry.trim().is_empty() {
                    return Err(DomainError::option(field, "an entry is empty"));
                }
                if entry.chars().count() > MAX_TERM_CHARS {
                    return Err(DomainError::option(
                        field,
                        format!("an entry is longer than {MAX_TERM_CHARS} characters"),
                    ));
                }
            }
        }
        self.fill()?;
        if self.reads_pages() {
            self.search_pages(count)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(page: u32) -> RedactArea {
        RedactArea {
            page,
            region: Region {
                x: 0.1,
                y: 0.1,
                width: 0.2,
                height: 0.05,
            },
        }
    }

    #[test]
    fn something_to_remove_is_needed() {
        assert!(RedactOptions::default().validate(3).is_err());
        let area_only = RedactOptions {
            areas: vec![area(2)],
            ..RedactOptions::default()
        };
        assert!(area_only.validate(3).is_ok());
        let words_only = RedactOptions {
            terms: vec!["secret".into()],
            ..RedactOptions::default()
        };
        assert!(words_only.validate(3).is_ok());
        let preset_only = RedactOptions {
            presets: vec![RedactPreset::Email],
            ..RedactOptions::default()
        };
        assert!(preset_only.validate(3).is_ok());
    }

    #[test]
    fn the_model_alone_can_be_what_to_remove_and_is_checked() {
        let mut o = RedactOptions {
            ai_suggestions: true,
            ..RedactOptions::default()
        };
        assert!(o.validate(3).is_ok());
        o.ai.model = Some("not-in-the-catalog".into());
        assert!(o.validate(3).is_err());
        o.ai_suggestions = false;
        assert!(o.validate(3).is_err(), "nothing to remove again");
    }

    #[test]
    fn areas_must_be_on_a_page_of_the_document_and_inside_it() {
        let outside = RedactOptions {
            areas: vec![area(4)],
            ..RedactOptions::default()
        };
        assert!(outside.validate(3).is_err());
        let zero = RedactOptions {
            areas: vec![area(0)],
            ..RedactOptions::default()
        };
        assert!(zero.validate(3).is_err());
        let mut bad = area(1);
        bad.region.width = 2.0;
        let wide = RedactOptions {
            areas: vec![bad],
            ..RedactOptions::default()
        };
        assert!(wide.validate(3).is_err());
    }

    #[test]
    fn blank_and_oversized_entries_are_refused() {
        let blank = RedactOptions {
            terms: vec!["  ".into()],
            ..RedactOptions::default()
        };
        assert!(blank.validate(1).is_err());
        let long = RedactOptions {
            patterns: vec!["a".repeat(MAX_TERM_CHARS + 1)],
            ..RedactOptions::default()
        };
        assert!(long.validate(1).is_err());
    }

    #[test]
    fn the_fill_color_and_the_search_pages_are_checked() {
        let color = RedactOptions {
            areas: vec![area(1)],
            fill_color: "red".into(),
            ..RedactOptions::default()
        };
        assert!(color.validate(1).is_err());
        let pages = RedactOptions {
            terms: vec!["x".into()],
            pages: PageRange::parse("9").unwrap(),
            ..RedactOptions::default()
        };
        assert!(pages.validate(2).is_err());
        assert!(pages.validate(9).is_ok());
        // The search range is not looked at when only areas are given.
        let areas_only = RedactOptions {
            areas: vec![area(1)],
            pages: PageRange::parse("9").unwrap(),
            ..RedactOptions::default()
        };
        assert!(areas_only.validate(2).is_ok());
        assert_eq!(RedactOptions::default().fill().unwrap(), Rgb::BLACK);
    }

    #[test]
    fn options_read_from_json_with_defaults_and_refuse_unknown_keys() {
        let options: RedactOptions =
            serde_json::from_str(r#"{"terms":["Ada"],"presets":["card-number","iban"]}"#).unwrap();
        assert_eq!(
            options.presets,
            [RedactPreset::CardNumber, RedactPreset::Iban]
        );
        assert!(options.strip_metadata);
        assert!(serde_json::from_str::<RedactOptions>(r#"{"nope":1}"#).is_err());
    }
}
