//! The fonts text can be drawn with: the 12 standard fonts every reader has, and TrueType fonts
//! installed on the computer, which are embedded whole as simple WinAnsi fonts.

use application::AppError;
use application::ports::FontCatalog;
use domain::ErrorCode;
use domain::fonts::StandardFont;
use domain::tools::editor::TextStyle;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// Largest font file embedded.
const MAX_FONT_BYTES: u64 = 40 * 1024 * 1024;

/// One font file that can be embedded.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    family: String,
    bold: bool,
    italic: bool,
    weight: u16,
    path: PathBuf,
}

/// Folders that hold fonts: the system's and the current user's.
fn default_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(windir) = std::env::var_os("WINDIR") {
        dirs.push(PathBuf::from(windir).join("Fonts"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(
            PathBuf::from(local)
                .join("Microsoft")
                .join("Windows")
                .join("Fonts"),
        );
    }
    dirs
}

fn is_plain_truetype(data: &[u8]) -> bool {
    data.starts_with(&[0, 1, 0, 0]) || data.starts_with(b"true")
}

fn family_of(face: &ttf_parser::Face<'_>) -> Option<String> {
    face.names()
        .into_iter()
        .filter(|n| n.name_id == ttf_parser::name_id::FAMILY && n.is_unicode())
        .find_map(|n| n.to_string())
}

/// Reads the fonts of the folders: only plain TrueType files with outlines, which can be
/// embedded as they are.
fn scan(dirs: &[PathBuf]) -> Vec<Entry> {
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_ttf = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("ttf"));
            let small = entry.metadata().is_ok_and(|m| m.len() <= MAX_FONT_BYTES);
            if !is_ttf || !small {
                continue;
            }
            let Ok(data) = std::fs::read(&path) else {
                continue;
            };
            if !is_plain_truetype(&data) {
                continue;
            }
            let Ok(face) = ttf_parser::Face::parse(&data, 0) else {
                continue;
            };
            if face.tables().glyf.is_none() {
                continue;
            }
            let Some(family) = family_of(&face) else {
                continue;
            };
            found.push(Entry {
                family,
                bold: face.is_bold(),
                italic: face.is_italic() || face.is_oblique(),
                weight: face.weight().to_number(),
                path,
            });
        }
    }
    found
}

fn installed() -> &'static Vec<Entry> {
    static CATALOG: OnceLock<Vec<Entry>> = OnceLock::new();
    CATALOG.get_or_init(|| scan(&default_dirs()))
}

/// [`FontCatalog`] over the fonts installed in Windows.
#[derive(Debug, Clone, Copy, Default)]
pub struct InstalledFonts;

impl FontCatalog for InstalledFonts {
    fn families(&self) -> Result<Vec<String>, AppError> {
        let mut families: Vec<String> = installed().iter().map(|e| e.family.clone()).collect();
        families.sort_by_key(|f| f.to_lowercase());
        families.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        Ok(families)
    }
}

/// Picks the file of a family that is closest to the wanted style.
fn pick<'a>(entries: &'a [Entry], family: &str, bold: bool, italic: bool) -> Option<&'a Entry> {
    entries
        .iter()
        .filter(|e| e.family.eq_ignore_ascii_case(family))
        .min_by_key(|e| {
            let style_gap = u32::from(e.bold != bold) * 2 + u32::from(e.italic != italic);
            let weight_gap = u32::from(e.weight.abs_diff(if bold { 700 } else { 400 }));
            (style_gap, weight_gap)
        })
}

/// A TrueType font ready to be written into a document.
#[derive(Debug, Clone)]
pub struct Embedded {
    data: Arc<Vec<u8>>,
    name: String,
    /// Advance widths in 1/1000 em for the WinAnsi codes 32 to 255; 0 marks a code the font lacks.
    widths: Vec<u16>,
    ascent: f32,
    descent: f32,
    cap_height: f32,
    bbox: [f32; 4],
    italic_angle: f32,
    italic: bool,
}

/// The WinAnsi character of each code from 32 to 255.
pub(crate) fn winansi_chars() -> &'static Vec<char> {
    static TABLE: OnceLock<Vec<char>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = vec!['\0'; 224];
        for c in (0x20u32..=0x2200).filter_map(char::from_u32) {
            if let Some(code) = StandardFont::encode_char(c)
                && code >= 32
                && table[usize::from(code) - 32] == '\0'
            {
                table[usize::from(code) - 32] = c;
            }
        }
        table
    })
}

fn load_embedded(entry: &Entry) -> Result<Embedded, AppError> {
    let data = std::fs::read(&entry.path).map_err(|e| AppError::from_io("read the font", &e))?;
    let bad = || {
        AppError::new(
            ErrorCode::OptionInvalid,
            format!("The font \"{}\" could not be read.", entry.family),
        )
    };
    let face = ttf_parser::Face::parse(&data, 0).map_err(|_| bad())?;
    if let Some(permissions) = face.permissions()
        && permissions == ttf_parser::Permissions::Restricted
    {
        return Err(AppError::new(
            ErrorCode::OptionInvalid,
            format!(
                "The font \"{}\" does not allow embedding in documents. Choose another font.",
                entry.family
            ),
        ));
    }
    let units = f32::from(face.units_per_em().max(1));
    let scale = |v: f32| v * 1000.0 / units;
    let widths: Vec<u16> = winansi_chars()
        .iter()
        .map(|c| {
            if *c == '\0' {
                return 0;
            }
            face.glyph_index(*c)
                .and_then(|g| face.glyph_hor_advance(g))
                .map_or(0, |w| scale(f32::from(w)).round().max(1.0) as u16)
        })
        .collect();
    let bbox = face.global_bounding_box();
    let name = face
        .names()
        .into_iter()
        .find(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME && n.is_unicode())
        .and_then(|n| n.to_string())
        .unwrap_or_else(|| entry.family.clone())
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>();
    let ascent = scale(f32::from(face.ascender()));
    Ok(Embedded {
        name: if name.is_empty() {
            "EmbeddedFont".to_owned()
        } else {
            name
        },
        widths,
        ascent,
        descent: scale(f32::from(face.descender())),
        cap_height: face
            .capital_height()
            .map_or(ascent * 0.9, |h| scale(f32::from(h))),
        bbox: [
            scale(f32::from(bbox.x_min)),
            scale(f32::from(bbox.y_min)),
            scale(f32::from(bbox.x_max)),
            scale(f32::from(bbox.y_max)),
        ],
        italic_angle: face.italic_angle(),
        italic: entry.italic,
        data: Arc::new(data),
    })
}

/// A font text can be set in.
#[derive(Debug, Clone)]
pub enum Face {
    /// One of the 12 standard fonts.
    Standard(StandardFont),
    /// An installed font that is embedded.
    Embedded(Embedded),
}

impl Face {
    /// The font a text style asks for. An installed font that cannot be found or embedded is an
    /// error, not a silent change of font.
    pub fn resolve(style: &TextStyle) -> Result<Self, AppError> {
        Self::resolve_in(style, installed())
    }

    fn resolve_in(style: &TextStyle, entries: &[Entry]) -> Result<Self, AppError> {
        let Some(family) = &style.installed_font else {
            return Ok(Self::Standard(StandardFont::new(
                style.font,
                style.bold,
                style.italic,
            )));
        };
        let entry = pick(entries, family, style.bold, style.italic).ok_or_else(|| {
            AppError::new(
                ErrorCode::OptionInvalid,
                format!(
                    "The font \"{family}\" is not installed, or is not a TrueType font that can be embedded."
                ),
            )
        })?;
        load_embedded(entry).map(Self::Embedded)
    }

    /// A key that is the same for the same font, so a document gets one font object for it.
    pub fn key(&self) -> String {
        match self {
            Self::Standard(f) => format!("std:{}", f.pdf_name()),
            Self::Embedded(e) => format!("emb:{}", e.name),
        }
    }

    /// The bytes of a text in this font, or the first character it cannot draw.
    pub fn encode(&self, text: &str) -> Result<Vec<u8>, char> {
        match self {
            Self::Standard(_) => StandardFont::encode(text),
            Self::Embedded(e) => text
                .chars()
                .map(|c| {
                    StandardFont::encode_char(c)
                        .filter(|code| *code >= 32 && e.widths[usize::from(*code) - 32] > 0)
                        .ok_or(c)
                })
                .collect(),
        }
    }

    /// The width of a text in points.
    pub fn width(&self, text: &str, size_pt: f32) -> f32 {
        match self {
            Self::Standard(f) => f.text_width(text, size_pt),
            Self::Embedded(e) => {
                let units: u32 = text
                    .chars()
                    .map(|c| {
                        StandardFont::encode_char(c)
                            .filter(|code| *code >= 32)
                            .map_or(500, |code| u32::from(e.widths[usize::from(code) - 32]))
                    })
                    .sum();
                units as f32 * size_pt / 1000.0
            }
        }
    }

    /// Height of a capital letter as a fraction of the size, to place text vertically.
    pub fn cap_height(&self) -> f32 {
        match self {
            Self::Standard(_) => 0.72,
            Self::Embedded(e) => (e.cap_height / 1000.0).clamp(0.4, 1.0),
        }
    }

    /// Distance from the baseline to the top of the line as a fraction of the size.
    pub fn ascent(&self) -> f32 {
        match self {
            Self::Standard(_) => 0.8,
            Self::Embedded(e) => (e.ascent / 1000.0).clamp(0.5, 1.2),
        }
    }

    /// The name of the font as the PDF knows it.
    pub fn pdf_name(&self) -> String {
        match self {
            Self::Standard(f) => f.pdf_name().to_owned(),
            Self::Embedded(e) => e.name.clone(),
        }
    }

    /// Writes the font object into the document and returns it.
    pub fn write(&self, doc: &mut Document) -> ObjectId {
        match self {
            Self::Standard(f) => {
                let mut dict = Dictionary::new();
                dict.set("Type", Object::Name(b"Font".to_vec()));
                dict.set("Subtype", Object::Name(b"Type1".to_vec()));
                dict.set("BaseFont", Object::Name(f.pdf_name().as_bytes().to_vec()));
                dict.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
                doc.add_object(Object::Dictionary(dict))
            }
            Self::Embedded(e) => e.write(doc),
        }
    }
}

impl Embedded {
    fn write(&self, doc: &mut Document) -> ObjectId {
        let mut file_dict = Dictionary::new();
        file_dict.set("Length1", Object::Integer(self.data.len() as i64));
        let mut file = Stream::new(file_dict, self.data.as_ref().clone());
        // A failed compression leaves the stream as it is, which is still a valid font.
        let _ = file.compress();
        let file_id = doc.add_object(file);

        let mut descriptor = Dictionary::new();
        descriptor.set("Type", Object::Name(b"FontDescriptor".to_vec()));
        descriptor.set("FontName", Object::Name(self.name.as_bytes().to_vec()));
        descriptor.set(
            "Flags",
            Object::Integer(if self.italic { 32 + 64 } else { 32 }),
        );
        descriptor.set(
            "FontBBox",
            Object::Array(self.bbox.iter().map(|v| Object::Real(*v)).collect()),
        );
        descriptor.set("ItalicAngle", Object::Real(self.italic_angle));
        descriptor.set("Ascent", Object::Real(self.ascent));
        descriptor.set("Descent", Object::Real(self.descent));
        descriptor.set("CapHeight", Object::Real(self.cap_height));
        descriptor.set("StemV", Object::Integer(80));
        descriptor.set("FontFile2", Object::Reference(file_id));
        let descriptor_id = doc.add_object(Object::Dictionary(descriptor));

        let mut font = Dictionary::new();
        font.set("Type", Object::Name(b"Font".to_vec()));
        font.set("Subtype", Object::Name(b"TrueType".to_vec()));
        font.set("BaseFont", Object::Name(self.name.as_bytes().to_vec()));
        font.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
        font.set("FirstChar", Object::Integer(32));
        font.set("LastChar", Object::Integer(255));
        font.set(
            "Widths",
            Object::Array(
                self.widths
                    .iter()
                    .map(|w| Object::Integer(i64::from(*w)))
                    .collect(),
            ),
        );
        font.set("FontDescriptor", Object::Reference(descriptor_id));
        doc.add_object(Object::Dictionary(font))
    }
}

/// A font file that lives at `path`, for tests that bring their own.
#[cfg(test)]
pub(crate) fn entry_for_test(family: &str, path: &std::path::Path) -> Vec<Entry> {
    vec![Entry {
        family: family.to_owned(),
        bold: false,
        italic: false,
        weight: 400,
        path: path.to_path_buf(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::fonts::FontFamily;

    fn arial() -> Option<PathBuf> {
        let windir = std::env::var_os("WINDIR")?;
        let path = PathBuf::from(windir).join("Fonts").join("arial.ttf");
        path.is_file().then_some(path)
    }

    #[test]
    fn standard_fonts_are_the_default_and_measure_text() {
        let face = Face::resolve(&TextStyle::default()).unwrap();
        assert!(matches!(face, Face::Standard(_)));
        // "Hello" in Helvetica at 10 points: (722 + 556 + 222 + 222 + 556) / 100.
        assert!((face.width("Hello", 10.0) - 22.78).abs() < 0.01);
        assert_eq!(face.encode("Hé").unwrap(), vec![72, 0xE9]);
        assert_eq!(face.encode("a\u{4e2d}").unwrap_err(), '\u{4e2d}');
        let bold = Face::resolve(&TextStyle {
            font: FontFamily::Times,
            bold: true,
            ..TextStyle::default()
        })
        .unwrap();
        assert_eq!(bold.pdf_name(), "Times-Bold");
    }

    #[test]
    fn an_installed_font_is_found_measured_and_written_with_its_file() {
        let Some(path) = arial() else { return };
        let entries = entry_for_test("Arial", &path);
        let style = TextStyle {
            installed_font: Some("arial".to_owned()),
            ..TextStyle::default()
        };
        let face = Face::resolve_in(&style, &entries).unwrap();
        assert!(matches!(face, Face::Embedded(_)));
        // Arial and Helvetica share their widths.
        assert!(
            (face.width("Hello", 10.0) - 22.78).abs() < 0.2,
            "{}",
            face.width("Hello", 10.0)
        );
        assert_eq!(face.encode("Hé").unwrap(), vec![72, 0xE9]);
        let mut doc = Document::with_version("1.7");
        let id = face.write(&mut doc);
        let font = doc.get_dictionary(id).unwrap();
        assert_eq!(
            font.get(b"Subtype").unwrap().as_name().unwrap(),
            b"TrueType"
        );
        let descriptor = doc
            .get_dictionary(font.get(b"FontDescriptor").unwrap().as_reference().unwrap())
            .unwrap();
        let file = doc
            .get_object(
                descriptor
                    .get(b"FontFile2")
                    .unwrap()
                    .as_reference()
                    .unwrap(),
            )
            .unwrap();
        assert!(file.as_stream().unwrap().content.len() > 1000);
    }

    #[test]
    fn a_font_that_is_not_installed_is_an_error_not_a_substitute() {
        let style = TextStyle {
            installed_font: Some("No Such Font".to_owned()),
            ..TextStyle::default()
        };
        let err = Face::resolve_in(&style, &[]).unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
        assert!(err.message.contains("No Such Font"));
    }

    #[test]
    fn the_catalog_lists_families_once_and_sorted() {
        let families = InstalledFonts.families().unwrap();
        let mut sorted = families.clone();
        sorted.sort_by_key(|f| f.to_lowercase());
        assert_eq!(families, sorted);
        if arial().is_some() {
            assert!(families.iter().any(|f| f == "Arial"));
        }
    }

    #[test]
    fn the_closest_style_wins() {
        let entry = |bold, italic, weight| Entry {
            family: "Test".to_owned(),
            bold,
            italic,
            weight,
            path: PathBuf::new(),
        };
        let all = [
            entry(false, false, 400),
            entry(true, false, 700),
            entry(false, true, 400),
        ];
        assert!(pick(&all, "test", true, false).unwrap().bold);
        assert!(pick(&all, "TEST", false, true).unwrap().italic);
        assert!(!pick(&all, "test", false, false).unwrap().bold);
        assert!(pick(&all, "other", false, false).is_none());
    }
}
