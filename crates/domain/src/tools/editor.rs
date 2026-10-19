//! Options of the tools that change what a page shows or asks: Edit PDF (row 42), Annotate
//! (row 43), Fill and sign (row 45), Create forms (row 46) and Sign PDF (row 53).
//!
//! The editor in the app builds these lists by drawing on a preview of the page, so every
//! position is a fraction of the page as it is displayed, measured from the top left, like the
//! regions of the other tools. Pages count from 1.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::DomainError;
use crate::fonts::FontFamily;
use crate::stamp::Rgb;
use crate::tools::common::Region;

/// Most items one run may carry.
pub const MAX_ITEMS: usize = 2_000;
/// Longest text of one item, in characters.
pub const MAX_TEXT_CHARS: usize = 20_000;
/// Smallest and largest font size, in points.
pub const SIZE_RANGE: (f32, f32) = (4.0, 400.0);
/// Most points in one freehand stroke, and most strokes in one drawing.
pub const MAX_STROKE_POINTS: usize = 20_000;
/// Most strokes in one drawing.
pub const MAX_STROKES: usize = 500;
/// Longest field name.
pub const MAX_NAME_CHARS: usize = 200;
/// Longest picture kept inside options, as base64 characters (8 MB of picture).
pub const MAX_IMAGE_DATA: usize = 11_000_000;

/// A point on a page, as fractions of the displayed page from its top left.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Point {
    /// Distance from the left edge, 0 to 1.
    pub x: f32,
    /// Distance from the top edge, 0 to 1.
    pub y: f32,
}

impl Point {
    fn validate(&self, what: &str) -> Result<(), DomainError> {
        let ok = |v: f32| v.is_finite() && (-0.001..=1.001).contains(&v);
        if ok(self.x) && ok(self.y) {
            Ok(())
        } else {
            Err(DomainError::option(what, "a point lies outside the page"))
        }
    }
}

fn page_ok(page: u32, count: u32, what: &str) -> Result<(), DomainError> {
    if page == 0 || page > count {
        Err(DomainError::option(
            what,
            format!("page {page} is outside this document of {count} pages"),
        ))
    } else {
        Ok(())
    }
}

fn color_ok(color: &str, what: &str) -> Result<Rgb, DomainError> {
    Rgb::parse(color).map_err(|_| DomainError::option(what, "use a color such as #1F2937"))
}

fn region_ok(region: &Region, what: &str) -> Result<(), DomainError> {
    region
        .validate()
        .map_err(|_| DomainError::option(what, "a box lies outside the page"))
}

fn size_ok(size: f32, what: &str) -> Result<(), DomainError> {
    if size.is_finite() && (SIZE_RANGE.0..=SIZE_RANGE.1).contains(&size) {
        Ok(())
    } else {
        Err(DomainError::option(
            what,
            format!(
                "the size must be between {} and {} points",
                SIZE_RANGE.0, SIZE_RANGE.1
            ),
        ))
    }
}

fn width_ok(width: f32, what: &str) -> Result<(), DomainError> {
    if width.is_finite() && (0.1..=100.0).contains(&width) {
        Ok(())
    } else {
        Err(DomainError::option(
            what,
            "the line width must be between 0.1 and 100 points",
        ))
    }
}

fn text_ok(text: &str, what: &str) -> Result<(), DomainError> {
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(DomainError::option(
            what,
            format!("a text may have at most {MAX_TEXT_CHARS} characters"),
        ));
    }
    Ok(())
}

fn count_ok(count: usize, what: &str) -> Result<(), DomainError> {
    if count > MAX_ITEMS {
        Err(DomainError::option(
            what,
            format!("a run may carry at most {MAX_ITEMS} entries"),
        ))
    } else {
        Ok(())
    }
}

/// How lines of text sit in their box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TextAlign {
    /// Against the left edge (default).
    #[default]
    Left,
    /// Centered.
    Center,
    /// Against the right edge.
    Right,
}

/// The look of text: a standard font, or a font installed on the computer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TextStyle {
    /// A standard font family. Default Helvetica.
    pub font: FontFamily,
    /// Name of an installed font family, such as `Segoe UI`. It replaces `font` when set.
    pub installed_font: Option<String>,
    /// Bold. Default off.
    pub bold: bool,
    /// Italic. Default off.
    pub italic: bool,
    /// Size in points. Default 12.
    pub size_pt: f32,
    /// Color as `#RRGGBB`. Default black.
    pub color: String,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font: FontFamily::Helvetica,
            installed_font: None,
            bold: false,
            italic: false,
            size_pt: 12.0,
            color: "#000000".to_owned(),
        }
    }
}

impl TextStyle {
    fn validate(&self) -> Result<(), DomainError> {
        size_ok(self.size_pt, "size_pt")?;
        color_ok(&self.color, "color")?;
        if let Some(name) = &self.installed_font
            && (name.trim().is_empty() || name.chars().count() > 100)
        {
            return Err(DomainError::option(
                "installed_font",
                "give the name of an installed font family",
            ));
        }
        Ok(())
    }
}

/// Text added to a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextItem {
    /// The page, from 1.
    pub page: u32,
    /// Where the text goes. Lines wrap at its width and start at its top.
    pub region: Region,
    /// The text; a line break starts a new line.
    pub text: String,
    /// The look of the text.
    #[serde(default)]
    pub style: TextStyle,
    /// How lines sit in the box. Default left.
    #[serde(default)]
    pub align: TextAlign,
}

/// A picture added to a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageItem {
    /// The page, from 1.
    pub page: u32,
    /// Where the picture goes. It keeps its proportions and is centered in the box.
    pub region: Region,
    /// A PNG or JPEG file.
    pub path: PathBuf,
}

/// The shapes that can be drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ShapeKind {
    /// A rectangle.
    Rectangle,
    /// An ellipse that fills the box.
    Ellipse,
    /// A line from the top left corner of the box to the bottom right one.
    Line,
    /// A line with an arrow head at its end.
    Arrow,
}

/// A shape drawn on a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShapeItem {
    /// The page, from 1.
    pub page: u32,
    /// Which shape.
    pub shape: ShapeKind,
    /// The box the shape fills.
    pub region: Region,
    /// Run a line or arrow from the bottom left corner to the top right one instead.
    #[serde(default)]
    pub flip: bool,
    /// Outline color as `#RRGGBB`. Default red.
    #[serde(default = "default_stroke")]
    pub stroke_color: String,
    /// Outline width in points. Default 2.
    #[serde(default = "default_stroke_width")]
    pub stroke_width_pt: f32,
    /// Fill color as `#RRGGBB`; none leaves the inside empty.
    #[serde(default)]
    pub fill_color: Option<String>,
}

fn default_stroke() -> String {
    "#D32F2F".to_owned()
}

fn default_stroke_width() -> f32 {
    2.0
}

/// Where a link leads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum LinkTarget {
    /// A web address (http or https) or a mail address (mailto).
    Uri {
        /// The address.
        uri: String,
    },
    /// Another page of the same document.
    Page {
        /// The page, from 1.
        page: u32,
    },
}

/// A clickable area.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinkItem {
    /// The page, from 1.
    pub page: u32,
    /// The clickable area.
    pub region: Region,
    /// Where it leads.
    pub target: LinkTarget,
}

/// A patch that covers what is under it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WhiteoutItem {
    /// The page, from 1.
    pub page: u32,
    /// The area to cover.
    pub region: Region,
    /// Color of the patch as `#RRGGBB`. Default white.
    #[serde(default = "default_white")]
    pub color: String,
}

fn default_white() -> String {
    "#FFFFFF".to_owned()
}

/// One change to the content of a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum EditItem {
    /// Text.
    Text(TextItem),
    /// A picture.
    Image(ImageItem),
    /// A shape.
    Shape(ShapeItem),
    /// A link.
    Link(LinkItem),
    /// A patch over existing content.
    Whiteout(WhiteoutItem),
}

impl EditItem {
    /// The page the item is on, from 1.
    pub fn page(&self) -> u32 {
        match self {
            Self::Text(i) => i.page,
            Self::Image(i) => i.page,
            Self::Shape(i) => i.page,
            Self::Link(i) => i.page,
            Self::Whiteout(i) => i.page,
        }
    }

    /// Checks the item against a document of `count` pages.
    pub fn validate(&self, count: u32) -> Result<(), DomainError> {
        page_ok(self.page(), count, "items")?;
        match self {
            Self::Text(i) => {
                region_ok(&i.region, "items")?;
                text_ok(&i.text, "items")?;
                i.style.validate()
            }
            Self::Image(i) => region_ok(&i.region, "items"),
            Self::Shape(i) => {
                region_ok(&i.region, "items")?;
                color_ok(&i.stroke_color, "stroke_color")?;
                width_ok(i.stroke_width_pt, "stroke_width_pt")?;
                if let Some(fill) = &i.fill_color {
                    color_ok(fill, "fill_color")?;
                }
                Ok(())
            }
            Self::Link(i) => {
                region_ok(&i.region, "items")?;
                match &i.target {
                    LinkTarget::Uri { uri } => check_uri(uri),
                    LinkTarget::Page { page } => page_ok(*page, count, "items"),
                }
            }
            Self::Whiteout(i) => {
                region_ok(&i.region, "items")?;
                color_ok(&i.color, "color").map(|_| ())
            }
        }
    }
}

/// Checks a link address: only web and mail addresses, with no control characters.
pub fn check_uri(uri: &str) -> Result<(), DomainError> {
    let lower = uri.trim().to_ascii_lowercase();
    let allowed = ["http://", "https://", "mailto:"]
        .iter()
        .any(|p| lower.starts_with(p));
    if !allowed || uri.len() > 2_000 || uri.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(DomainError::option(
            "uri",
            "use a web address starting with http:// or https://, or a mail address starting with mailto:",
        ));
    }
    Ok(())
}

/// Options for Edit PDF.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EditPdfOptions {
    /// Everything added to the pages, in drawing order.
    pub items: Vec<EditItem>,
}

impl EditPdfOptions {
    /// Checks every item against a document of `count` pages. At least one item is needed.
    pub fn validate(&self, count: u32) -> Result<(), DomainError> {
        if self.items.is_empty() {
            return Err(DomainError::option(
                "items",
                "add at least one text, picture, shape, link or patch",
            ));
        }
        count_ok(self.items.len(), "items")?;
        self.items.iter().try_for_each(|i| i.validate(count))
    }
}

// ----- annotations -------------------------------------------------------------------------

fn default_highlight() -> String {
    "#FFEB3B".to_owned()
}

fn default_ink() -> String {
    "#1565C0".to_owned()
}

fn default_note() -> String {
    "#FFD54F".to_owned()
}

/// A note, mark or drawing that belongs to the document but is not part of the page content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Annotation {
    /// Text marked with a colored band.
    Highlight(MarkupAnnotation),
    /// Text with a line through it.
    Strikeout(MarkupAnnotation),
    /// Text with a line under it.
    Underline(MarkupAnnotation),
    /// A sticky note.
    Note(NoteAnnotation),
    /// A freehand drawing.
    Freehand(FreehandAnnotation),
}

/// The areas a highlight, strikeout or underline covers: one box for each line of text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkupAnnotation {
    /// The page, from 1.
    pub page: u32,
    /// The marked areas.
    pub regions: Vec<Region>,
    /// Color as `#RRGGBB`. Default yellow.
    #[serde(default = "default_highlight")]
    pub color: String,
}

/// A sticky note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoteAnnotation {
    /// The page, from 1.
    pub page: u32,
    /// Where the note's icon sits.
    pub at: Point,
    /// The text of the note.
    pub text: String,
    /// Color of the icon as `#RRGGBB`.
    #[serde(default = "default_note")]
    pub color: String,
}

/// A freehand drawing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FreehandAnnotation {
    /// The page, from 1.
    pub page: u32,
    /// The strokes; each is a list of points.
    pub strokes: Vec<Vec<Point>>,
    /// Color as `#RRGGBB`.
    #[serde(default = "default_ink")]
    pub color: String,
    /// Line width in points. Default 2.
    #[serde(default = "default_stroke_width")]
    pub width_pt: f32,
}

impl Annotation {
    /// The page the annotation is on, from 1.
    pub fn page(&self) -> u32 {
        match self {
            Self::Highlight(a) | Self::Strikeout(a) | Self::Underline(a) => a.page,
            Self::Note(a) => a.page,
            Self::Freehand(a) => a.page,
        }
    }

    /// Checks the annotation against a document of `count` pages.
    pub fn validate(&self, count: u32) -> Result<(), DomainError> {
        page_ok(self.page(), count, "annotations")?;
        match self {
            Self::Highlight(a) | Self::Strikeout(a) | Self::Underline(a) => {
                if a.regions.is_empty() {
                    return Err(DomainError::option(
                        "annotations",
                        "mark at least one area of text",
                    ));
                }
                if a.regions.len() > 1_000 {
                    return Err(DomainError::option("annotations", "too many marked areas"));
                }
                a.regions
                    .iter()
                    .try_for_each(|r| region_ok(r, "annotations"))?;
                color_ok(&a.color, "color").map(|_| ())
            }
            Self::Note(a) => {
                a.at.validate("annotations")?;
                text_ok(&a.text, "annotations")?;
                color_ok(&a.color, "color").map(|_| ())
            }
            Self::Freehand(a) => {
                strokes_ok(&a.strokes, "annotations")?;
                width_ok(a.width_pt, "width_pt")?;
                color_ok(&a.color, "color").map(|_| ())
            }
        }
    }
}

fn strokes_ok(strokes: &[Vec<Point>], what: &str) -> Result<(), DomainError> {
    if strokes.is_empty() || strokes.iter().all(Vec::is_empty) {
        return Err(DomainError::option(what, "draw at least one stroke"));
    }
    if strokes.len() > MAX_STROKES {
        return Err(DomainError::option(
            what,
            "the drawing has too many strokes",
        ));
    }
    for stroke in strokes {
        if stroke.len() > MAX_STROKE_POINTS {
            return Err(DomainError::option(what, "a stroke has too many points"));
        }
        stroke.iter().try_for_each(|p| p.validate(what))?;
    }
    Ok(())
}

/// Options for Annotate.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AnnotateOptions {
    /// The notes, marks and drawings to add.
    pub annotations: Vec<Annotation>,
}

impl AnnotateOptions {
    /// Checks every annotation against a document of `count` pages. At least one is needed.
    pub fn validate(&self, count: u32) -> Result<(), DomainError> {
        if self.annotations.is_empty() {
            return Err(DomainError::option(
                "annotations",
                "add at least one highlight, note or drawing",
            ));
        }
        count_ok(self.annotations.len(), "annotations")?;
        self.annotations.iter().try_for_each(|a| a.validate(count))
    }
}

// ----- signatures --------------------------------------------------------------------------

/// How a date is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DateFormat {
    /// 2026-10-19 (default).
    #[default]
    Iso,
    /// 19 October 2026.
    Long,
    /// 10/19/2026.
    Us,
    /// 19/10/2026.
    Eu,
}

impl DateFormat {
    /// Writes a date in this format. `month` is 1 to 12.
    pub fn write(self, year: i32, month: u32, day: u32) -> String {
        const MONTHS: [&str; 12] = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        match self {
            Self::Iso => format!("{year:04}-{month:02}-{day:02}"),
            Self::Long => format!(
                "{day} {} {year}",
                MONTHS
                    .get((month.max(1) - 1) as usize)
                    .copied()
                    .unwrap_or("")
            ),
            Self::Us => format!("{month:02}/{day:02}/{year:04}"),
            Self::Eu => format!("{day:02}/{month:02}/{year:04}"),
        }
    }
}

/// What a visual signature looks like.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SignatureSource {
    /// Strokes drawn with the mouse, pen or finger. Points are fractions of the signature's box.
    Drawn {
        /// The strokes.
        strokes: Vec<Vec<Point>>,
        /// Color as `#RRGGBB`.
        #[serde(default = "default_ink_black")]
        color: String,
        /// Line width in points. Default 1.5.
        #[serde(default = "default_sign_width")]
        width_pt: f32,
    },
    /// A name or initials typed and set in a font.
    Typed {
        /// The text.
        text: String,
        /// The look of the text.
        #[serde(default = "script_style")]
        style: TextStyle,
    },
    /// A picture of a signature or a company stamp (PNG or JPEG).
    Image {
        /// The file.
        path: PathBuf,
    },
    /// A picture kept inside the options, such as a saved signature, as base64 text.
    ImageData {
        /// The PNG or JPEG file, base64 encoded.
        data: String,
    },
    /// Today's date.
    Date {
        /// How it is written.
        #[serde(default)]
        format: DateFormat,
        /// The look of the text.
        #[serde(default)]
        style: TextStyle,
    },
}

fn default_ink_black() -> String {
    "#111827".to_owned()
}

fn default_sign_width() -> f32 {
    1.5
}

fn script_style() -> TextStyle {
    TextStyle {
        font: FontFamily::Times,
        italic: true,
        size_pt: 24.0,
        ..TextStyle::default()
    }
}

/// A visual signature, initials, stamp or date placed on a page. It is a picture of a signature,
/// not a digital signature: it proves nothing about who placed it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SignatureItem {
    /// The page, from 1.
    pub page: u32,
    /// Where it goes. Drawn strokes, pictures and typed text are fitted into it.
    pub region: Region,
    /// What it shows.
    pub source: SignatureSource,
}

impl SignatureItem {
    /// Checks the item against a document of `count` pages.
    pub fn validate(&self, count: u32) -> Result<(), DomainError> {
        page_ok(self.page, count, "signatures")?;
        region_ok(&self.region, "signatures")?;
        match &self.source {
            SignatureSource::Drawn {
                strokes,
                color,
                width_pt,
            } => {
                strokes_ok(strokes, "signatures")?;
                width_ok(*width_pt, "width_pt")?;
                color_ok(color, "color").map(|_| ())
            }
            SignatureSource::Typed { text, style } => {
                if text.trim().is_empty() {
                    return Err(DomainError::option(
                        "signatures",
                        "type the name or initials",
                    ));
                }
                text_ok(text, "signatures")?;
                style.validate()
            }
            SignatureSource::Image { .. } => Ok(()),
            SignatureSource::ImageData { data } => {
                if data.is_empty() || data.len() > MAX_IMAGE_DATA {
                    return Err(DomainError::option(
                        "signatures",
                        "a saved picture is empty or larger than 8 MB",
                    ));
                }
                Ok(())
            }
            SignatureSource::Date { style, .. } => style.validate(),
        }
    }
}

fn signatures_ok(signatures: &[SignatureItem], count: u32) -> Result<(), DomainError> {
    count_ok(signatures.len(), "signatures")?;
    signatures.iter().try_for_each(|s| s.validate(count))
}

/// Options for Sign PDF (visual signatures).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SignPdfOptions {
    /// The signatures, initials, stamps and dates to place.
    pub signatures: Vec<SignatureItem>,
}

impl SignPdfOptions {
    /// Checks the signatures against a document of `count` pages. At least one is needed.
    pub fn validate(&self, count: u32) -> Result<(), DomainError> {
        if self.signatures.is_empty() {
            return Err(DomainError::option(
                "signatures",
                "place at least one signature, initials, stamp or date",
            ));
        }
        signatures_ok(&self.signatures, count)
    }
}

// ----- forms -------------------------------------------------------------------------------

/// A value for a form field, by the field's name. Text goes in text fields, `true` or `false`
/// (or the field's own on value) in check boxes, the option's name in radio groups and lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldValue {
    /// The full name of the field.
    pub name: String,
    /// The value.
    pub value: String,
}

/// Options for Fill and sign.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct FillSignOptions {
    /// Values for fields of the form.
    pub values: Vec<FieldValue>,
    /// Signatures, initials, stamps and dates to place.
    pub signatures: Vec<SignatureItem>,
}

impl FillSignOptions {
    /// Checks the values and signatures. At least one of them is needed.
    pub fn validate(&self, count: u32) -> Result<(), DomainError> {
        if self.values.is_empty() && self.signatures.is_empty() {
            return Err(DomainError::option(
                "values",
                "fill at least one field or place at least one signature",
            ));
        }
        count_ok(self.values.len(), "values")?;
        for (i, v) in self.values.iter().enumerate() {
            if v.name.trim().is_empty() || v.name.chars().count() > MAX_NAME_CHARS {
                return Err(DomainError::option("values", "a value has no field name"));
            }
            text_ok(&v.value, "values")?;
            if self.values[..i].iter().any(|o| o.name == v.name) {
                return Err(DomainError::option(
                    "values",
                    format!("the field \"{}\" has two values", v.name),
                ));
            }
        }
        signatures_ok(&self.signatures, count)
    }
}

/// The kinds of field Create forms can add.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum NewFieldKind {
    /// A line or box of text.
    Text {
        /// Allow several lines.
        #[serde(default)]
        multiline: bool,
        /// Longest text, in characters; 0 for no limit.
        #[serde(default)]
        max_len: u32,
        /// Text the field starts with.
        #[serde(default)]
        value: String,
    },
    /// A box that is ticked or not.
    Checkbox {
        /// Start ticked.
        #[serde(default)]
        checked: bool,
    },
    /// One choice of a group. Fields with the same name form the group.
    Radio {
        /// The name of this choice.
        option: String,
        /// Start selected.
        #[serde(default)]
        checked: bool,
    },
    /// A list to choose from.
    Dropdown {
        /// The choices.
        options: Vec<String>,
        /// The choice the field starts with.
        #[serde(default)]
        value: String,
    },
    /// A place to sign.
    Signature,
}

/// A field added to a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewField {
    /// The page, from 1.
    pub page: u32,
    /// The area the field covers.
    pub region: Region,
    /// The name of the field. Names are unique, except in a radio group.
    pub name: String,
    /// What kind of field it is.
    pub kind: NewFieldKind,
    /// The form cannot be submitted without it. Default off.
    #[serde(default)]
    pub required: bool,
    /// Help shown when the pointer rests on the field.
    #[serde(default)]
    pub tooltip: String,
}

/// Options for Create forms.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CreateFormsOptions {
    /// The fields to add.
    pub fields: Vec<NewField>,
}

impl CreateFormsOptions {
    /// Checks the fields against a document of `count` pages. At least one is needed.
    pub fn validate(&self, count: u32) -> Result<(), DomainError> {
        if self.fields.is_empty() {
            return Err(DomainError::option("fields", "add at least one field"));
        }
        count_ok(self.fields.len(), "fields")?;
        for (i, f) in self.fields.iter().enumerate() {
            page_ok(f.page, count, "fields")?;
            region_ok(&f.region, "fields")?;
            let name = f.name.trim();
            if name.is_empty()
                || f.name.chars().count() > MAX_NAME_CHARS
                || f.name.contains('.')
                || f.name.chars().any(char::is_control)
            {
                return Err(DomainError::option(
                    "fields",
                    "a field name must not be empty, hold a dot or exceed 200 characters",
                ));
            }
            text_ok(&f.tooltip, "fields")?;
            let radio = matches!(f.kind, NewFieldKind::Radio { .. });
            for other in &self.fields[..i] {
                if other.name == f.name {
                    let both_radio = radio && matches!(other.kind, NewFieldKind::Radio { .. });
                    if !both_radio {
                        return Err(DomainError::option(
                            "fields",
                            format!("two fields are named \"{}\"", f.name),
                        ));
                    }
                }
            }
            match &f.kind {
                NewFieldKind::Text { value, .. } => text_ok(value, "fields")?,
                NewFieldKind::Radio { option, .. } => {
                    if option.trim().is_empty() || option.chars().count() > MAX_NAME_CHARS {
                        return Err(DomainError::option("fields", "name every radio choice"));
                    }
                }
                NewFieldKind::Dropdown { options, value } => {
                    if options.is_empty() || options.len() > 500 {
                        return Err(DomainError::option(
                            "fields",
                            "a list needs between 1 and 500 choices",
                        ));
                    }
                    if !value.is_empty() && !options.contains(value) {
                        return Err(DomainError::option(
                            "fields",
                            "the starting choice is not in the list",
                        ));
                    }
                }
                NewFieldKind::Checkbox { .. } | NewFieldKind::Signature => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region() -> Region {
        Region {
            x: 0.1,
            y: 0.1,
            width: 0.3,
            height: 0.1,
        }
    }

    fn text(page: u32) -> EditItem {
        EditItem::Text(TextItem {
            page,
            region: region(),
            text: "Hello".to_owned(),
            style: TextStyle::default(),
            align: TextAlign::Left,
        })
    }

    #[test]
    fn items_are_checked_for_pages_boxes_colors_and_sizes() {
        let ok = EditPdfOptions {
            items: vec![text(2)],
        };
        assert!(ok.validate(3).is_ok());
        assert!(ok.validate(1).is_err(), "page 2 of 1");
        assert!(EditPdfOptions::default().validate(3).is_err(), "empty");
        let big = EditItem::Text(TextItem {
            style: TextStyle {
                size_pt: 1.0,
                ..TextStyle::default()
            },
            ..match text(1) {
                EditItem::Text(t) => t,
                _ => unreachable!(),
            }
        });
        assert!(EditPdfOptions { items: vec![big] }.validate(1).is_err());
        let outside = EditItem::Whiteout(WhiteoutItem {
            page: 1,
            region: Region {
                x: 0.8,
                y: 0.1,
                width: 0.5,
                height: 0.1,
            },
            color: "#FFFFFF".to_owned(),
        });
        assert!(
            EditPdfOptions {
                items: vec![outside]
            }
            .validate(1)
            .is_err()
        );
        let bad_color = EditItem::Shape(ShapeItem {
            page: 1,
            shape: ShapeKind::Line,
            region: region(),
            flip: false,
            stroke_color: "red".to_owned(),
            stroke_width_pt: 2.0,
            fill_color: None,
        });
        assert!(
            EditPdfOptions {
                items: vec![bad_color]
            }
            .validate(1)
            .is_err()
        );
    }

    #[test]
    fn links_allow_only_web_mail_and_pages_of_the_document() {
        for good in ["https://example.com/a?b=1", "http://x.y", "mailto:a@b.co"] {
            assert!(check_uri(good).is_ok(), "{good}");
        }
        for bad in [
            "javascript:alert(1)",
            "file:///C:/x",
            "ftp://x",
            "https://a b",
            "https://a\n",
            "data:text/html,x",
            "",
        ] {
            assert!(check_uri(bad).is_err(), "{bad:?}");
        }
        let link = |page| {
            EditItem::Link(LinkItem {
                page: 1,
                region: region(),
                target: LinkTarget::Page { page },
            })
        };
        assert!(link(2).validate(3).is_ok());
        assert!(link(9).validate(3).is_err());
    }

    #[test]
    fn json_is_tagged_and_unknown_fields_are_refused() {
        let items: EditPdfOptions = serde_json::from_str(
            r##"{"items":[{"kind":"text","page":1,"region":{"x":0.1,"y":0.1,"width":0.3,"height":0.1},"text":"Hi","style":{"size_pt":14,"bold":true}},
                          {"kind":"whiteout","page":1,"region":{"x":0,"y":0,"width":0.5,"height":0.5}},
                          {"kind":"link","page":1,"region":{"x":0,"y":0,"width":0.5,"height":0.5},"target":{"type":"uri","uri":"https://a.b"}}]}"##,
        )
        .unwrap();
        assert_eq!(items.items.len(), 3);
        let EditItem::Text(t) = &items.items[0] else {
            panic!("text")
        };
        assert_eq!((t.style.size_pt, t.style.bold), (14.0, true));
        assert_eq!(t.style.color, "#000000");
        assert!(items.validate(1).is_ok());
        assert!(
            serde_json::from_str::<EditPdfOptions>(
                r#"{"items":[{"kind":"text","page":1,"bogus":1}]}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<AnnotateOptions>(r#"{"annotations":[{"kind":"sparkle"}]}"#)
                .is_err()
        );
    }

    #[test]
    fn annotations_need_marked_areas_strokes_and_valid_points() {
        let mark = |regions: Vec<Region>| {
            Annotation::Highlight(MarkupAnnotation {
                page: 1,
                regions,
                color: default_highlight(),
            })
        };
        let options = |a| AnnotateOptions {
            annotations: vec![a],
        };
        assert!(options(mark(vec![region()])).validate(1).is_ok());
        assert!(options(mark(vec![])).validate(1).is_err());
        let ink = |strokes| {
            Annotation::Freehand(FreehandAnnotation {
                page: 1,
                strokes,
                color: default_ink(),
                width_pt: 2.0,
            })
        };
        assert!(
            options(ink(vec![vec![
                Point { x: 0.1, y: 0.1 },
                Point { x: 0.2, y: 0.3 }
            ]]))
            .validate(1)
            .is_ok()
        );
        assert!(options(ink(vec![])).validate(1).is_err());
        assert!(
            options(ink(vec![vec![Point { x: 2.0, y: 0.1 }]]))
                .validate(1)
                .is_err()
        );
        let note = Annotation::Note(NoteAnnotation {
            page: 1,
            at: Point { x: 0.5, y: 0.5 },
            text: "Check this".to_owned(),
            color: default_note(),
        });
        assert!(options(note).validate(1).is_ok());
    }

    #[test]
    fn dates_are_written_in_four_formats() {
        assert_eq!(DateFormat::Iso.write(2026, 10, 9), "2026-10-09");
        assert_eq!(DateFormat::Long.write(2026, 10, 9), "9 October 2026");
        assert_eq!(DateFormat::Us.write(2026, 10, 9), "10/09/2026");
        assert_eq!(DateFormat::Eu.write(2026, 10, 9), "09/10/2026");
    }

    #[test]
    fn signatures_are_drawn_typed_pictured_or_dated() {
        let item = |source| SignatureItem {
            page: 1,
            region: region(),
            source,
        };
        let drawn = item(SignatureSource::Drawn {
            strokes: vec![vec![Point { x: 0.0, y: 0.5 }, Point { x: 1.0, y: 0.5 }]],
            color: default_ink_black(),
            width_pt: 1.5,
        });
        assert!(
            SignPdfOptions {
                signatures: vec![drawn]
            }
            .validate(1)
            .is_ok()
        );
        let typed = item(SignatureSource::Typed {
            text: "  ".to_owned(),
            style: script_style(),
        });
        assert!(
            SignPdfOptions {
                signatures: vec![typed]
            }
            .validate(1)
            .is_err()
        );
        assert!(SignPdfOptions::default().validate(1).is_err());
        let date = item(SignatureSource::Date {
            format: DateFormat::Long,
            style: TextStyle::default(),
        });
        assert!(
            SignPdfOptions {
                signatures: vec![date]
            }
            .validate(1)
            .is_ok()
        );
    }

    #[test]
    fn fill_values_need_names_and_no_duplicates() {
        let value = |n: &str, v: &str| FieldValue {
            name: n.to_owned(),
            value: v.to_owned(),
        };
        let ok = FillSignOptions {
            values: vec![value("name", "Ada"), value("agree", "true")],
            signatures: vec![],
        };
        assert!(ok.validate(1).is_ok());
        assert!(FillSignOptions::default().validate(1).is_err());
        let twice = FillSignOptions {
            values: vec![value("a", "1"), value("a", "2")],
            signatures: vec![],
        };
        assert!(twice.validate(1).is_err());
        let nameless = FillSignOptions {
            values: vec![value(" ", "1")],
            signatures: vec![],
        };
        assert!(nameless.validate(1).is_err());
    }

    fn field(name: &str, kind: NewFieldKind) -> NewField {
        NewField {
            page: 1,
            region: region(),
            name: name.to_owned(),
            kind,
            required: false,
            tooltip: String::new(),
        }
    }

    #[test]
    fn form_fields_have_unique_names_except_radio_groups() {
        let text = || NewFieldKind::Text {
            multiline: false,
            max_len: 0,
            value: String::new(),
        };
        let radio = |o: &str| NewFieldKind::Radio {
            option: o.to_owned(),
            checked: false,
        };
        let ok = CreateFormsOptions {
            fields: vec![
                field("name", text()),
                field("size", radio("S")),
                field("size", radio("M")),
            ],
        };
        assert!(ok.validate(1).is_ok());
        let dup = CreateFormsOptions {
            fields: vec![field("name", text()), field("name", text())],
        };
        assert!(dup.validate(1).is_err());
        let mixed = CreateFormsOptions {
            fields: vec![field("size", radio("S")), field("size", text())],
        };
        assert!(mixed.validate(1).is_err());
        for bad in ["", "a.b", "tab\tname"] {
            let o = CreateFormsOptions {
                fields: vec![field(bad, text())],
            };
            assert!(o.validate(1).is_err(), "{bad:?}");
        }
        let list = |value: &str| CreateFormsOptions {
            fields: vec![field(
                "color",
                NewFieldKind::Dropdown {
                    options: vec!["red".into(), "blue".into()],
                    value: value.to_owned(),
                },
            )],
        };
        assert!(list("red").validate(1).is_ok());
        assert!(list("green").validate(1).is_err());
        assert!(CreateFormsOptions::default().validate(1).is_err());
    }
}
