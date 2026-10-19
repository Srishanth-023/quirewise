//! Options of the tools that need an office suite or a web view: Word to PDF (row 19), PowerPoint
//! to PDF (row 20), Excel to PDF (row 21) and HTML to PDF (row 22).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::page_range::PageRange;
use crate::tools::layout::PaperSize;

/// Which program converts the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OfficeEngine {
    /// Microsoft Office when it is installed, otherwise LibreOffice (default).
    #[default]
    Auto,
    /// LibreOffice, from the component list or an installed copy.
    LibreOffice,
    /// Microsoft Office on this computer.
    MsOffice,
}

/// Options for Word to PDF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct WordToPdfOptions {
    /// Pages of the result to keep: `all`, a range, `even`, `odd`. Default `all`.
    pub pages: PageRange,
    /// The program that converts the file. Default auto.
    pub engine: OfficeEngine,
}

impl Default for WordToPdfOptions {
    fn default() -> Self {
        Self {
            pages: PageRange::all(),
            engine: OfficeEngine::Auto,
        }
    }
}

/// Options for PowerPoint to PDF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PowerPointToPdfOptions {
    /// Slides to keep, counting the slides that are in the result. Default `all`.
    pub slides: PageRange,
    /// Include slides that are hidden in the presentation. Default off.
    pub include_hidden: bool,
    /// The program that converts the file. Default auto.
    pub engine: OfficeEngine,
}

impl Default for PowerPointToPdfOptions {
    fn default() -> Self {
        Self {
            slides: PageRange::all(),
            include_hidden: false,
            engine: OfficeEngine::Auto,
        }
    }
}

/// Page turn of the sheets of a workbook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SheetOrientation {
    /// What each sheet already has (default).
    #[default]
    Auto,
    /// Every sheet upright.
    Portrait,
    /// Every sheet on its side.
    Landscape,
}

/// Options for Excel to PDF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ExcelToPdfOptions {
    /// Sheets to print, by position: `all` or for example `1,3`. Default `all`.
    pub sheets: PageRange,
    /// Shrink wide sheets so every column fits the page width. Default on.
    pub fit_to_page_width: bool,
    /// Page turn. Default auto.
    pub orientation: SheetOrientation,
    /// The program that converts the file. Default auto.
    pub engine: OfficeEngine,
}

impl Default for ExcelToPdfOptions {
    fn default() -> Self {
        Self {
            sheets: PageRange::all(),
            fit_to_page_width: true,
            orientation: SheetOrientation::Auto,
            engine: OfficeEngine::Auto,
        }
    }
}

/// Page turn of a printed web page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum HtmlOrientation {
    /// Upright (default).
    #[default]
    Portrait,
    /// On its side.
    Landscape,
}

/// Longest a page may take to load, in seconds.
pub const MAX_LOAD_TIMEOUT_SECONDS: u32 = 300;

/// Options for HTML to PDF. The file to print comes from the file list, or from `url`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct HtmlToPdfOptions {
    /// A web address to print instead of a file. Needs `allow_network`. Default none.
    pub url: Option<String>,
    /// Allow the page to load from the internet. Without it only local files print and pictures
    /// or styles that live on the web are left out. Default off.
    pub allow_network: bool,
    /// Paper size: A4, Letter, Legal, A3 or custom. Default A4.
    pub page_size: PaperSize,
    /// Width in millimeters, for a custom size. Default 210.
    pub custom_width_mm: f32,
    /// Height in millimeters, for a custom size. Default 297.
    pub custom_height_mm: f32,
    /// Page turn. Default portrait.
    pub orientation: HtmlOrientation,
    /// Margin on every side in millimeters. Default 10.
    pub margin_mm: f32,
    /// Print background colors and pictures. Default on.
    pub background_graphics: bool,
    /// Scale in percent, 10 to 200. Default 100.
    pub scale_percent: u32,
    /// Print the page title and number in the margins. Default off.
    pub header_footer: bool,
    /// Longest the page may take to load, in seconds, 1 to 300. Default 15.
    pub load_timeout_s: u32,
}

impl Default for HtmlToPdfOptions {
    fn default() -> Self {
        Self {
            url: None,
            allow_network: false,
            page_size: PaperSize::A4,
            custom_width_mm: 210.0,
            custom_height_mm: 297.0,
            orientation: HtmlOrientation::Portrait,
            margin_mm: 10.0,
            background_graphics: true,
            scale_percent: 100,
            header_footer: false,
            load_timeout_s: 15,
        }
    }
}

/// Millimeters in an inch.
const MM_PER_INCH: f32 = 25.4;

impl HtmlToPdfOptions {
    /// The address to print, when one was given: it must be an `https` or `http` address.
    pub fn address(&self) -> Result<Option<&str>, DomainError> {
        let Some(url) = self.url.as_deref().map(str::trim).filter(|u| !u.is_empty()) else {
            return Ok(None);
        };
        if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(DomainError::option("url", "the address has spaces in it"));
        }
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(DomainError::option(
                "url",
                "the address must start with https:// or http://",
            ));
        }
        Ok(Some(url))
    }

    /// Checks everything that does not depend on the page.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.address()?.is_some() && !self.allow_network {
            return Err(DomainError::NetworkConsent);
        }
        if !(10..=200).contains(&self.scale_percent) {
            return Err(DomainError::option(
                "scale_percent",
                "must be between 10 and 200",
            ));
        }
        if !(1..=MAX_LOAD_TIMEOUT_SECONDS).contains(&self.load_timeout_s) {
            return Err(DomainError::option(
                "load_timeout_s",
                format!("must be between 1 and {MAX_LOAD_TIMEOUT_SECONDS}"),
            ));
        }
        if !(0.0..=100.0).contains(&self.margin_mm) {
            return Err(DomainError::option(
                "margin_mm",
                "must be between 0 and 100 millimeters",
            ));
        }
        let (w, h) = self.paper_inches()?;
        let margin = self.margin_mm / MM_PER_INCH;
        if w <= 2.0 * margin + 0.1 || h <= 2.0 * margin + 0.1 {
            return Err(DomainError::option(
                "margin_mm",
                "the margins leave no room for the page",
            ));
        }
        Ok(())
    }

    /// Paper width and height in inches after the page turn.
    pub fn paper_inches(&self) -> Result<(f32, f32), DomainError> {
        let (w_mm, h_mm) = match self.page_size {
            PaperSize::A4 => (210.0, 297.0),
            PaperSize::A3 => (297.0, 420.0),
            PaperSize::Letter => (215.9, 279.4),
            PaperSize::Legal => (215.9, 355.6),
            PaperSize::Custom => {
                for (name, v) in [
                    ("custom_width_mm", self.custom_width_mm),
                    ("custom_height_mm", self.custom_height_mm),
                ] {
                    if !(20.0..=5000.0).contains(&v) {
                        return Err(DomainError::option(
                            name,
                            "must be between 20 and 5000 millimeters",
                        ));
                    }
                }
                (self.custom_width_mm, self.custom_height_mm)
            }
        };
        let (w, h) = (w_mm / MM_PER_INCH, h_mm / MM_PER_INCH);
        Ok(match self.orientation {
            HtmlOrientation::Portrait => (w, h),
            HtmlOrientation::Landscape => (h, w),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_feature_matrix() {
        let word = WordToPdfOptions::default();
        assert_eq!(word.engine, OfficeEngine::Auto);
        let slides = PowerPointToPdfOptions::default();
        assert!(!slides.include_hidden);
        let sheets = ExcelToPdfOptions::default();
        assert!(sheets.fit_to_page_width);
        let html = HtmlToPdfOptions::default();
        assert_eq!(html.page_size, PaperSize::A4);
        assert_eq!(html.margin_mm, 10.0);
        assert!(html.background_graphics);
        assert_eq!(html.scale_percent, 100);
        assert!(!html.header_footer);
        assert_eq!(html.load_timeout_s, 15);
        assert!(html.validate().is_ok());
    }

    #[test]
    fn a_web_address_needs_consent_and_a_proper_scheme() {
        let with = |url: &str, allow: bool| HtmlToPdfOptions {
            url: Some(url.to_owned()),
            allow_network: allow,
            ..HtmlToPdfOptions::default()
        };
        assert_eq!(
            with("https://example.com/a", false).validate(),
            Err(DomainError::NetworkConsent)
        );
        assert!(with("https://example.com/a", true).validate().is_ok());
        assert!(with("ftp://example.com", true).validate().is_err());
        assert!(with("file:///C:/secret.txt", true).validate().is_err());
        assert!(with("https://exa mple.com", true).validate().is_err());
        assert_eq!(with("  ", false).address(), Ok(None));
    }

    #[test]
    fn numbers_are_bounded() {
        let bad = |f: &dyn Fn(&mut HtmlToPdfOptions)| {
            let mut o = HtmlToPdfOptions::default();
            f(&mut o);
            o.validate().is_err()
        };
        assert!(bad(&|o| o.scale_percent = 5));
        assert!(bad(&|o| o.scale_percent = 500));
        assert!(bad(&|o| o.load_timeout_s = 0));
        assert!(bad(&|o| o.load_timeout_s = 301));
        assert!(bad(&|o| o.margin_mm = 150.0));
        assert!(bad(&|o| o.margin_mm = 105.0));
        assert!(bad(&|o| {
            o.page_size = PaperSize::Custom;
            o.custom_width_mm = 5.0;
        }));
    }

    #[test]
    fn paper_turns_with_the_orientation() {
        let mut o = HtmlToPdfOptions::default();
        let (w, h) = o.paper_inches().unwrap();
        assert!(w < h && (w - 8.268).abs() < 0.01);
        o.orientation = HtmlOrientation::Landscape;
        let (w2, h2) = o.paper_inches().unwrap();
        assert_eq!((w2, h2), (h, w));
    }
}
