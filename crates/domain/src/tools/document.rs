//! Options of the document tools: Remove annotations (row 44), Flatten (row 47) and Edit
//! metadata (row 48).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::page_range::{Duplicates, PageRange, PageSelection};

/// A kind of annotation Remove annotations can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AnnotationKind {
    /// Every annotation except form fields and signatures (default).
    All,
    /// Highlight marks.
    Highlights,
    /// Strikeout marks.
    Strikeouts,
    /// Sticky notes and text comments.
    Notes,
    /// Links.
    Links,
}

/// Options for Remove annotations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RemoveAnnotationsOptions {
    /// Which kinds to remove. Default all.
    pub kinds: Vec<AnnotationKind>,
    /// Pages to clean: `all`, `even`, `odd` or a range such as `1,3-5`. Default `all`.
    pub pages: PageRange,
}

impl Default for RemoveAnnotationsOptions {
    fn default() -> Self {
        Self {
            kinds: vec![AnnotationKind::All],
            pages: PageRange::all(),
        }
    }
}

impl RemoveAnnotationsOptions {
    /// Checks that something is selected and resolves the pages.
    pub fn select(&self, page_count: u32) -> Result<PageSelection, DomainError> {
        if self.kinds.is_empty() {
            return Err(DomainError::option(
                "kinds",
                "choose at least one kind of annotation",
            ));
        }
        self.pages.resolve(page_count, Duplicates::Reject)
    }
}

/// What Flatten turns into page content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FlattenMode {
    /// Form fields and annotations become part of the page; text and vectors stay (default).
    #[default]
    FormsAndAnnotations,
    /// Every page becomes a picture. Text can no longer be selected or searched.
    Everything,
}

/// Resolution for [`FlattenMode::Everything`], in dots per inch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum FlattenDpi {
    /// 75 dpi, smallest.
    #[serde(rename = "75")]
    Dpi75,
    /// 100 dpi.
    #[serde(rename = "100")]
    Dpi100,
    /// 150 dpi (default).
    #[serde(rename = "150")]
    Dpi150,
    /// 220 dpi, sharpest.
    #[serde(rename = "220")]
    Dpi220,
}

impl FlattenDpi {
    /// The resolution in dots per inch.
    pub const fn value(self) -> u32 {
        match self {
            Self::Dpi75 => 75,
            Self::Dpi100 => 100,
            Self::Dpi150 => 150,
            Self::Dpi220 => 220,
        }
    }
}

/// Options for Flatten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct FlattenOptions {
    /// What to flatten. Default form fields and annotations only.
    pub mode: FlattenMode,
    /// Resolution when everything becomes a picture. Default 150.
    pub dpi: FlattenDpi,
}

impl Default for FlattenOptions {
    fn default() -> Self {
        Self {
            mode: FlattenMode::FormsAndAnnotations,
            dpi: FlattenDpi::Dpi150,
        }
    }
}

/// The value of the `Trapped` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Trapped {
    /// Trapping was applied.
    True,
    /// Trapping was not applied.
    False,
    /// Not known.
    Unknown,
}

impl Trapped {
    /// The PDF name written to the file.
    pub const fn pdf_name(self) -> &'static str {
        match self {
            Self::True => "True",
            Self::False => "False",
            Self::Unknown => "Unknown",
        }
    }
}

/// A document information field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MetadataField {
    /// Title.
    Title,
    /// Author.
    Author,
    /// Subject.
    Subject,
    /// Keywords.
    Keywords,
    /// The program that made the original document.
    Creator,
    /// The program that made the PDF.
    Producer,
    /// Creation date, as a PDF date string.
    CreationDate,
    /// Modification date, as a PDF date string.
    ModDate,
}

impl MetadataField {
    /// The key in the document information dictionary.
    pub const fn info_key(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Author => "Author",
            Self::Subject => "Subject",
            Self::Keywords => "Keywords",
            Self::Creator => "Creator",
            Self::Producer => "Producer",
            Self::CreationDate => "CreationDate",
            Self::ModDate => "ModDate",
        }
    }
}

/// The changes Edit metadata applies. A `None` value removes the field.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MetadataChange {
    /// Remove every field, the XMP packet included. Other entries are ignored.
    pub remove_all: bool,
    /// Fields to set or remove.
    pub fields: Vec<(MetadataField, Option<String>)>,
    /// New `Trapped` value.
    pub trapped: Option<Trapped>,
}

/// Options for Edit metadata. A field left out keeps its value; an empty text removes it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MetadataOptions {
    /// Title.
    pub title: Option<String>,
    /// Author.
    pub author: Option<String>,
    /// Subject.
    pub subject: Option<String>,
    /// Keywords, separated by commas.
    pub keywords: Option<String>,
    /// The program that made the original document.
    pub creator: Option<String>,
    /// The program that made the PDF.
    pub producer: Option<String>,
    /// Creation date as `2026-10-19` or `2026-10-19T14:30:00`.
    pub creation_date: Option<String>,
    /// Modification date, same format.
    pub modification_date: Option<String>,
    /// Trapped flag.
    pub trapped: Option<Trapped>,
    /// Remove all metadata. Default off.
    pub remove_all: bool,
}

/// Converts `2026-10-19`, `2026-10-19T14:30` or `2026-10-19T14:30:05` (an optional trailing `Z`
/// is accepted) to the PDF date format `D:20261019143005Z`.
pub fn pdf_date(text: &str) -> Result<String, DomainError> {
    let bad = || {
        DomainError::option(
            "date",
            format!("\"{text}\" is not a date; use 2026-10-19 or 2026-10-19T14:30:00"),
        )
    };
    let trimmed = text.trim().trim_end_matches('Z');
    let (date, time) = trimmed.split_once('T').unwrap_or((trimmed, "00:00:00"));
    let number = |s: &str, len: usize| -> Result<u32, DomainError> {
        if s.len() == len && s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse().map_err(|_| bad())
        } else {
            Err(bad())
        }
    };
    let d: Vec<&str> = date.split('-').collect();
    let t: Vec<&str> = time.split(':').collect();
    let (&[y, mo, da], true) = (d.as_slice(), (2..=3).contains(&t.len())) else {
        return Err(bad());
    };
    let (year, month, day) = (number(y, 4)?, number(mo, 2)?, number(da, 2)?);
    let hour = number(t[0], 2)?;
    let minute = number(t[1], 2)?;
    let second = if t.len() == 3 { number(t[2], 2)? } else { 0 };
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(bad()),
    };
    if day == 0 || day > days || hour > 23 || minute > 59 || second > 59 {
        return Err(bad());
    }
    Ok(format!(
        "D:{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}Z"
    ))
}

impl MetadataOptions {
    /// Checks the dates and builds the change. Nothing to change is an error.
    pub fn change(&self) -> Result<MetadataChange, DomainError> {
        if self.remove_all {
            return Ok(MetadataChange {
                remove_all: true,
                ..MetadataChange::default()
            });
        }
        let text = |value: &Option<String>| {
            value.as_ref().map(|v| {
                if v.trim().is_empty() {
                    None
                } else {
                    Some(v.clone())
                }
            })
        };
        let date = |value: &Option<String>| -> Result<Option<Option<String>>, DomainError> {
            match text(value) {
                None => Ok(None),
                Some(None) => Ok(Some(None)),
                Some(Some(v)) => Ok(Some(Some(pdf_date(&v)?))),
            }
        };
        let mut fields = Vec::new();
        let plain = [
            (MetadataField::Title, text(&self.title)),
            (MetadataField::Author, text(&self.author)),
            (MetadataField::Subject, text(&self.subject)),
            (MetadataField::Keywords, text(&self.keywords)),
            (MetadataField::Creator, text(&self.creator)),
            (MetadataField::Producer, text(&self.producer)),
        ];
        let dates = [
            (MetadataField::CreationDate, date(&self.creation_date)?),
            (MetadataField::ModDate, date(&self.modification_date)?),
        ];
        for (field, value) in plain.into_iter().chain(dates) {
            if let Some(value) = value {
                fields.push((field, value));
            }
        }
        if fields.is_empty() && self.trapped.is_none() {
            return Err(DomainError::option(
                "title",
                "change at least one field, or choose remove all",
            ));
        }
        Ok(MetadataChange {
            remove_all: false,
            fields,
            trapped: self.trapped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn annotation_defaults_cover_everything_and_an_empty_choice_is_rejected() {
        let d = RemoveAnnotationsOptions::default();
        assert_eq!(d.kinds, vec![AnnotationKind::All]);
        assert_eq!(d.select(4).unwrap().len(), 4);
        let none = RemoveAnnotationsOptions {
            kinds: vec![],
            ..d.clone()
        };
        assert_eq!(none.select(4).unwrap_err().code(), ErrorCode::OptionInvalid);
        let out = RemoveAnnotationsOptions {
            pages: PageRange::parse("9").unwrap(),
            ..d
        };
        assert_eq!(
            out.select(4).unwrap_err().code(),
            ErrorCode::RangeOutOfBounds
        );
    }

    #[test]
    fn flatten_defaults_match_appendix_a() {
        let d = FlattenOptions::default();
        assert_eq!(d.mode, FlattenMode::FormsAndAnnotations);
        assert_eq!(d.dpi.value(), 150);
        let o: FlattenOptions =
            serde_json::from_str(r#"{"mode":"everything","dpi":"220"}"#).unwrap();
        assert_eq!((o.mode, o.dpi.value()), (FlattenMode::Everything, 220));
        assert!(serde_json::from_str::<FlattenOptions>(r#"{"dpi":"300"}"#).is_err());
    }

    #[test]
    fn dates_become_pdf_dates() {
        assert_eq!(pdf_date("2026-10-19").unwrap(), "D:20261019000000Z");
        assert_eq!(pdf_date("2026-10-19T14:30").unwrap(), "D:20261019143000Z");
        assert_eq!(
            pdf_date("2026-10-19T14:30:05Z").unwrap(),
            "D:20261019143005Z"
        );
        assert_eq!(pdf_date("2024-02-29").unwrap(), "D:20240229000000Z");
    }

    #[test]
    fn impossible_dates_are_rejected() {
        for bad in [
            "",
            "yesterday",
            "2026-13-01",
            "2026-02-29",
            "2026-04-31",
            "2026-10-19T24:00",
            "2026-1-1",
            "2026-10-19T10",
        ] {
            assert_eq!(
                pdf_date(bad).unwrap_err().code(),
                ErrorCode::OptionInvalid,
                "{bad}"
            );
        }
    }

    #[test]
    fn metadata_leaves_missing_fields_alone_and_empty_text_removes() {
        let o = MetadataOptions {
            title: Some("Report".into()),
            author: Some("  ".into()),
            creation_date: Some("2026-01-02".into()),
            ..MetadataOptions::default()
        };
        let c = o.change().unwrap();
        assert!(
            c.fields
                .contains(&(MetadataField::Title, Some("Report".into())))
        );
        assert!(c.fields.contains(&(MetadataField::Author, None)));
        assert!(c.fields.contains(&(
            MetadataField::CreationDate,
            Some("D:20260102000000Z".into())
        )));
        assert!(!c.fields.iter().any(|(f, _)| *f == MetadataField::Subject));
    }

    #[test]
    fn metadata_needs_a_change_and_remove_all_wins() {
        assert!(MetadataOptions::default().change().is_err());
        let all = MetadataOptions {
            remove_all: true,
            title: Some("ignored".into()),
            ..MetadataOptions::default()
        };
        let c = all.change().unwrap();
        assert!(c.remove_all && c.fields.is_empty());
        let trapped = MetadataOptions {
            trapped: Some(Trapped::False),
            ..MetadataOptions::default()
        };
        assert_eq!(trapped.change().unwrap().trapped, Some(Trapped::False));
    }
}
