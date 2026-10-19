//! Options of the conversion tools that need no outside program: Images to PDF (row 18), PDF to
//! images (row 23), Extract images (row 24), PDF to text (row 28) and TIFF to and from PDF
//! (row 68).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::fonts::StandardFont;
use crate::page_range::PageRange;
use crate::units::Dpi;

/// A4 in points.
const A4_POINTS: (f32, f32) = (595.276, 841.89);
/// US Letter in points.
const LETTER_POINTS: (f32, f32) = (612.0, 792.0);
/// Resolution assumed for the page of an image that is shown at its own size.
pub const FIT_DPI: f32 = 150.0;
/// PDF viewers handle pages up to 200 inches.
pub const MAX_PAGE_POINTS: f32 = 14_400.0;
/// Margin of the "small" setting, in points (10 mm).
pub const SMALL_MARGIN_POINTS: f32 = 28.346;
/// Margin of the "big" setting, in points (25 mm).
pub const BIG_MARGIN_POINTS: f32 = 70.866;

/// Turn of the page for the fixed paper sizes of Images to PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ImageOrientation {
    /// Landscape for a picture that is wider than tall, portrait otherwise (default).
    #[default]
    Auto,
    /// Always portrait.
    Portrait,
    /// Always landscape.
    Landscape,
}

/// Space between the picture and the page edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ImageMargin {
    /// No space (default).
    #[default]
    None,
    /// 10 millimeters.
    Small,
    /// 25 millimeters.
    Big,
}

impl ImageMargin {
    /// The space in points.
    pub const fn points(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Small => SMALL_MARGIN_POINTS,
            Self::Big => BIG_MARGIN_POINTS,
        }
    }
}

/// Page size of Images to PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ImagePageSize {
    /// The page has the proportions of the picture, at 150 dpi (default).
    #[default]
    Fit,
    /// ISO A4.
    A4,
    /// US Letter.
    Letter,
}

/// Options for Images to PDF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ImagesToPdfOptions {
    /// Page turn for A4 and Letter. Default auto.
    pub orientation: ImageOrientation,
    /// Space around the picture. Default none.
    pub margin: ImageMargin,
    /// Page size. Default fit.
    pub page_size: ImagePageSize,
    /// One PDF for all pictures (default) or one PDF per picture.
    pub one_pdf: bool,
}

impl Default for ImagesToPdfOptions {
    fn default() -> Self {
        Self {
            orientation: ImageOrientation::Auto,
            margin: ImageMargin::None,
            page_size: ImagePageSize::Fit,
            one_pdf: true,
        }
    }
}

/// Where a picture goes on its page, in points. `x` and `y` are from the lower left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Page width.
    pub page_w: f32,
    /// Page height.
    pub page_h: f32,
    /// Left edge of the picture.
    pub x: f32,
    /// Bottom edge of the picture.
    pub y: f32,
    /// Picture width.
    pub w: f32,
    /// Picture height.
    pub h: f32,
}

impl ImagesToPdfOptions {
    /// Page and picture size for a picture of the given size in pixels.
    ///
    /// `fit` makes the page as large as the picture at 150 dpi plus the margins, shrunk together
    /// when it would pass the PDF page limit. A4 and Letter scale the picture to the largest size
    /// that fits inside the margins, centered.
    pub fn place(&self, px_w: u32, px_h: u32) -> Result<Placement, DomainError> {
        if px_w == 0 || px_h == 0 {
            return Err(DomainError::option("image", "the picture has no pixels"));
        }
        let margin = self.margin.points();
        let (px_w, px_h) = (px_w as f32, px_h as f32);
        let base = match self.page_size {
            ImagePageSize::Fit => None,
            ImagePageSize::A4 => Some(A4_POINTS),
            ImagePageSize::Letter => Some(LETTER_POINTS),
        };
        let Some((short, long)) = base else {
            let k = 72.0 / FIT_DPI;
            let (mut w, mut h, mut m) = (px_w * k, px_h * k, margin);
            let largest = (w + 2.0 * m).max(h + 2.0 * m);
            if largest > MAX_PAGE_POINTS {
                let shrink = MAX_PAGE_POINTS / largest;
                (w, h, m) = (w * shrink, h * shrink, m * shrink);
            }
            return Ok(Placement {
                page_w: w + 2.0 * m,
                page_h: h + 2.0 * m,
                x: m,
                y: m,
                w,
                h,
            });
        };
        let landscape = match self.orientation {
            ImageOrientation::Auto => px_w > px_h,
            ImageOrientation::Landscape => true,
            ImageOrientation::Portrait => false,
        };
        let (page_w, page_h) = if landscape {
            (long, short)
        } else {
            (short, long)
        };
        let (area_w, area_h) = (page_w - 2.0 * margin, page_h - 2.0 * margin);
        let k = (area_w / px_w).min(area_h / px_h);
        let (w, h) = (px_w * k, px_h * k);
        Ok(Placement {
            page_w,
            page_h,
            x: (page_w - w) / 2.0,
            y: (page_h - h) / 2.0,
            w,
            h,
        })
    }
}

/// File format of the page pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PictureFormat {
    /// JPEG (default).
    #[default]
    Jpg,
    /// PNG.
    Png,
    /// TIFF.
    Tiff,
}

impl PictureFormat {
    /// File extension without the dot.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Jpg => "jpg",
            Self::Png => "png",
            Self::Tiff => "tif",
        }
    }
}

/// Colors of the page pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PictureColor {
    /// Full color (default).
    #[default]
    Color,
    /// Shades of gray.
    Gray,
    /// Black and white only.
    BlackWhite,
}

/// Options for PDF to images.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PdfToImagesOptions {
    /// File format. Default JPG.
    pub format: PictureFormat,
    /// Resolution in dots per inch, 10 to 2400. Default 150.
    pub dpi: Dpi,
    /// JPEG quality, 1 to 100. Default 90.
    pub jpeg_quality: u8,
    /// Colors. Default color.
    pub color: PictureColor,
    /// Pages to convert. Default `all`.
    pub pages: PageRange,
    /// For TIFF: one multi-page file instead of one file per page. Default off.
    pub single_tiff: bool,
}

impl Default for PdfToImagesOptions {
    fn default() -> Self {
        Self {
            format: PictureFormat::Jpg,
            dpi: Dpi::default(),
            jpeg_quality: 90,
            color: PictureColor::Color,
            pages: PageRange::all(),
            single_tiff: false,
        }
    }
}

impl PdfToImagesOptions {
    /// Checks the JPEG quality.
    pub fn validate(&self) -> Result<(), DomainError> {
        if !(1..=100).contains(&self.jpeg_quality) {
            return Err(DomainError::option(
                "jpeg_quality",
                "must be between 1 and 100",
            ));
        }
        Ok(())
    }
}

/// Options for Extract images.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ExtractImagesOptions {
    /// Keep the format the picture has in the PDF (default). Off writes every picture as PNG.
    pub keep_original_format: bool,
    /// Skip pictures narrower than this many pixels. Default 50.
    pub min_width: u32,
    /// Skip pictures lower than this many pixels. Default 50.
    pub min_height: u32,
}

impl Default for ExtractImagesOptions {
    fn default() -> Self {
        Self {
            keep_original_format: true,
            min_width: 50,
            min_height: 50,
        }
    }
}

/// How PDF to text lays the text out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TextLayout {
    /// The text in reading order, one line per line of the page (default).
    #[default]
    Plain,
    /// Keeps columns and gaps in place with spaces.
    Layout,
}

/// Character encoding of the text file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TextEncoding {
    /// UTF-8 (default).
    #[default]
    Utf8,
    /// UTF-8 with a byte order mark, which older Windows programs need to detect it.
    Utf8Bom,
    /// UTF-16 little endian with a byte order mark.
    Utf16,
    /// Windows-1252; characters it lacks become `?`.
    Windows1252,
}

/// Options for PDF to text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PdfToTextOptions {
    /// Plain or layout-preserving text. Default plain.
    pub layout: TextLayout,
    /// Put a line such as `--- Page 2 ---` between pages. Default on.
    pub page_separators: bool,
    /// Encoding of the file. Default UTF-8.
    pub encoding: TextEncoding,
    /// Pages to read. Default `all`.
    pub pages: PageRange,
}

impl Default for PdfToTextOptions {
    fn default() -> Self {
        Self {
            layout: TextLayout::Plain,
            page_separators: true,
            encoding: TextEncoding::Utf8,
            pages: PageRange::all(),
        }
    }
}

/// The separator line before a page.
pub fn page_separator(page_number: u32) -> String {
    format!("--- Page {page_number} ---")
}

impl TextEncoding {
    /// The bytes of `text` in this encoding.
    pub fn encode(self, text: &str) -> Vec<u8> {
        match self {
            Self::Utf8 => text.as_bytes().to_vec(),
            Self::Utf8Bom => {
                let mut out = vec![0xEF, 0xBB, 0xBF];
                out.extend_from_slice(text.as_bytes());
                out
            }
            Self::Utf16 => {
                let mut out = vec![0xFF, 0xFE];
                for unit in text.encode_utf16() {
                    out.extend_from_slice(&unit.to_le_bytes());
                }
                out
            }
            Self::Windows1252 => text
                .chars()
                .map(|c| match c {
                    '\n' | '\r' | '\t' => c as u8,
                    _ => StandardFont::encode_char(c).unwrap_or(b'?'),
                })
                .collect(),
        }
    }
}

/// Compression of the TIFF pages written by PDF to TIFF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TiffCompression {
    /// LZW (default).
    #[default]
    Lzw,
    /// Deflate (zip).
    Deflate,
    /// CCITT Group 4, for black and white pages. Other colors are turned black and white.
    CcittG4,
}

/// Options for TIFF to PDF. It has none: the pages follow the frames of the file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TiffToPdfOptions {}

/// Options for PDF to TIFF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PdfToTiffOptions {
    /// Compression. Default LZW.
    pub compression: TiffCompression,
    /// Resolution in dots per inch, 10 to 2400. Default 150.
    pub dpi: Dpi,
    /// Colors. CCITT G4 always writes black and white. Default color.
    pub color: PictureColor,
    /// Pages to convert. Default `all`.
    pub pages: PageRange,
}

impl Default for PdfToTiffOptions {
    fn default() -> Self {
        Self {
            compression: TiffCompression::Lzw,
            dpi: Dpi::default(),
            color: PictureColor::Color,
            pages: PageRange::all(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(
        size: ImagePageSize,
        orientation: ImageOrientation,
        margin: ImageMargin,
    ) -> ImagesToPdfOptions {
        ImagesToPdfOptions {
            orientation,
            margin,
            page_size: size,
            one_pdf: true,
        }
    }

    #[test]
    fn fit_makes_the_page_as_large_as_the_picture_at_150_dpi() {
        let p = ImagesToPdfOptions::default().place(300, 150).unwrap();
        assert!((p.page_w - 144.0).abs() < 0.01 && (p.page_h - 72.0).abs() < 0.01);
        assert_eq!((p.x, p.y), (0.0, 0.0));
    }

    #[test]
    fn fit_with_a_margin_grows_the_page_around_the_picture() {
        let options = fixed(
            ImagePageSize::Fit,
            ImageOrientation::Auto,
            ImageMargin::Small,
        );
        let p = options.place(300, 150).unwrap();
        assert!((p.page_w - (144.0 + 2.0 * SMALL_MARGIN_POINTS)).abs() < 0.01);
        assert!((p.x - SMALL_MARGIN_POINTS).abs() < 0.01);
    }

    #[test]
    fn a_huge_picture_is_shrunk_to_the_page_limit_with_its_margin() {
        let options = fixed(ImagePageSize::Fit, ImageOrientation::Auto, ImageMargin::Big);
        let p = options.place(100_000, 1000).unwrap();
        assert!(p.page_w <= MAX_PAGE_POINTS + 0.01);
        assert!((p.w / p.h - 100.0).abs() < 0.01);
    }

    #[test]
    fn a4_turns_with_the_picture_and_centers_it() {
        let options = fixed(ImagePageSize::A4, ImageOrientation::Auto, ImageMargin::None);
        let wide = options.place(2000, 1000).unwrap();
        assert!(wide.page_w > wide.page_h);
        assert!((wide.w - wide.page_w).abs() < 0.01);
        assert!((wide.y - (wide.page_h - wide.h) / 2.0).abs() < 0.01);
        let tall = options.place(1000, 2000).unwrap();
        assert!(tall.page_w < tall.page_h);
        assert!((tall.h - tall.page_h).abs() < 0.01);
    }

    #[test]
    fn a_forced_orientation_wins_over_the_picture_shape() {
        let options = fixed(
            ImagePageSize::Letter,
            ImageOrientation::Portrait,
            ImageMargin::None,
        );
        let p = options.place(2000, 1000).unwrap();
        assert_eq!((p.page_w, p.page_h), (612.0, 792.0));
        assert!((p.w - 612.0).abs() < 0.01);
    }

    #[test]
    fn margins_shrink_the_area_on_fixed_pages() {
        let options = fixed(
            ImagePageSize::A4,
            ImageOrientation::Portrait,
            ImageMargin::Big,
        );
        let p = options.place(1000, 1000).unwrap();
        assert!((p.w - (A4_POINTS.0 - 2.0 * BIG_MARGIN_POINTS)).abs() < 0.01);
        assert!((p.x - BIG_MARGIN_POINTS).abs() < 0.01);
    }

    #[test]
    fn an_empty_picture_is_rejected() {
        assert!(ImagesToPdfOptions::default().place(0, 10).is_err());
    }

    #[test]
    fn text_encodings_write_their_bytes() {
        assert_eq!(TextEncoding::Utf8.encode("é"), [0xC3, 0xA9]);
        assert_eq!(TextEncoding::Utf8Bom.encode("a"), [0xEF, 0xBB, 0xBF, b'a']);
        assert_eq!(TextEncoding::Utf16.encode("a"), [0xFF, 0xFE, b'a', 0]);
        assert_eq!(TextEncoding::Windows1252.encode("é€漢"), [0xE9, 0x80, b'?']);
    }

    #[test]
    fn defaults_match_the_feature_matrix() {
        let images = PdfToImagesOptions::default();
        assert_eq!(images.format, PictureFormat::Jpg);
        assert_eq!(images.dpi.get(), 150);
        assert!(images.validate().is_ok());
        let bad = PdfToImagesOptions {
            jpeg_quality: 0,
            ..images
        };
        assert!(bad.validate().is_err());
        let extract = ExtractImagesOptions::default();
        assert!(extract.keep_original_format);
        assert_eq!((extract.min_width, extract.min_height), (50, 50));
        assert_eq!(
            PdfToTiffOptions::default().compression,
            TiffCompression::Lzw
        );
        assert!(PdfToTextOptions::default().page_separators);
    }

    #[test]
    fn separator_names_the_page() {
        assert_eq!(page_separator(2), "--- Page 2 ---");
    }
}
