//! Options of the stamping tools: Page numbers (row 33), Header and footer (row 34), Bates
//! numbering (row 35) and Watermark (row 36). Each turns its options into the stamps to draw.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::PageIndex;
use crate::error::DomainError;
use crate::fonts::{FontFamily, StandardFont};
use crate::page_range::{Duplicates, PageRange};
use crate::stamp::{
    EDGE_MARGIN_PT, Layer, NumberStyle, PageStamps, Position, Rgb, Stamp, StampContent, TextSpec,
    TokenValues, expand_template, validate_template,
};

const MAX_SIZE_PT: f32 = 400.0;
const MAX_OFFSET_PT: f32 = 2000.0;

fn default_size() -> f32 {
    10.0
}

fn default_color() -> String {
    "#000000".to_owned()
}

/// Checks a size and builds the font, color and size shared by the stamping tools.
fn text_format(
    family: FontFamily,
    bold: bool,
    italic: bool,
    size_pt: f32,
    color: &str,
) -> Result<(StandardFont, f32, Rgb), DomainError> {
    if !size_pt.is_finite() || !(1.0..=MAX_SIZE_PT).contains(&size_pt) {
        return Err(DomainError::option(
            "size_pt",
            format!("must be between 1 and {MAX_SIZE_PT} points"),
        ));
    }
    Ok((
        StandardFont::new(family, bold, italic),
        size_pt,
        Rgb::parse(color)?,
    ))
}

fn check_offset(name: &str, value: f32) -> Result<(), DomainError> {
    if value.is_finite() && value.abs() <= MAX_OFFSET_PT {
        Ok(())
    } else {
        Err(DomainError::option(
            name,
            format!("must be between -{MAX_OFFSET_PT} and {MAX_OFFSET_PT} points"),
        ))
    }
}

/// Fails when the font cannot draw some character of `text`.
fn check_drawable(field: &str, font: StandardFont, text: &str) -> Result<(), DomainError> {
    match StandardFont::encode(text) {
        Ok(_) => Ok(()),
        Err(c) => Err(DomainError::option(
            field,
            format!(
                "the character \"{c}\" cannot be drawn with the {} font; use letters, digits and common punctuation",
                font.pdf_name()
            ),
        )),
    }
}

/// A text stamp content.
fn plain_text(text: String, font: StandardFont, size_pt: f32, color: Rgb) -> StampContent {
    StampContent::Text(TextSpec {
        text,
        font,
        size_pt,
        color,
    })
}

/// Options for Page numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PageNumbersOptions {
    /// Pages to number: `all`, `even`, `odd` or a range such as `2-`. Default `all`.
    pub pages: PageRange,
    /// The number shown on the first numbered page. Default 1.
    pub start_number: u32,
    /// Leave the first page without a number and do not count it. Default off.
    pub skip_first_page: bool,
    /// Mirror the position on left-hand (even) pages, for two-sided printing. Default off.
    pub facing_pages: bool,
    /// Place on the page. Default bottom center.
    pub position: Position,
    /// Move right by this many points. Default 0.
    pub offset_x_pt: f32,
    /// Move up by this many points. Default 0.
    pub offset_y_pt: f32,
    /// Font family. Default Helvetica.
    pub font: FontFamily,
    /// Bold. Default off.
    pub bold: bool,
    /// Italic. Default off.
    pub italic: bool,
    /// Font size in points. Default 10.
    pub size_pt: f32,
    /// Text color as `#RRGGBB`. Default black.
    pub color: String,
    /// Text with `{page}`, `{pages}`, `{date}` and `{filename}`. Default `{page}`.
    pub template: String,
    /// How the number is written. Default arabic.
    pub style: NumberStyle,
}

impl Default for PageNumbersOptions {
    fn default() -> Self {
        Self {
            pages: PageRange::all(),
            start_number: 1,
            skip_first_page: false,
            facing_pages: false,
            position: Position::BottomCenter,
            offset_x_pt: 0.0,
            offset_y_pt: 0.0,
            font: FontFamily::Helvetica,
            bold: false,
            italic: false,
            size_pt: default_size(),
            color: default_color(),
            template: "{page}".to_owned(),
            style: NumberStyle::Arabic,
        }
    }
}

/// Details of one file the stamps are made for.
#[derive(Debug, Clone, Copy)]
pub struct FileFacts<'a> {
    /// Number of pages.
    pub page_count: u32,
    /// Name without the extension.
    pub filename: &'a str,
    /// Today as `2026-10-19`.
    pub date: &'a str,
}

impl PageNumbersOptions {
    /// Checks everything that does not need the document.
    pub fn validate(&self) -> Result<(), DomainError> {
        let (font, ..) = text_format(self.font, self.bold, self.italic, self.size_pt, &self.color)?;
        validate_template(&self.template)?;
        if self.start_number > 1_000_000 {
            return Err(DomainError::option(
                "start_number",
                "must be at most 1000000",
            ));
        }
        check_offset("offset_x_pt", self.offset_x_pt)?;
        check_offset("offset_y_pt", self.offset_y_pt)?;
        check_drawable("template", font, &self.template.replace(['{', '}'], ""))
    }

    /// The numbers to draw: one text per numbered page.
    pub fn plan(&self, file: &FileFacts<'_>) -> Result<Vec<PageStamps>, DomainError> {
        self.validate()?;
        let (font, size, color) =
            text_format(self.font, self.bold, self.italic, self.size_pt, &self.color)?;
        let selection = self.pages.resolve(file.page_count, Duplicates::Reject)?;
        let mut pages: Vec<PageIndex> = selection.pages().to_vec();
        pages.sort_by_key(|p| p.get());
        if self.skip_first_page {
            pages.retain(|p| p.get() != 0);
        }
        let count = u32::try_from(pages.len()).unwrap_or(u32::MAX);
        let last = self.start_number.saturating_add(count.saturating_sub(1));
        let pages_text = self.style.format(last);
        let mut out = Vec::with_capacity(pages.len());
        for (k, page) in pages.into_iter().enumerate() {
            let number = self
                .start_number
                .saturating_add(u32::try_from(k).unwrap_or(u32::MAX));
            let page_text = self.style.format(number);
            let text = expand_template(
                &self.template,
                &TokenValues {
                    page: &page_text,
                    pages: &pages_text,
                    date: file.date,
                    filename: file.filename,
                },
            );
            check_drawable("template", font, &text)?;
            // Page 1 is a right-hand page, so the even pages are the ones that mirror.
            let mirror = self.facing_pages && page.get() % 2 == 1;
            out.push(PageStamps {
                page,
                stamps: vec![Stamp {
                    content: plain_text(text, font, size, color),
                    position: if mirror {
                        self.position.mirrored()
                    } else {
                        self.position
                    },
                    offset_x_pt: if mirror {
                        -self.offset_x_pt
                    } else {
                        self.offset_x_pt
                    },
                    offset_y_pt: self.offset_y_pt,
                    rotation_deg: 0.0,
                    opacity: 1.0,
                    layer: Layer::Above,
                    tile: false,
                    margin_pt: EDGE_MARGIN_PT,
                }],
            });
        }
        Ok(out)
    }
}

/// Options for Header and footer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct HeaderFooterOptions {
    /// Pages to mark. Default `all`.
    pub pages: PageRange,
    /// Header text at the left. Tokens: `{page}`, `{pages}`, `{date}`, `{filename}`.
    pub header_left: String,
    /// Header text in the center.
    pub header_center: String,
    /// Header text at the right.
    pub header_right: String,
    /// Footer text at the left.
    pub footer_left: String,
    /// Footer text in the center.
    pub footer_center: String,
    /// Footer text at the right.
    pub footer_right: String,
    /// Font family. Default Helvetica.
    pub font: FontFamily,
    /// Bold. Default off.
    pub bold: bool,
    /// Italic. Default off.
    pub italic: bool,
    /// Font size in points. Default 10.
    pub size_pt: f32,
    /// Text color as `#RRGGBB`. Default black.
    pub color: String,
    /// Distance from the page edge in points. Default 36.
    pub margin_pt: f32,
}

impl Default for HeaderFooterOptions {
    fn default() -> Self {
        Self {
            pages: PageRange::all(),
            header_left: String::new(),
            header_center: String::new(),
            header_right: String::new(),
            footer_left: String::new(),
            footer_center: String::new(),
            footer_right: String::new(),
            font: FontFamily::Helvetica,
            bold: false,
            italic: false,
            size_pt: default_size(),
            color: default_color(),
            margin_pt: EDGE_MARGIN_PT,
        }
    }
}

impl HeaderFooterOptions {
    fn slots(&self) -> [(&str, Position, &str); 6] {
        [
            ("header_left", Position::TopLeft, &self.header_left),
            ("header_center", Position::TopCenter, &self.header_center),
            ("header_right", Position::TopRight, &self.header_right),
            ("footer_left", Position::BottomLeft, &self.footer_left),
            ("footer_center", Position::BottomCenter, &self.footer_center),
            ("footer_right", Position::BottomRight, &self.footer_right),
        ]
    }

    /// Checks everything that does not need the document.
    pub fn validate(&self) -> Result<(), DomainError> {
        let (font, ..) = text_format(self.font, self.bold, self.italic, self.size_pt, &self.color)?;
        if !self.margin_pt.is_finite() || !(0.0..=400.0).contains(&self.margin_pt) {
            return Err(DomainError::option(
                "margin_pt",
                "must be between 0 and 400 points",
            ));
        }
        let mut any = false;
        for (name, _, text) in self.slots() {
            validate_template(text).map_err(|e| match e {
                DomainError::OptionInvalid { reason, .. } => DomainError::option(name, reason),
                other => other,
            })?;
            check_drawable(name, font, &text.replace(['{', '}'], ""))?;
            any |= !text.trim().is_empty();
        }
        if any {
            Ok(())
        } else {
            Err(DomainError::option(
                "header_left",
                "type some header or footer text",
            ))
        }
    }

    /// The texts to draw on each selected page.
    pub fn plan(&self, file: &FileFacts<'_>) -> Result<Vec<PageStamps>, DomainError> {
        self.validate()?;
        let (font, size, color) =
            text_format(self.font, self.bold, self.italic, self.size_pt, &self.color)?;
        let selection = self.pages.resolve(file.page_count, Duplicates::Reject)?;
        let mut pages: Vec<PageIndex> = selection.pages().to_vec();
        pages.sort_by_key(|p| p.get());
        let total = file.page_count.to_string();
        let mut out = Vec::with_capacity(pages.len());
        for page in pages {
            let number = (page.get() + 1).to_string();
            let mut stamps = Vec::new();
            for (name, position, template) in self.slots() {
                if template.trim().is_empty() {
                    continue;
                }
                let text = expand_template(
                    template,
                    &TokenValues {
                        page: &number,
                        pages: &total,
                        date: file.date,
                        filename: file.filename,
                    },
                );
                check_drawable(name, font, &text)?;
                stamps.push(Stamp {
                    content: plain_text(text, font, size, color),
                    position,
                    offset_x_pt: 0.0,
                    offset_y_pt: 0.0,
                    rotation_deg: 0.0,
                    opacity: 1.0,
                    layer: Layer::Above,
                    tile: false,
                    margin_pt: self.margin_pt,
                });
            }
            out.push(PageStamps { page, stamps });
        }
        Ok(out)
    }
}

/// Options for Bates numbering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct BatesOptions {
    /// Text before the number, such as `ACME-`. Default none.
    pub prefix: String,
    /// Text after the number. Default none.
    pub suffix: String,
    /// Digits the number is padded to with zeros, 1 to 12. Default 6.
    pub digits: u8,
    /// The first number. Default 1.
    pub start: u64,
    /// Keep counting from one file to the next, in the order of the file list. Default on.
    pub continue_across_files: bool,
    /// Place on the page. Default bottom right.
    pub position: Position,
    /// Move right by this many points. Default 0.
    pub offset_x_pt: f32,
    /// Move up by this many points. Default 0.
    pub offset_y_pt: f32,
    /// Font family. Default Helvetica.
    pub font: FontFamily,
    /// Bold. Default off.
    pub bold: bool,
    /// Italic. Default off.
    pub italic: bool,
    /// Font size in points. Default 10.
    pub size_pt: f32,
    /// Text color as `#RRGGBB`. Default black.
    pub color: String,
}

impl Default for BatesOptions {
    fn default() -> Self {
        Self {
            prefix: String::new(),
            suffix: String::new(),
            digits: 6,
            start: 1,
            continue_across_files: true,
            position: Position::BottomRight,
            offset_x_pt: 0.0,
            offset_y_pt: 0.0,
            font: FontFamily::Helvetica,
            bold: false,
            italic: false,
            size_pt: default_size(),
            color: default_color(),
        }
    }
}

impl BatesOptions {
    /// Checks the options.
    pub fn validate(&self) -> Result<(), DomainError> {
        let (font, ..) = text_format(self.font, self.bold, self.italic, self.size_pt, &self.color)?;
        if !(1..=12).contains(&self.digits) {
            return Err(DomainError::option("digits", "must be between 1 and 12"));
        }
        if self.start > 999_999_999_999 {
            return Err(DomainError::option("start", "must be at most 999999999999"));
        }
        check_offset("offset_x_pt", self.offset_x_pt)?;
        check_offset("offset_y_pt", self.offset_y_pt)?;
        check_drawable("prefix", font, &self.prefix)?;
        check_drawable("suffix", font, &self.suffix)
    }

    /// The label of one number, such as `ACME-000042`.
    pub fn label(&self, number: u64) -> String {
        format!(
            "{}{number:0width$}{}",
            self.prefix,
            self.suffix,
            width = usize::from(self.digits)
        )
    }

    /// Where counting starts for each file, given the page counts in file order.
    pub fn starts(&self, page_counts: &[u32]) -> Vec<u64> {
        let mut next = self.start;
        page_counts
            .iter()
            .map(|count| {
                let start = if self.continue_across_files {
                    next
                } else {
                    self.start
                };
                next = next.saturating_add(u64::from(*count));
                start
            })
            .collect()
    }

    /// One number on every page of a file whose first page gets `first`.
    pub fn plan(&self, page_count: u32, first: u64) -> Result<Vec<PageStamps>, DomainError> {
        self.validate()?;
        let (font, size, color) =
            text_format(self.font, self.bold, self.italic, self.size_pt, &self.color)?;
        Ok((0..page_count)
            .map(|i| PageStamps {
                page: PageIndex::new(i),
                stamps: vec![Stamp {
                    content: plain_text(
                        self.label(first.saturating_add(u64::from(i))),
                        font,
                        size,
                        color,
                    ),
                    position: self.position,
                    offset_x_pt: self.offset_x_pt,
                    offset_y_pt: self.offset_y_pt,
                    rotation_deg: 0.0,
                    opacity: 1.0,
                    layer: Layer::Above,
                    tile: false,
                    margin_pt: EDGE_MARGIN_PT,
                }],
            })
            .collect())
    }
}

/// What a watermark shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum WatermarkKind {
    /// A line of text (default).
    #[default]
    Text,
    /// A PNG or JPEG picture.
    Image,
}

/// Options for Watermark.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct WatermarkOptions {
    /// Text or picture. Default text.
    pub kind: WatermarkKind,
    /// The text, for a text watermark.
    pub text: String,
    /// Path of a PNG or JPEG file, for a picture watermark.
    pub image_path: Option<PathBuf>,
    /// Width of the picture as a percent of the page width, 1 to 100. Default 50.
    pub image_width_percent: u8,
    /// Pages to mark. Default `all`.
    pub pages: PageRange,
    /// Place on the page. Default center.
    pub position: Position,
    /// Move right by this many points. Default 0.
    pub offset_x_pt: f32,
    /// Move up by this many points. Default 0.
    pub offset_y_pt: f32,
    /// Repeat the mark across the whole page. Default off.
    pub tile: bool,
    /// Counter-clockwise rotation in degrees, 0 to 360. Default 0.
    pub rotation_deg: f32,
    /// How see-through the mark is, 1 to 100. Default 50.
    pub transparency: u8,
    /// Font family. Default Helvetica.
    pub font: FontFamily,
    /// Bold. Default off.
    pub bold: bool,
    /// Italic. Default off.
    pub italic: bool,
    /// Font size in points. Default 48.
    pub size_pt: f32,
    /// Text color as `#RRGGBB`. Default gray.
    pub color: String,
    /// Over or under the page content. Default above.
    pub layer: Layer,
}

impl Default for WatermarkOptions {
    fn default() -> Self {
        Self {
            kind: WatermarkKind::Text,
            text: "CONFIDENTIAL".to_owned(),
            image_path: None,
            image_width_percent: 50,
            pages: PageRange::all(),
            position: Position::Center,
            offset_x_pt: 0.0,
            offset_y_pt: 0.0,
            tile: false,
            rotation_deg: 0.0,
            transparency: 50,
            font: FontFamily::Helvetica,
            bold: false,
            italic: false,
            size_pt: 48.0,
            color: "#808080".to_owned(),
            layer: Layer::Above,
        }
    }
}

impl WatermarkOptions {
    /// Checks the options.
    pub fn validate(&self) -> Result<(), DomainError> {
        let (font, ..) = text_format(self.font, self.bold, self.italic, self.size_pt, &self.color)?;
        if !(1..=100).contains(&self.transparency) {
            return Err(DomainError::option(
                "transparency",
                "must be between 1 and 100",
            ));
        }
        if !self.rotation_deg.is_finite() || !(0.0..=360.0).contains(&self.rotation_deg) {
            return Err(DomainError::option(
                "rotation_deg",
                "must be between 0 and 360 degrees",
            ));
        }
        check_offset("offset_x_pt", self.offset_x_pt)?;
        check_offset("offset_y_pt", self.offset_y_pt)?;
        match self.kind {
            WatermarkKind::Text => {
                if self.text.trim().is_empty() {
                    return Err(DomainError::option("text", "type the watermark text"));
                }
                if self.text.chars().count() > 200 {
                    return Err(DomainError::option(
                        "text",
                        "must be at most 200 characters",
                    ));
                }
                check_drawable("text", font, &self.text)
            }
            WatermarkKind::Image => {
                if self.image_path.is_none() {
                    return Err(DomainError::option(
                        "image_path",
                        "choose the picture to use as watermark",
                    ));
                }
                if !(1..=100).contains(&self.image_width_percent) {
                    return Err(DomainError::option(
                        "image_width_percent",
                        "must be between 1 and 100",
                    ));
                }
                Ok(())
            }
        }
    }

    /// The stamp to draw on every selected page.
    pub fn stamp(&self) -> Result<Stamp, DomainError> {
        self.validate()?;
        let content = match (self.kind, &self.image_path) {
            (WatermarkKind::Image, Some(path)) => StampContent::Image {
                path: path.clone(),
                width_fraction: f32::from(self.image_width_percent) / 100.0,
            },
            _ => {
                let (font, size, color) =
                    text_format(self.font, self.bold, self.italic, self.size_pt, &self.color)?;
                plain_text(self.text.clone(), font, size, color)
            }
        };
        Ok(Stamp {
            content,
            position: self.position,
            offset_x_pt: self.offset_x_pt,
            offset_y_pt: self.offset_y_pt,
            rotation_deg: self.rotation_deg,
            opacity: 1.0 - f32::from(self.transparency) / 100.0,
            layer: self.layer,
            tile: self.tile,
            margin_pt: EDGE_MARGIN_PT,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn file(pages: u32) -> FileFacts<'static> {
        FileFacts {
            page_count: pages,
            filename: "report",
            date: "2026-10-19",
        }
    }

    fn texts(plan: &[PageStamps]) -> Vec<(u32, String)> {
        plan.iter()
            .map(|p| {
                let StampContent::Text(t) = &p.stamps[0].content else {
                    unreachable!("text stamps only");
                };
                (p.page.get(), t.text.clone())
            })
            .collect()
    }

    #[test]
    fn page_numbers_default_to_one_digit_per_page_at_the_bottom_center() {
        let plan = PageNumbersOptions::default().plan(&file(3)).unwrap();
        assert_eq!(
            texts(&plan),
            vec![(0, "1".into()), (1, "2".into()), (2, "3".into())]
        );
        assert_eq!(plan[0].stamps[0].position, Position::BottomCenter);
        assert_eq!(plan[0].stamps[0].layer, Layer::Above);
    }

    #[test]
    fn templates_styles_and_start_numbers_combine() {
        let options = PageNumbersOptions {
            template: "Page {page} of {pages}".into(),
            start_number: 5,
            style: NumberStyle::RomanUpper,
            ..PageNumbersOptions::default()
        };
        let plan = options.plan(&file(3)).unwrap();
        assert_eq!(
            texts(&plan),
            vec![
                (0, "Page V of VII".into()),
                (1, "Page VI of VII".into()),
                (2, "Page VII of VII".into())
            ]
        );
    }

    #[test]
    fn a_skipped_cover_is_neither_numbered_nor_counted() {
        let options = PageNumbersOptions {
            skip_first_page: true,
            ..PageNumbersOptions::default()
        };
        let plan = options.plan(&file(3)).unwrap();
        assert_eq!(texts(&plan), vec![(1, "1".into()), (2, "2".into())]);
    }

    #[test]
    fn only_the_selected_pages_are_numbered_in_page_order() {
        let options = PageNumbersOptions {
            pages: PageRange::parse("3,1").unwrap(),
            ..PageNumbersOptions::default()
        };
        let plan = options.plan(&file(4)).unwrap();
        assert_eq!(texts(&plan), vec![(0, "1".into()), (2, "2".into())]);
    }

    #[test]
    fn facing_pages_mirror_the_even_pages_including_the_offset() {
        let options = PageNumbersOptions {
            position: Position::BottomRight,
            offset_x_pt: -10.0,
            facing_pages: true,
            ..PageNumbersOptions::default()
        };
        let plan = options.plan(&file(2)).unwrap();
        assert_eq!(plan[0].stamps[0].position, Position::BottomRight);
        assert_eq!(plan[0].stamps[0].offset_x_pt, -10.0);
        assert_eq!(plan[1].stamps[0].position, Position::BottomLeft);
        assert_eq!(plan[1].stamps[0].offset_x_pt, 10.0);
    }

    #[test]
    fn bad_page_number_options_name_the_field() {
        let field = |o: PageNumbersOptions| match o.validate().unwrap_err() {
            DomainError::OptionInvalid { field, .. } => field,
            other => panic!("{other:?}"),
        };
        let base = PageNumbersOptions::default;
        assert_eq!(
            field(PageNumbersOptions {
                size_pt: 0.0,
                ..base()
            }),
            "size_pt"
        );
        assert_eq!(
            field(PageNumbersOptions {
                size_pt: f32::NAN,
                ..base()
            }),
            "size_pt"
        );
        assert_eq!(
            field(PageNumbersOptions {
                color: "red".into(),
                ..base()
            }),
            "color"
        );
        assert_eq!(
            field(PageNumbersOptions {
                template: "{nope}".into(),
                ..base()
            }),
            "template"
        );
        assert_eq!(
            field(PageNumbersOptions {
                template: "\u{4E2D}".into(),
                ..base()
            }),
            "template"
        );
        assert_eq!(
            field(PageNumbersOptions {
                offset_x_pt: 1e9,
                ..base()
            }),
            "offset_x_pt"
        );
    }

    #[test]
    fn a_filename_the_font_cannot_draw_is_reported_instead_of_garbled() {
        let options = PageNumbersOptions {
            template: "{filename}".into(),
            ..PageNumbersOptions::default()
        };
        let facts = FileFacts {
            filename: "\u{4E2D}\u{6587}",
            ..file(1)
        };
        assert_eq!(
            options.plan(&facts).unwrap_err().code(),
            ErrorCode::OptionInvalid
        );
    }

    #[test]
    fn header_and_footer_place_each_text_in_its_slot_with_tokens() {
        let options = HeaderFooterOptions {
            header_left: "{filename}".into(),
            footer_center: "{page}/{pages} {date}".into(),
            ..HeaderFooterOptions::default()
        };
        let plan = options.plan(&file(2)).unwrap();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[1].stamps.len(), 2);
        let StampContent::Text(left) = &plan[1].stamps[0].content else {
            unreachable!()
        };
        assert_eq!(left.text, "report");
        assert_eq!(plan[1].stamps[0].position, Position::TopLeft);
        let StampContent::Text(foot) = &plan[1].stamps[1].content else {
            unreachable!()
        };
        assert_eq!(foot.text, "2/2 2026-10-19");
        assert_eq!(plan[1].stamps[1].position, Position::BottomCenter);
    }

    #[test]
    fn header_and_footer_need_some_text_and_report_the_slot_with_the_problem() {
        let err = HeaderFooterOptions::default().validate().unwrap_err();
        assert_eq!(err.code(), ErrorCode::OptionInvalid);
        let bad = HeaderFooterOptions {
            footer_right: "{oops}".into(),
            ..HeaderFooterOptions::default()
        };
        match bad.validate().unwrap_err() {
            DomainError::OptionInvalid { field, .. } => assert_eq!(field, "footer_right"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bates_numbers_are_padded_and_continue_across_files_in_list_order() {
        let options = BatesOptions {
            prefix: "ACME-".into(),
            suffix: "-X".into(),
            digits: 4,
            start: 41,
            ..BatesOptions::default()
        };
        assert_eq!(options.label(7), "ACME-0007-X");
        assert_eq!(options.label(123_456), "ACME-123456-X");
        assert_eq!(options.starts(&[3, 2, 5]), vec![41, 44, 46]);
        let separate = BatesOptions {
            continue_across_files: false,
            ..options.clone()
        };
        assert_eq!(separate.starts(&[3, 2]), vec![41, 41]);
        let plan = options.plan(2, 44).unwrap();
        assert_eq!(
            texts(&plan),
            vec![(0, "ACME-0044-X".into()), (1, "ACME-0045-X".into())]
        );
        assert_eq!(plan[0].stamps[0].position, Position::BottomRight);
    }

    #[test]
    fn bates_rejects_bad_digits_and_undrawable_text() {
        for o in [
            BatesOptions {
                digits: 0,
                ..BatesOptions::default()
            },
            BatesOptions {
                digits: 13,
                ..BatesOptions::default()
            },
            BatesOptions {
                prefix: "\u{4E2D}".into(),
                ..BatesOptions::default()
            },
            BatesOptions {
                start: u64::MAX,
                ..BatesOptions::default()
            },
        ] {
            assert_eq!(o.validate().unwrap_err().code(), ErrorCode::OptionInvalid);
        }
    }

    #[test]
    fn text_watermarks_become_one_stamp_with_opacity_from_transparency() {
        let stamp = WatermarkOptions::default().stamp().unwrap();
        assert!((stamp.opacity - 0.5).abs() < 1e-6);
        assert_eq!(stamp.position, Position::Center);
        assert_eq!(stamp.layer, Layer::Above);
        let solid = WatermarkOptions {
            transparency: 1,
            layer: Layer::Below,
            tile: true,
            rotation_deg: 45.0,
            ..WatermarkOptions::default()
        }
        .stamp()
        .unwrap();
        assert!((solid.opacity - 0.99).abs() < 1e-6);
        assert!(solid.tile);
        assert_eq!(solid.rotation_deg, 45.0);
    }

    #[test]
    fn picture_watermarks_need_a_file_and_use_a_width_fraction() {
        let none = WatermarkOptions {
            kind: WatermarkKind::Image,
            ..WatermarkOptions::default()
        };
        assert_eq!(
            none.validate().unwrap_err().code(),
            ErrorCode::OptionInvalid
        );
        let ok = WatermarkOptions {
            kind: WatermarkKind::Image,
            image_path: Some(PathBuf::from("logo.png")),
            image_width_percent: 25,
            ..WatermarkOptions::default()
        };
        match ok.stamp().unwrap().content {
            StampContent::Image { width_fraction, .. } => assert_eq!(width_fraction, 0.25),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn watermark_bounds_are_enforced() {
        for o in [
            WatermarkOptions {
                transparency: 0,
                ..WatermarkOptions::default()
            },
            WatermarkOptions {
                transparency: 101,
                ..WatermarkOptions::default()
            },
            WatermarkOptions {
                rotation_deg: 361.0,
                ..WatermarkOptions::default()
            },
            WatermarkOptions {
                rotation_deg: -1.0,
                ..WatermarkOptions::default()
            },
            WatermarkOptions {
                text: "  ".into(),
                ..WatermarkOptions::default()
            },
            WatermarkOptions {
                text: "\u{4E2D}".into(),
                ..WatermarkOptions::default()
            },
        ] {
            assert_eq!(
                o.validate().unwrap_err().code(),
                ErrorCode::OptionInvalid,
                "{o:?}"
            );
        }
    }

    #[test]
    fn json_round_trips_with_documented_names() {
        let o: PageNumbersOptions = serde_json::from_str(
            r#"{"position":"top-right","style":"roman-lower","font":"times"}"#,
        )
        .unwrap();
        assert_eq!(o.position, Position::TopRight);
        assert_eq!(o.style, NumberStyle::RomanLower);
        assert_eq!(o.font, FontFamily::Times);
        assert_eq!(o.template, "{page}");
        assert!(serde_json::from_str::<PageNumbersOptions>(r#"{"posn":"x"}"#).is_err());
        let w: WatermarkOptions = serde_json::from_str(r#"{"layer":"below"}"#).unwrap();
        assert_eq!(w.layer, Layer::Below);
    }
}
