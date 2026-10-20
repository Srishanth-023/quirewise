//! Installed TrueType fonts embedded as composite fonts with glyph codes (Identity-H), so text in
//! any script an installed font covers can be drawn: Latin with all its accents, Cyrillic, Greek,
//! Vietnamese and, with a font that has them, Korean. There is no shaping, so scripts that need
//! it (Arabic, Hebrew, Indic, Thai) are not drawn.
//!
//! The whole font is embedded and a map from glyphs back to characters is written, so the text
//! can be searched and copied.

use application::AppError;
use domain::ErrorCode;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use crate::fonts::{Entry, installed, pick};

/// Families tried first, in order: they carry the widest ranges and exist on a Windows install.
const PREFERRED: [&str; 12] = [
    "Segoe UI",
    "Arial",
    "Calibri",
    "Times New Roman",
    "Tahoma",
    "Verdana",
    "Malgun Gothic",
    "Microsoft Sans Serif",
    "Arial Unicode MS",
    "Noto Sans",
    "DejaVu Sans",
    "Liberation Sans",
];
/// A text is drawn when at least this share of its characters has a glyph.
const MIN_COVERAGE: f64 = 0.99;
/// Most font files read when the preferred families do not cover a text.
const MAX_SCANNED: usize = 400;
/// Most pairs in one block of the glyph to character map.
const CMAP_CHUNK: usize = 100;

/// An installed font that is embedded with glyph codes.
#[derive(Debug)]
pub(crate) struct UniFace {
    data: Arc<Vec<u8>>,
    name: String,
    glyphs: HashMap<char, u16>,
    /// Advance widths in 1/1000 em, by glyph.
    advances: Vec<u16>,
    ascent: f32,
    descent: f32,
    cap_height: f32,
    bbox: [f32; 4],
    italic_angle: f32,
    italic: bool,
}

fn char_map(face: &ttf_parser::Face<'_>) -> HashMap<char, u16> {
    let mut map = HashMap::new();
    let Some(cmap) = face.tables().cmap else {
        return map;
    };
    for sub in cmap.subtables {
        if !sub.is_unicode() {
            continue;
        }
        sub.codepoints(|cp| {
            if let (Some(c), Some(g)) = (char::from_u32(cp), sub.glyph_index(cp))
                && g.0 != 0
            {
                map.entry(c).or_insert(g.0);
            }
        });
    }
    map
}

fn counted(text: &str) -> HashSet<char> {
    text.chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect()
}

impl UniFace {
    /// Reads a font file for embedding.
    pub(crate) fn load(entry: &Entry) -> Result<Self, AppError> {
        let data =
            std::fs::read(&entry.path).map_err(|e| AppError::from_io("read the font", &e))?;
        let bad = || {
            AppError::new(
                ErrorCode::OptionInvalid,
                format!("The font \"{}\" could not be read.", entry.family),
            )
        };
        let face = ttf_parser::Face::parse(&data, 0).map_err(|_| bad())?;
        if face
            .permissions()
            .is_some_and(|p| p == ttf_parser::Permissions::Restricted)
        {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                format!(
                    "The font \"{}\" does not allow embedding in documents.",
                    entry.family
                ),
            ));
        }
        let units = f32::from(face.units_per_em().max(1));
        let scale = |v: f32| v * 1000.0 / units;
        let glyphs = char_map(&face);
        let advances: Vec<u16> = (0..face.number_of_glyphs())
            .map(|g| {
                face.glyph_hor_advance(ttf_parser::GlyphId(g))
                    .map_or(0, |w| scale(f32::from(w)).round().max(0.0) as u16)
            })
            .collect();
        let bbox = face.global_bounding_box();
        let name: String = face
            .names()
            .into_iter()
            .find(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME && n.is_unicode())
            .and_then(|n| n.to_string())
            .unwrap_or_else(|| entry.family.clone())
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        let ascent = scale(f32::from(face.ascender()));
        Ok(Self {
            name: if name.is_empty() {
                "UnicodeFont".to_owned()
            } else {
                name
            },
            glyphs,
            advances,
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

    /// The name of the font as the PDF knows it.
    pub(crate) fn pdf_name(&self) -> &str {
        &self.name
    }

    /// Share of the characters of `text` (spaces and control characters not counted) that have a
    /// glyph, from 0 to 1. An empty text is covered.
    pub(crate) fn coverage(&self, text: &str) -> f64 {
        let set = counted(text);
        if set.is_empty() {
            return 1.0;
        }
        let have = set.iter().filter(|c| self.glyphs.contains_key(c)).count();
        have as f64 / set.len() as f64
    }

    /// The glyph for a character: its own, or the one of `?`, or the empty glyph.
    fn glyph(&self, c: char) -> u16 {
        if let Some(g) = self.glyphs.get(&c) {
            return *g;
        }
        if c.is_whitespace() || c.is_control() {
            return self.glyphs.get(&' ').copied().unwrap_or(0);
        }
        self.glyphs.get(&'?').copied().unwrap_or(0)
    }

    /// The codes of a text: two bytes for each character.
    pub(crate) fn encode(&self, text: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len() * 2);
        for c in text.chars().filter(|c| !c.is_control() || *c == '\t') {
            out.extend_from_slice(&self.glyph(c).to_be_bytes());
        }
        out
    }

    /// The width of a text in points.
    pub(crate) fn width(&self, text: &str, size_pt: f32) -> f32 {
        let units: u32 = text
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .map(|c| {
                u32::from(
                    self.advances
                        .get(usize::from(self.glyph(c)))
                        .copied()
                        .unwrap_or(0),
                )
            })
            .sum();
        units as f32 * size_pt / 1000.0
    }

    /// Height of a capital letter as a fraction of the size.
    pub(crate) fn cap_height(&self) -> f32 {
        (self.cap_height / 1000.0).clamp(0.4, 1.0)
    }

    /// Distance from the baseline to the top of the line as a fraction of the size.
    pub(crate) fn ascent(&self) -> f32 {
        (self.ascent / 1000.0).clamp(0.5, 1.2)
    }

    /// The map from glyphs back to characters, so text can be copied and searched.
    fn to_unicode(&self, doc: &mut Document) -> ObjectId {
        let mut by_glyph: BTreeMap<u16, char> = BTreeMap::new();
        for (c, g) in &self.glyphs {
            by_glyph
                .entry(*g)
                .and_modify(|old| *old = (*old).min(*c))
                .or_insert(*c);
        }
        let pairs: Vec<(u16, char)> = by_glyph.into_iter().collect();
        let mut cmap = String::from(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
             /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
             /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
             1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        );
        for chunk in pairs.chunks(CMAP_CHUNK) {
            cmap.push_str(&format!("{} beginbfchar\n", chunk.len()));
            for (glyph, c) in chunk {
                let mut buf = [0u16; 2];
                let units: String = c
                    .encode_utf16(&mut buf)
                    .iter()
                    .map(|u| format!("{u:04X}"))
                    .collect();
                cmap.push_str(&format!("<{glyph:04X}> <{units}>\n"));
            }
            cmap.push_str("endbfchar\n");
        }
        cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        let mut stream = Stream::new(Dictionary::new(), cmap.into_bytes());
        let _ = stream.compress();
        doc.add_object(stream)
    }

    /// Writes the font into the document and returns the font object.
    pub(crate) fn write(&self, doc: &mut Document) -> ObjectId {
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

        let mut system_info = Dictionary::new();
        system_info.set("Registry", Object::string_literal("Adobe"));
        system_info.set("Ordering", Object::string_literal("Identity"));
        system_info.set("Supplement", Object::Integer(0));

        let mut cid = Dictionary::new();
        cid.set("Type", Object::Name(b"Font".to_vec()));
        cid.set("Subtype", Object::Name(b"CIDFontType2".to_vec()));
        cid.set("BaseFont", Object::Name(self.name.as_bytes().to_vec()));
        cid.set("CIDSystemInfo", Object::Dictionary(system_info));
        cid.set("FontDescriptor", Object::Reference(descriptor_id));
        cid.set("CIDToGIDMap", Object::Name(b"Identity".to_vec()));
        cid.set("DW", Object::Integer(1000));
        cid.set(
            "W",
            Object::Array(vec![
                Object::Integer(0),
                Object::Array(
                    self.advances
                        .iter()
                        .map(|w| Object::Integer(i64::from(*w)))
                        .collect(),
                ),
            ]),
        );
        let cid_id = doc.add_object(Object::Dictionary(cid));
        let to_unicode = self.to_unicode(doc);

        let mut font = Dictionary::new();
        font.set("Type", Object::Name(b"Font".to_vec()));
        font.set("Subtype", Object::Name(b"Type0".to_vec()));
        font.set("BaseFont", Object::Name(self.name.as_bytes().to_vec()));
        font.set("Encoding", Object::Name(b"Identity-H".to_vec()));
        font.set(
            "DescendantFonts",
            Object::Array(vec![Object::Reference(cid_id)]),
        );
        font.set("ToUnicode", Object::Reference(to_unicode));
        doc.add_object(Object::Dictionary(font))
    }
}

/// The family of an installed font that has the characters of `text`, best first: a preferred
/// family that covers all of it, else any family that covers it, else an error naming the
/// characters that cannot be drawn.
pub(crate) fn best_family(text: &str) -> Result<String, AppError> {
    best_family_in(installed(), text)
}

pub(crate) fn best_family_in(entries: &[Entry], text: &str) -> Result<String, AppError> {
    let mut best: Option<(f64, String)> = None;
    let mut consider = |entry: &Entry| -> Option<String> {
        let face = UniFace::load(entry).ok()?;
        let share = face.coverage(text);
        if share >= 1.0 {
            return Some(entry.family.clone());
        }
        if best.as_ref().is_none_or(|(b, _)| share > *b) {
            best = Some((share, entry.family.clone()));
        }
        None
    };
    for family in PREFERRED {
        if let Some(entry) = pick(entries, family, false, false)
            && let Some(found) = consider(entry)
        {
            return Ok(found);
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut scanned = 0;
    for entry in entries {
        if scanned >= MAX_SCANNED {
            break;
        }
        if !seen.insert(entry.family.to_lowercase()) {
            continue;
        }
        scanned += 1;
        if let Some(found) = consider(entry) {
            return Ok(found);
        }
    }
    match best {
        Some((share, family)) if share >= MIN_COVERAGE => Ok(family),
        _ => {
            let mut odd: Vec<char> = counted(text)
                .into_iter()
                .filter(|c| !c.is_ascii())
                .collect();
            odd.sort_unstable();
            let sample: String = odd.into_iter().take(8).collect();
            Err(AppError::new(
                ErrorCode::ComponentMissing,
                format!(
                    "No installed font can show every character of this text{}. Install a TrueType font for the language (for example Noto Sans), then try again.",
                    if sample.is_empty() {
                        String::new()
                    } else {
                        format!(" (such as {sample})")
                    }
                ),
            ))
        }
    }
}
