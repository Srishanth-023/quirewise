//! Options of the tools that read scans: OCR (row 16), Deskew (row 17) and Scan to PDF (row 13).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::page_range::PageRange;
use crate::tools::convert::PictureColor;
use crate::units::Dpi;

/// Most languages one recognition run may combine.
pub const MAX_LANGUAGES: usize = 6;
/// Smallest and largest resolution pages are read at.
pub const OCR_DPI_RANGE: (u32, u32) = (100, 600);

/// What OCR writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OcrOutput {
    /// The document with an invisible text layer, so it can be searched and copied (default).
    #[default]
    Pdf,
    /// A text file with the recognized text.
    Text,
}

fn default_languages() -> Vec<String> {
    vec!["eng".to_owned()]
}

/// Checks one Tesseract language code such as `eng`, `deu` or `chi_sim`.
pub fn check_language(code: &str) -> Result<(), DomainError> {
    let ok = (3..=16).contains(&code.len())
        && code.starts_with(|c: char| c.is_ascii_lowercase())
        && code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(DomainError::option(
            "languages",
            format!("\"{code}\" is not a language code such as eng, deu or chi_sim"),
        ))
    }
}

/// Checks a list of language codes: one to six, each valid, none twice.
pub fn check_languages(languages: &[String]) -> Result<(), DomainError> {
    if languages.is_empty() {
        return Err(DomainError::option(
            "languages",
            "choose at least one language",
        ));
    }
    if languages.len() > MAX_LANGUAGES {
        return Err(DomainError::option(
            "languages",
            format!("choose at most {MAX_LANGUAGES} languages"),
        ));
    }
    for (i, code) in languages.iter().enumerate() {
        check_language(code)?;
        if languages[..i].contains(code) {
            return Err(DomainError::option(
                "languages",
                format!("\"{code}\" is listed twice"),
            ));
        }
    }
    Ok(())
}

/// The languages in the form Tesseract's `-l` takes: `eng+deu`.
pub fn language_argument(languages: &[String]) -> String {
    languages.join("+")
}

/// Options for OCR.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct OcrOptions {
    /// Tesseract language codes, such as `eng` or `deu`. Default `eng`.
    pub languages: Vec<String>,
    /// A searchable PDF or a text file. Default PDF.
    pub output: OcrOutput,
    /// Straighten tilted pages before reading them. Default off.
    pub deskew: bool,
    /// Pages to read. Default `all`.
    pub pages: PageRange,
    /// Leave pages that already have text alone. Default on.
    pub skip_pages_with_text: bool,
    /// Resolution pages are read at, 100 to 600. Default 300.
    pub dpi: Dpi,
}

impl Default for OcrOptions {
    fn default() -> Self {
        Self {
            languages: default_languages(),
            output: OcrOutput::Pdf,
            deskew: false,
            pages: PageRange::all(),
            skip_pages_with_text: true,
            dpi: Dpi::new(300).unwrap_or_default(),
        }
    }
}

impl OcrOptions {
    /// Checks the languages and the resolution.
    pub fn validate(&self) -> Result<(), DomainError> {
        check_languages(&self.languages)?;
        check_dpi(self.dpi)
    }
}

fn check_dpi(dpi: Dpi) -> Result<(), DomainError> {
    let (low, high) = OCR_DPI_RANGE;
    if (low..=high).contains(&dpi.get()) {
        Ok(())
    } else {
        Err(DomainError::option(
            "dpi",
            format!("must be between {low} and {high}"),
        ))
    }
}

/// How Deskew finds the angle of each page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DeskewMode {
    /// Measure the tilt of each page (default).
    #[default]
    Auto,
    /// Use the angles typed in the list.
    Manual,
}

/// An angle for one page, for manual deskew.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageAngle {
    /// The page, counting from 1.
    pub page: u32,
    /// How far to turn the content counter-clockwise, in degrees. Negative turns it clockwise.
    pub degrees: f32,
}

/// Largest tilt Deskew will correct, in degrees.
pub const MAX_DESKEW_DEGREES: f32 = 45.0;

/// Options for Deskew.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DeskewOptions {
    /// Measure the tilt (default) or use the typed angles.
    pub mode: DeskewMode,
    /// The angles of single pages, for manual deskew.
    pub angles: Vec<PageAngle>,
    /// Pages to straighten, for automatic deskew. Default `all`.
    pub pages: PageRange,
    /// The largest tilt that is corrected automatically, in degrees, 1 to 45. Default 15.
    pub max_angle_deg: f32,
}

impl Default for DeskewOptions {
    fn default() -> Self {
        Self {
            mode: DeskewMode::Auto,
            angles: Vec::new(),
            pages: PageRange::all(),
            max_angle_deg: 15.0,
        }
    }
}

impl DeskewOptions {
    /// Checks the angle limit and, for manual mode, the typed angles against the page count.
    pub fn validate(&self, page_count: u32) -> Result<(), DomainError> {
        if !(1.0..=MAX_DESKEW_DEGREES).contains(&self.max_angle_deg) {
            return Err(DomainError::option(
                "max_angle_deg",
                format!("must be between 1 and {MAX_DESKEW_DEGREES}"),
            ));
        }
        if self.mode == DeskewMode::Manual {
            if self.angles.is_empty() {
                return Err(DomainError::option(
                    "angles",
                    "add the angle of at least one page",
                ));
            }
            for (i, a) in self.angles.iter().enumerate() {
                if a.page == 0 || a.page > page_count {
                    return Err(DomainError::option(
                        "angles",
                        format!(
                            "page {} is outside this document of {page_count} pages",
                            a.page
                        ),
                    ));
                }
                if !a.degrees.is_finite() || a.degrees.abs() > 90.0 {
                    return Err(DomainError::option(
                        "angles",
                        "an angle must be between -90 and 90 degrees",
                    ));
                }
                if self.angles[..i].iter().any(|b| b.page == a.page) {
                    return Err(DomainError::option(
                        "angles",
                        format!("page {} has two angles", a.page),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Options for Scan to PDF. The pictures in the file list are the scanned pages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ScanToPdfOptions {
    /// Cut away what is not paper around the page. Default on.
    pub auto_crop: bool,
    /// Straighten pages photographed at an angle. Default on.
    pub perspective_fix: bool,
    /// Colors of the pages. Default color.
    pub color: PictureColor,
    /// Resolution of the scan, which sets the page size, 100 to 600. Default 300.
    pub dpi: Dpi,
    /// Make the PDF searchable. Default off.
    pub ocr: bool,
    /// Languages for `ocr`. Default `eng`.
    pub languages: Vec<String>,
}

impl Default for ScanToPdfOptions {
    fn default() -> Self {
        Self {
            auto_crop: true,
            perspective_fix: true,
            color: PictureColor::Color,
            dpi: Dpi::new(300).unwrap_or_default(),
            ocr: false,
            languages: default_languages(),
        }
    }
}

impl ScanToPdfOptions {
    /// Checks the resolution, and the languages when OCR is on.
    pub fn validate(&self) -> Result<(), DomainError> {
        check_dpi(self.dpi)?;
        if self.ocr {
            check_languages(&self.languages)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_codes_are_checked_and_joined_for_tesseract() {
        for good in ["eng", "deu", "chi_sim", "chi_sim_vert", "deu_frak"] {
            assert!(check_language(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "en",
            "ENG",
            "e n g",
            "eng;rm",
            "../eng",
            "a".repeat(17).as_str(),
            "1eng",
        ] {
            assert!(check_language(bad).is_err(), "{bad}");
        }
        assert_eq!(
            language_argument(&["eng".to_owned(), "deu".to_owned()]),
            "eng+deu"
        );
    }

    #[test]
    fn language_lists_are_bounded_and_without_repeats() {
        let l = |codes: &[&str]| codes.iter().map(|c| (*c).to_owned()).collect::<Vec<_>>();
        assert!(check_languages(&l(&["eng", "deu"])).is_ok());
        assert!(check_languages(&[]).is_err());
        assert!(check_languages(&l(&["eng", "eng"])).is_err());
        assert!(check_languages(&l(&["eng", "deu", "fra", "spa", "ita", "por", "nld"])).is_err());
    }

    #[test]
    fn ocr_defaults_match_the_feature_matrix() {
        let o = OcrOptions::default();
        assert_eq!(o.languages, ["eng"]);
        assert_eq!(o.output, OcrOutput::Pdf);
        assert!(!o.deskew && o.skip_pages_with_text);
        assert_eq!(o.dpi.get(), 300);
        assert!(o.validate().is_ok());
        let low = OcrOptions {
            dpi: Dpi::new(50).unwrap(),
            ..OcrOptions::default()
        };
        assert!(low.validate().is_err());
    }

    #[test]
    fn manual_deskew_needs_valid_angles_for_pages_that_exist() {
        let manual = |angles: Vec<PageAngle>| DeskewOptions {
            mode: DeskewMode::Manual,
            angles,
            ..DeskewOptions::default()
        };
        let a = |page, degrees| PageAngle { page, degrees };
        assert!(manual(vec![a(1, 2.5), a(3, -1.0)]).validate(3).is_ok());
        assert!(manual(vec![]).validate(3).is_err());
        assert!(manual(vec![a(4, 1.0)]).validate(3).is_err());
        assert!(manual(vec![a(0, 1.0)]).validate(3).is_err());
        assert!(manual(vec![a(1, 120.0)]).validate(3).is_err());
        assert!(manual(vec![a(1, f32::NAN)]).validate(3).is_err());
        assert!(manual(vec![a(1, 1.0), a(1, 2.0)]).validate(3).is_err());
        assert!(DeskewOptions::default().validate(3).is_ok());
        let wide = DeskewOptions {
            max_angle_deg: 60.0,
            ..DeskewOptions::default()
        };
        assert!(wide.validate(3).is_err());
    }

    #[test]
    fn scan_defaults_match_the_feature_matrix_and_languages_matter_only_with_ocr() {
        let s = ScanToPdfOptions::default();
        assert!(s.auto_crop && s.perspective_fix && !s.ocr);
        assert_eq!(s.color, PictureColor::Color);
        assert!(s.validate().is_ok());
        let silent = ScanToPdfOptions {
            languages: vec![],
            ..ScanToPdfOptions::default()
        };
        assert!(silent.validate().is_ok());
        let loud = ScanToPdfOptions {
            ocr: true,
            ..silent
        };
        assert!(loud.validate().is_err());
    }
}
