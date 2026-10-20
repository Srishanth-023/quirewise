//! Options of the archive and accessibility tools: PDF to PDF/A (row 30), Validate PDF/A (row 31)
//! and the PDF/UA accessibility check (row 70).
//!
//! PDF/A is the family of standards for documents that must look the same decades from now. Each
//! part (1, 2, 3) is a level of the PDF base it builds on, and each conformance level says how
//! much is promised: `b` the pages look the same, `u` also that the text can be copied right, `a`
//! also that the document is tagged for accessibility.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A level of PDF/A.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
pub enum PdfaLevel {
    /// PDF/A-1a: PDF 1.4, tagged.
    #[serde(rename = "1a")]
    A1a,
    /// PDF/A-1b: PDF 1.4, visual appearance.
    #[serde(rename = "1b")]
    A1b,
    /// PDF/A-2a: PDF 1.7, tagged.
    #[serde(rename = "2a")]
    A2a,
    /// PDF/A-2b: PDF 1.7, visual appearance (default).
    #[default]
    #[serde(rename = "2b")]
    A2b,
    /// PDF/A-2u: PDF 1.7, visual appearance and text that can be copied.
    #[serde(rename = "2u")]
    A2u,
    /// PDF/A-3a: PDF 1.7 with any attachments, tagged.
    #[serde(rename = "3a")]
    A3a,
    /// PDF/A-3b: PDF 1.7 with any attachments, visual appearance.
    #[serde(rename = "3b")]
    A3b,
    /// PDF/A-3u: PDF 1.7 with any attachments, text that can be copied.
    #[serde(rename = "3u")]
    A3u,
}

impl PdfaLevel {
    /// Every level, in the order of the standards.
    pub const ALL: [PdfaLevel; 8] = [
        Self::A1a,
        Self::A1b,
        Self::A2a,
        Self::A2b,
        Self::A2u,
        Self::A3a,
        Self::A3b,
        Self::A3u,
    ];

    /// The part of the standard: 1, 2 or 3.
    pub const fn part(self) -> u8 {
        match self {
            Self::A1a | Self::A1b => 1,
            Self::A2a | Self::A2b | Self::A2u => 2,
            Self::A3a | Self::A3b | Self::A3u => 3,
        }
    }

    /// The conformance letter in upper case: `A`, `B` or `U`.
    pub const fn conformance(self) -> char {
        match self {
            Self::A1a | Self::A2a | Self::A3a => 'A',
            Self::A1b | Self::A2b | Self::A3b => 'B',
            Self::A2u | Self::A3u => 'U',
        }
    }

    /// The name used on the wire and by validators, such as `2b`.
    pub const fn code(self) -> &'static str {
        match self {
            Self::A1a => "1a",
            Self::A1b => "1b",
            Self::A2a => "2a",
            Self::A2b => "2b",
            Self::A2u => "2u",
            Self::A3a => "3a",
            Self::A3b => "3b",
            Self::A3u => "3u",
        }
    }

    /// The name for people, such as `PDF/A-2b`.
    pub fn name(self) -> String {
        format!("PDF/A-{}", self.code())
    }

    /// The level with this code, such as `2b`.
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.code() == code)
    }

    /// True when the document must be tagged.
    pub const fn needs_tags(self) -> bool {
        self.conformance() == 'A'
    }

    /// True when every character must have a Unicode value (levels `u` and `a`).
    pub const fn needs_unicode(self) -> bool {
        matches!(self.conformance(), 'A' | 'U')
    }

    /// True when PDF 1.4 rules apply: no transparency, layers, JPEG 2000 or attachments.
    pub const fn is_pdf14(self) -> bool {
        self.part() == 1
    }

    /// True when any file may be attached (part 3).
    pub const fn allows_attachments(self) -> bool {
        self.part() == 3
    }

    /// The levels to try, in order, when this one cannot be reached: first the same part with a
    /// weaker promise, then (from part 1) part 2, which accepts what part 1 cannot.
    pub fn fallbacks(self) -> Vec<Self> {
        match self {
            Self::A1a => vec![Self::A1b, Self::A2b],
            Self::A1b => vec![Self::A2b],
            Self::A2a => vec![Self::A2u, Self::A2b],
            Self::A2u => vec![Self::A2b],
            Self::A3a => vec![Self::A3u, Self::A3b],
            Self::A3u => vec![Self::A3b],
            Self::A2b | Self::A3b => Vec::new(),
        }
    }
}

/// How a report is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReportFormat {
    /// A web page to read (default).
    #[default]
    Html,
    /// Data for other programs.
    Json,
}

impl ReportFormat {
    /// The file extension.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Json => "json",
        }
    }
}

/// Options for PDF to PDF/A.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PdfToPdfaOptions {
    /// The level to reach. Default `2b`.
    pub level: PdfaLevel,
    /// When the level cannot be reached, try the next weaker one instead of failing. Default on.
    pub allow_downgrade: bool,
    /// When a page still cannot be made to conform, replace the document by pictures of its pages.
    /// The result conforms but the text can no longer be selected. Default off.
    pub rasterize_if_needed: bool,
}

impl Default for PdfToPdfaOptions {
    fn default() -> Self {
        Self {
            level: PdfaLevel::A2b,
            allow_downgrade: true,
            rasterize_if_needed: false,
        }
    }
}

/// Options for Validate PDF/A.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ValidatePdfaOptions {
    /// The level to check against. Default: the level the file claims, or `1b` when it claims none.
    pub level: Option<PdfaLevel>,
    /// How to write the report. Default a web page.
    pub format: ReportFormat,
}

/// Options for the PDF/UA accessibility check.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CheckPdfuaOptions {
    /// How to write the report. Default a web page.
    pub format: ReportFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_have_part_conformance_and_name() {
        assert_eq!(PdfaLevel::A2b.part(), 2);
        assert_eq!(PdfaLevel::A3u.conformance(), 'U');
        assert_eq!(PdfaLevel::A1a.name(), "PDF/A-1a");
        assert_eq!(PdfaLevel::default(), PdfaLevel::A2b);
        for l in PdfaLevel::ALL {
            assert_eq!(PdfaLevel::from_code(l.code()), Some(l));
        }
        assert_eq!(PdfaLevel::from_code("4"), None);
    }

    #[test]
    fn promises_follow_the_conformance_letter() {
        assert!(PdfaLevel::A2a.needs_tags() && PdfaLevel::A2a.needs_unicode());
        assert!(!PdfaLevel::A2u.needs_tags() && PdfaLevel::A2u.needs_unicode());
        assert!(!PdfaLevel::A2b.needs_tags() && !PdfaLevel::A2b.needs_unicode());
        assert!(PdfaLevel::A1b.is_pdf14() && !PdfaLevel::A2b.is_pdf14());
        assert!(PdfaLevel::A3b.allows_attachments() && !PdfaLevel::A2b.allows_attachments());
    }

    #[test]
    fn fallbacks_get_weaker_and_end() {
        assert_eq!(
            PdfaLevel::A3a.fallbacks(),
            vec![PdfaLevel::A3u, PdfaLevel::A3b]
        );
        assert_eq!(
            PdfaLevel::A1a.fallbacks(),
            vec![PdfaLevel::A1b, PdfaLevel::A2b]
        );
        assert!(PdfaLevel::A2b.fallbacks().is_empty());
        // Following the chain always ends, and never returns to a level.
        for l in PdfaLevel::ALL {
            let chain = l.fallbacks();
            assert!(!chain.contains(&l));
            for next in &chain {
                assert!(next.fallbacks().iter().all(|n| chain.contains(n)));
            }
        }
    }

    #[test]
    fn levels_use_their_codes_on_the_wire() {
        let o: PdfToPdfaOptions = serde_json::from_str(r#"{"level":"3u"}"#).unwrap();
        assert_eq!(o.level, PdfaLevel::A3u);
        assert!(o.allow_downgrade && !o.rasterize_if_needed);
        assert_eq!(serde_json::to_string(&PdfaLevel::A1b).unwrap(), "\"1b\"");
        let v: ValidatePdfaOptions = serde_json::from_str(r#"{"format":"json"}"#).unwrap();
        assert_eq!((v.level, v.format), (None, ReportFormat::Json));
        assert!(serde_json::from_str::<PdfToPdfaOptions>(r#"{"level":"4"}"#).is_err());
        assert!(serde_json::from_str::<CheckPdfuaOptions>(r#"{"nope":1}"#).is_err());
    }

    #[test]
    fn report_formats_have_extensions() {
        assert_eq!(ReportFormat::Html.extension(), "html");
        assert_eq!(ReportFormat::Json.extension(), "json");
        assert_eq!(ReportFormat::default(), ReportFormat::Html);
    }
}
