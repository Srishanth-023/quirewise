//! Options of the page layout tools: Crop (row 37), Resize (row 38), Flip (row 39), N-up
//! (row 40) and Grayscale (row 41), with the geometry each one needs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::common::{FractionBox, Region};
use super::document::FlattenDpi;
use crate::error::DomainError;
use crate::page_range::PageRange;

const MM_PER_INCH: f32 = 25.4;
const PT_PER_INCH: f32 = 72.0;

/// Points in a millimeter.
const PT_PER_MM: f32 = PT_PER_INCH / MM_PER_INCH;

/// Smallest part of a page a crop may leave: below this the page is practically gone.
const MIN_KEPT: f32 = 0.05;

fn check_length(name: &str, value: f32, max: f32) -> Result<(), DomainError> {
    if value.is_finite() && (0.0..=max).contains(&value) {
        Ok(())
    } else {
        Err(DomainError::option(
            name,
            format!("must be between 0 and {max}"),
        ))
    }
}

/// How Crop finds the part of the page to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CropMethod {
    /// Cut the same distances off every edge (default).
    #[default]
    Margins,
    /// Keep the box drawn on the page preview.
    Region,
    /// Cut away blank space around the content, page by page.
    Auto,
}

/// Unit of the margins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LengthUnit {
    /// Millimeters (default).
    #[default]
    Mm,
    /// Inches.
    In,
    /// Points, 1/72 inch.
    Pt,
}

impl LengthUnit {
    /// Converts a length in this unit to points.
    pub fn to_points(self, value: f32) -> f32 {
        match self {
            Self::Mm => value * PT_PER_MM,
            Self::In => value * PT_PER_INCH,
            Self::Pt => value,
        }
    }
}

/// Distances to cut off each edge.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Margins {
    /// Cut off the top.
    pub top: f32,
    /// Cut off the right.
    pub right: f32,
    /// Cut off the bottom.
    pub bottom: f32,
    /// Cut off the left.
    pub left: f32,
}

/// Options for Crop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CropOptions {
    /// How to choose what to keep. Default margins.
    pub method: CropMethod,
    /// Distances to cut off, for the margins method.
    pub margins: Margins,
    /// Unit of the margins. Default millimeters.
    pub unit: LengthUnit,
    /// The box to keep, for the region method.
    pub region: Option<Region>,
    /// Space left around the content, in millimeters, for the auto method. Default 2.
    pub auto_padding_mm: f32,
    /// Pages to crop: `all`, `even`, `odd` or a range. Default `all`.
    pub pages: PageRange,
    /// Also cut the media box, so other programs see the smaller page. Default off.
    pub apply_to_media_box: bool,
}

impl Default for CropOptions {
    fn default() -> Self {
        Self {
            method: CropMethod::Margins,
            margins: Margins::default(),
            unit: LengthUnit::Mm,
            region: None,
            auto_padding_mm: 2.0,
            pages: PageRange::all(),
            apply_to_media_box: false,
        }
    }
}

impl CropOptions {
    /// Checks the options for the chosen method.
    pub fn validate(&self) -> Result<(), DomainError> {
        match self.method {
            CropMethod::Margins => {
                let m = self.margins;
                for (name, v) in [
                    ("margins.top", m.top),
                    ("margins.right", m.right),
                    ("margins.bottom", m.bottom),
                    ("margins.left", m.left),
                ] {
                    check_length(name, v, 10_000.0)?;
                }
                if m.top + m.right + m.bottom + m.left <= 0.0 {
                    return Err(DomainError::option(
                        "margins",
                        "enter how much to cut off at least one edge",
                    ));
                }
                Ok(())
            }
            CropMethod::Region => match &self.region {
                Some(region) => region.validate(),
                None => Err(DomainError::option(
                    "region",
                    "draw the box to keep on the page preview",
                )),
            },
            CropMethod::Auto => check_length("auto_padding_mm", self.auto_padding_mm, 100.0),
        }
    }

    /// The part of a page of the given displayed size that the margins keep.
    pub fn margin_box(&self, width_pt: f32, height_pt: f32) -> Result<FractionBox, DomainError> {
        let to_pt = |v: f32| self.unit.to_points(v);
        let m = self.margins;
        let kept = FractionBox {
            left: to_pt(m.left) / width_pt,
            bottom: to_pt(m.bottom) / height_pt,
            right: 1.0 - to_pt(m.right) / width_pt,
            top: 1.0 - to_pt(m.top) / height_pt,
        };
        if kept.right - kept.left < MIN_KEPT || kept.top - kept.bottom < MIN_KEPT {
            return Err(DomainError::option(
                "margins",
                "these margins cut away the whole page; make them smaller",
            ));
        }
        Ok(kept)
    }
}

/// A region drawn from the top left as the part to keep, in the bottom-left form pages use.
pub fn region_box(region: &Region) -> FractionBox {
    FractionBox {
        left: region.x,
        bottom: 1.0 - (region.y + region.height),
        right: region.x + region.width,
        top: 1.0 - region.y,
    }
}

/// The box of the non-blank part of a rendered page, plus `padding_pt`, as fractions of the page
/// measured from the bottom left. `None` when the page is blank.
///
/// `rgba` is the rendered page (`width * height * 4` bytes); a pixel counts as content when any
/// color channel is darker than `threshold` and it is not transparent.
pub fn content_box(
    rgba: &[u8],
    width: u32,
    height: u32,
    threshold: u8,
    padding_pt: f32,
    page_width_pt: f32,
    page_height_pt: f32,
) -> Option<FractionBox> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return None;
    }
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for y in 0..h {
        let row = &rgba[y * w * 4..(y + 1) * w * 4];
        for (x, px) in row.as_chunks::<4>().0.iter().enumerate() {
            if px[3] > 8 && (px[0] < threshold || px[1] < threshold || px[2] < threshold) {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    if min_x == usize::MAX {
        return None;
    }
    let pad_x = padding_pt / page_width_pt;
    let pad_y = padding_pt / page_height_pt;
    let left = (min_x as f32 / w as f32 - pad_x).max(0.0);
    let right = ((max_x + 1) as f32 / w as f32 + pad_x).min(1.0);
    let top = (min_y as f32 / h as f32 - pad_y).max(0.0);
    let bottom = ((max_y + 1) as f32 / h as f32 + pad_y).min(1.0);
    Some(FractionBox {
        left,
        bottom: 1.0 - bottom,
        right,
        top: 1.0 - top,
    })
}

/// A paper size for Resize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PaperSize {
    /// ISO A4 (default).
    #[default]
    A4,
    /// US Letter.
    Letter,
    /// US Legal.
    Legal,
    /// ISO A3.
    A3,
    /// The width and height typed in the options.
    Custom,
}

/// Options for Resize.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ResizeOptions {
    /// Target paper size. Default A4.
    pub size: PaperSize,
    /// Width in millimeters, for a custom size. Default 210.
    pub custom_width_mm: f32,
    /// Height in millimeters, for a custom size. Default 297.
    pub custom_height_mm: f32,
    /// Empty space around the content, in millimeters. Default 0.
    pub margin_mm: f32,
    /// Scale the content to fit the new page (default). Off keeps its size, centered.
    pub scale_content: bool,
    /// Turn the target size to match each page's orientation (default), so a landscape page
    /// stays landscape.
    pub match_orientation: bool,
}

impl Default for ResizeOptions {
    fn default() -> Self {
        Self {
            size: PaperSize::A4,
            custom_width_mm: 210.0,
            custom_height_mm: 297.0,
            margin_mm: 0.0,
            scale_content: true,
            match_orientation: true,
        }
    }
}

impl ResizeOptions {
    /// The portrait target size in points.
    pub fn target_points(&self) -> Result<(f32, f32), DomainError> {
        let named = |w_mm: f32, h_mm: f32| (w_mm * PT_PER_MM, h_mm * PT_PER_MM);
        let (w, h) = match self.size {
            PaperSize::A4 => named(210.0, 297.0),
            PaperSize::Letter => (612.0, 792.0),
            PaperSize::Legal => (612.0, 1008.0),
            PaperSize::A3 => named(297.0, 420.0),
            PaperSize::Custom => {
                for (name, v) in [
                    ("custom_width_mm", self.custom_width_mm),
                    ("custom_height_mm", self.custom_height_mm),
                ] {
                    if !v.is_finite() || !(20.0..=5000.0).contains(&v) {
                        return Err(DomainError::option(
                            name,
                            "must be between 20 and 5000 millimeters",
                        ));
                    }
                }
                named(self.custom_width_mm, self.custom_height_mm)
            }
        };
        Ok((w, h))
    }

    /// The margin in points.
    pub fn margin_pt(&self) -> f32 {
        self.margin_mm * PT_PER_MM
    }

    /// Checks the options.
    pub fn validate(&self) -> Result<(), DomainError> {
        let (w, h) = self.target_points()?;
        check_length("margin_mm", self.margin_mm, 1000.0)?;
        if self.margin_pt() * 2.0 > w.min(h) * 0.9 {
            return Err(DomainError::option(
                "margin_mm",
                "the margin leaves no room for the content",
            ));
        }
        Ok(())
    }

    /// The size of the page that replaces one of the given displayed size.
    pub fn page_size_for(&self, page_w: f32, page_h: f32) -> Result<(f32, f32), DomainError> {
        let (w, h) = self.target_points()?;
        let landscape_page = page_w > page_h;
        Ok(if self.match_orientation && landscape_page {
            (h, w)
        } else {
            (w, h)
        })
    }
}

/// Which way Flip mirrors the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FlipDirection {
    /// Left and right swap, like a mirror (default).
    #[default]
    Horizontal,
    /// Top and bottom swap.
    Vertical,
}

/// Options for Flip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct FlipOptions {
    /// Direction. Default horizontal.
    pub direction: FlipDirection,
    /// Pages to flip: `all`, `even`, `odd` or a range. Default `all`.
    pub pages: PageRange,
}

impl Default for FlipOptions {
    fn default() -> Self {
        Self {
            direction: FlipDirection::Horizontal,
            pages: PageRange::all(),
        }
    }
}

/// Pages per sheet for N-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub enum SheetCount {
    /// 2 pages per sheet (default).
    #[default]
    #[serde(rename = "2")]
    Two,
    /// 4 pages per sheet.
    #[serde(rename = "4")]
    Four,
    /// 8 pages per sheet.
    #[serde(rename = "8")]
    Eight,
    /// 16 pages per sheet.
    #[serde(rename = "16")]
    Sixteen,
    /// 32 pages per sheet.
    #[serde(rename = "32")]
    ThirtyTwo,
}

impl SheetCount {
    /// How many pages fit on a sheet.
    pub const fn count(self) -> u32 {
        match self {
            Self::Two => 2,
            Self::Four => 4,
            Self::Eight => 8,
            Self::Sixteen => 16,
            Self::ThirtyTwo => 32,
        }
    }

    /// Columns and rows of the grid.
    pub const fn grid(self) -> (u32, u32) {
        match self {
            Self::Two => (2, 1),
            Self::Four => (2, 2),
            Self::Eight => (4, 2),
            Self::Sixteen => (4, 4),
            Self::ThirtyTwo => (8, 4),
        }
    }
}

/// The order pages fill a sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FillOrder {
    /// Across each row, then down (default).
    #[default]
    Horizontal,
    /// Down each column, then across.
    Vertical,
}

/// A rectangle in points, measured from the bottom left of the sheet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    /// Left edge.
    pub x: f32,
    /// Bottom edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

/// Options for N-up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct NUpOptions {
    /// Pages per sheet. Default 2.
    pub per_sheet: SheetCount,
    /// The order pages fill a sheet. Default horizontal.
    pub order: FillOrder,
    /// Keep the sheet the size of the first page, shrinking the pages (default). Off makes the
    /// sheet as large as the grid of full-size pages.
    pub preserve_page_size: bool,
    /// Fill each row from the right, for right-to-left languages. Default off.
    pub right_to_left: bool,
}

impl Default for NUpOptions {
    fn default() -> Self {
        Self {
            per_sheet: SheetCount::Two,
            order: FillOrder::Horizontal,
            preserve_page_size: true,
            right_to_left: false,
        }
    }
}

impl NUpOptions {
    /// Number of sheets for a document of `pages` pages.
    pub fn sheets(&self, pages: u32) -> u32 {
        pages.div_ceil(self.per_sheet.count())
    }

    /// The size of every sheet, from the size of the first page.
    pub fn sheet_size(&self, first_w: f32, first_h: f32) -> (f32, f32) {
        let (cols, rows) = self.per_sheet.grid();
        if self.preserve_page_size {
            // Turn the sheet so the grid's long side lies along the paper's long side.
            let grid_wide = cols > rows;
            let page_wide = first_w > first_h;
            if cols != rows && grid_wide != page_wide {
                (first_h, first_w)
            } else {
                (first_w, first_h)
            }
        } else {
            (first_w * cols as f32, first_h * rows as f32)
        }
    }

    /// The cell of each slot in page order, for a sheet of the given size.
    pub fn cells(&self, sheet_w: f32, sheet_h: f32) -> Vec<Cell> {
        let (cols, rows) = self.per_sheet.grid();
        let (cw, ch) = (sheet_w / cols as f32, sheet_h / rows as f32);
        (0..self.per_sheet.count())
            .map(|slot| {
                let (col, row) = match self.order {
                    FillOrder::Horizontal => (slot % cols, slot / cols),
                    FillOrder::Vertical => (slot / rows, slot % rows),
                };
                let col = if self.right_to_left {
                    cols - 1 - col
                } else {
                    col
                };
                Cell {
                    x: col as f32 * cw,
                    // Row 0 is the top row.
                    y: (rows - 1 - row) as f32 * ch,
                    width: cw,
                    height: ch,
                }
            })
            .collect()
    }
}

/// What Grayscale turns gray.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GrayscaleScope {
    /// Pictures and colored text and lines (default).
    #[default]
    Both,
    /// Pictures only.
    Images,
    /// Colored text and lines only.
    Text,
}

/// Options for Grayscale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct GrayscaleOptions {
    /// What to turn gray. Default pictures and text.
    pub apply_to: GrayscaleScope,
    /// Render every page as a gray picture instead. Catches every color, but text can no longer
    /// be selected. Default off.
    pub raster_fallback: bool,
    /// Resolution of the picture when `raster_fallback` is on. Default 150.
    pub dpi: FlattenDpi,
}

impl Default for GrayscaleOptions {
    fn default() -> Self {
        Self {
            apply_to: GrayscaleScope::Both,
            raster_fallback: false,
            dpi: FlattenDpi::Dpi150,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn crop_with(f: impl FnOnce(&mut CropOptions)) -> CropOptions {
        let mut o = CropOptions::default();
        f(&mut o);
        o
    }

    #[test]
    fn margins_need_at_least_one_edge_and_stay_on_the_page() {
        assert_eq!(
            CropOptions::default().validate().unwrap_err().code(),
            ErrorCode::OptionInvalid
        );
        let o = crop_with(|o| o.margins.left = 10.0);
        assert!(o.validate().is_ok());
        let negative = crop_with(|o| o.margins.top = -1.0);
        assert!(negative.validate().is_err());
        let nan = crop_with(|o| o.margins.top = f32::NAN);
        assert!(nan.validate().is_err());
    }

    #[test]
    fn margins_become_fractions_of_the_displayed_page() {
        let o = crop_with(|o| {
            o.unit = LengthUnit::Pt;
            o.margins = Margins {
                top: 79.2,
                right: 61.2,
                bottom: 39.6,
                left: 30.6,
            };
        });
        let b = o.margin_box(612.0, 792.0).unwrap();
        assert!((b.left - 0.05).abs() < 1e-5);
        assert!((b.right - 0.9).abs() < 1e-5);
        assert!((b.bottom - 0.05).abs() < 1e-5);
        assert!((b.top - 0.9).abs() < 1e-5);
        let inches = crop_with(|o| {
            o.unit = LengthUnit::In;
            o.margins.left = 1.0;
        });
        assert!((inches.margin_box(720.0, 720.0).unwrap().left - 0.1).abs() < 1e-5);
        let mm = crop_with(|o| o.margins.left = 25.4);
        assert!((mm.margin_box(720.0, 720.0).unwrap().left - 0.1).abs() < 1e-5);
    }

    #[test]
    fn margins_that_remove_the_page_are_rejected() {
        let o = crop_with(|o| {
            o.unit = LengthUnit::Pt;
            o.margins.left = 400.0;
            o.margins.right = 250.0;
        });
        assert_eq!(
            o.margin_box(612.0, 792.0).unwrap_err().code(),
            ErrorCode::OptionInvalid
        );
    }

    #[test]
    fn a_drawn_region_becomes_a_bottom_left_box() {
        let b = region_box(&Region {
            x: 0.1,
            y: 0.2,
            width: 0.5,
            height: 0.3,
        });
        assert!((b.left - 0.1).abs() < 1e-6);
        assert!((b.right - 0.6).abs() < 1e-6);
        assert!((b.top - 0.8).abs() < 1e-6);
        assert!((b.bottom - 0.5).abs() < 1e-6);
        let none = crop_with(|o| o.method = CropMethod::Region);
        assert!(none.validate().is_err());
    }

    fn page_with_dark_block(
        w: usize,
        h: usize,
        x0: usize,
        y0: usize,
        x1: usize,
        y1: usize,
    ) -> Vec<u8> {
        let mut px = vec![255u8; w * h * 4];
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * w + x) * 4;
                px[i..i + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
        px
    }

    #[test]
    fn the_content_box_hugs_the_dark_pixels_and_adds_padding() {
        let px = page_with_dark_block(100, 200, 20, 40, 60, 120);
        let b = content_box(&px, 100, 200, 240, 0.0, 100.0, 200.0).unwrap();
        assert!((b.left - 0.2).abs() < 1e-5);
        assert!((b.right - 0.6).abs() < 1e-5);
        // Rows 40 to 119 from the top: top edge at 40/200, bottom edge at 120/200.
        assert!((b.top - 0.8).abs() < 1e-5);
        assert!((b.bottom - 0.4).abs() < 1e-5);
        let padded = content_box(&px, 100, 200, 240, 10.0, 100.0, 200.0).unwrap();
        assert!((padded.left - 0.1).abs() < 1e-5);
        assert!((padded.top - 0.85).abs() < 1e-5);
    }

    #[test]
    fn a_blank_or_malformed_page_has_no_content_box() {
        let blank = vec![255u8; 10 * 10 * 4];
        assert!(content_box(&blank, 10, 10, 240, 0.0, 10.0, 10.0).is_none());
        assert!(content_box(&blank[..8], 10, 10, 240, 0.0, 10.0, 10.0).is_none());
        assert!(content_box(&[], 0, 0, 240, 0.0, 10.0, 10.0).is_none());
        // Fully transparent pixels are not content even when their color is black.
        let clear = vec![0u8; 4 * 4 * 4];
        assert!(content_box(&clear, 4, 4, 240, 0.0, 4.0, 4.0).is_none());
    }

    #[test]
    fn paper_sizes_and_orientation() {
        let a4 = ResizeOptions::default().target_points().unwrap();
        assert!((a4.0 - 595.276).abs() < 0.01 && (a4.1 - 841.89).abs() < 0.01);
        let letter = ResizeOptions {
            size: PaperSize::Letter,
            ..ResizeOptions::default()
        };
        assert_eq!(letter.target_points().unwrap(), (612.0, 792.0));
        assert_eq!(letter.page_size_for(800.0, 600.0).unwrap(), (792.0, 612.0));
        assert_eq!(letter.page_size_for(600.0, 800.0).unwrap(), (612.0, 792.0));
        let fixed = ResizeOptions {
            size: PaperSize::Letter,
            match_orientation: false,
            ..ResizeOptions::default()
        };
        assert_eq!(fixed.page_size_for(800.0, 600.0).unwrap(), (612.0, 792.0));
    }

    #[test]
    fn custom_sizes_and_margins_are_bounded() {
        let custom = ResizeOptions {
            size: PaperSize::Custom,
            custom_width_mm: 100.0,
            custom_height_mm: 150.0,
            ..ResizeOptions::default()
        };
        let (w, h) = custom.target_points().unwrap();
        assert!((w - 283.46).abs() < 0.01 && (h - 425.2).abs() < 0.01);
        for bad in [
            ResizeOptions {
                size: PaperSize::Custom,
                custom_width_mm: 5.0,
                ..ResizeOptions::default()
            },
            ResizeOptions {
                size: PaperSize::Custom,
                custom_height_mm: f32::NAN,
                ..ResizeOptions::default()
            },
            ResizeOptions {
                margin_mm: -1.0,
                ..ResizeOptions::default()
            },
            ResizeOptions {
                margin_mm: 100.0,
                ..ResizeOptions::default()
            },
        ] {
            assert_eq!(
                bad.validate().unwrap_err().code(),
                ErrorCode::OptionInvalid,
                "{bad:?}"
            );
        }
        assert!(ResizeOptions::default().validate().is_ok());
    }

    #[test]
    fn n_up_grids_and_sheet_counts() {
        for (n, grid) in [
            (SheetCount::Two, (2, 1)),
            (SheetCount::Four, (2, 2)),
            (SheetCount::Eight, (4, 2)),
            (SheetCount::Sixteen, (4, 4)),
            (SheetCount::ThirtyTwo, (8, 4)),
        ] {
            assert_eq!(n.grid().0 * n.grid().1, n.count());
            assert_eq!(n.grid(), grid);
        }
        let o = NUpOptions {
            per_sheet: SheetCount::Four,
            ..NUpOptions::default()
        };
        assert_eq!(o.sheets(9), 3);
        assert_eq!(o.sheets(8), 2);
        assert_eq!(o.sheets(0), 0);
    }

    #[test]
    fn two_up_turns_a_portrait_sheet_to_landscape_unless_page_size_grows() {
        let o = NUpOptions::default();
        assert_eq!(o.sheet_size(600.0, 800.0), (800.0, 600.0));
        assert_eq!(o.sheet_size(800.0, 600.0), (800.0, 600.0));
        let four = NUpOptions {
            per_sheet: SheetCount::Four,
            ..NUpOptions::default()
        };
        assert_eq!(four.sheet_size(600.0, 800.0), (600.0, 800.0));
        let grow = NUpOptions {
            preserve_page_size: false,
            ..NUpOptions::default()
        };
        assert_eq!(grow.sheet_size(600.0, 800.0), (1200.0, 800.0));
    }

    #[test]
    fn cells_follow_the_fill_order() {
        let four = NUpOptions {
            per_sheet: SheetCount::Four,
            ..NUpOptions::default()
        };
        let h = four.cells(200.0, 100.0);
        let at = |c: &Cell| (c.x, c.y);
        // Across then down: top left, top right, bottom left, bottom right.
        assert_eq!(
            h.iter().map(at).collect::<Vec<_>>(),
            vec![(0.0, 50.0), (100.0, 50.0), (0.0, 0.0), (100.0, 0.0)]
        );
        let v = NUpOptions {
            order: FillOrder::Vertical,
            ..four.clone()
        }
        .cells(200.0, 100.0);
        assert_eq!(
            v.iter().map(at).collect::<Vec<_>>(),
            vec![(0.0, 50.0), (0.0, 0.0), (100.0, 50.0), (100.0, 0.0)]
        );
        let rtl = NUpOptions {
            right_to_left: true,
            ..four
        }
        .cells(200.0, 100.0);
        assert_eq!(
            rtl.iter().map(at).collect::<Vec<_>>(),
            vec![(100.0, 50.0), (0.0, 50.0), (100.0, 0.0), (0.0, 0.0)]
        );
        assert_eq!(rtl[0].width, 100.0);
        assert_eq!(rtl[0].height, 50.0);
    }

    #[test]
    fn json_names_match_the_documentation() {
        let n: NUpOptions =
            serde_json::from_str(r#"{"per_sheet":"8","order":"vertical"}"#).unwrap();
        assert_eq!(n.per_sheet, SheetCount::Eight);
        assert_eq!(n.order, FillOrder::Vertical);
        assert!(serde_json::from_str::<NUpOptions>(r#"{"per_sheet":"3"}"#).is_err());
        let g: GrayscaleOptions =
            serde_json::from_str(r#"{"apply_to":"images","dpi":"100"}"#).unwrap();
        assert_eq!(g.apply_to, GrayscaleScope::Images);
        assert_eq!(g.dpi, FlattenDpi::Dpi100);
        let f: FlipOptions = serde_json::from_str(r#"{"direction":"vertical"}"#).unwrap();
        assert_eq!(f.direction, FlipDirection::Vertical);
    }
}
