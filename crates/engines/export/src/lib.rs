//! Writes the editable formats the PDF export tools produce: Word (`.docx`), PowerPoint
//! (`.pptx`) and Excel (`.xlsx`). The files are Open XML packages written directly with a zip
//! writer, so no Office program and no copyleft library is needed.
#![forbid(unsafe_code)]

mod docx;
mod pptx;
mod xlsx;

use application::AppError;
use application::ports::{DocumentWriter, PictureSource, RenderedImage, SheetData};
use domain::ErrorCode;
use domain::layout::PageLayout;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

pub use xlsx::cell_number;

/// [`DocumentWriter`] over hand-written Open XML.
#[derive(Debug, Clone, Copy, Default)]
pub struct OfficeWriter;

impl DocumentWriter for OfficeWriter {
    fn write_docx(
        &self,
        pages: &[PageLayout],
        title: &str,
        exact: bool,
        pictures: &mut PictureSource<'_>,
        dest: &Path,
    ) -> Result<(), AppError> {
        docx::write(pages, title, exact, pictures, dest)
    }

    fn write_pptx(
        &self,
        pages: &[PageLayout],
        title: &str,
        pictures: &mut PictureSource<'_>,
        dest: &Path,
    ) -> Result<(), AppError> {
        pptx::write(pages, title, pictures, dest)
    }

    fn write_image_pptx(
        &self,
        sizes: &[(f32, f32)],
        title: &str,
        image: &mut dyn FnMut(usize) -> Result<RenderedImage, AppError>,
        dest: &Path,
    ) -> Result<(), AppError> {
        pptx::write_images(sizes, title, image, dest)
    }

    fn write_xlsx(&self, sheets: &[SheetData], dest: &Path) -> Result<(), AppError> {
        xlsx::write(sheets, dest)
    }

    fn verify(&self, path: &Path, parts: u32) -> Result<(), AppError> {
        verify_package(path, parts)
    }
}

fn bad_output(detail: &str) -> AppError {
    AppError::new(
        ErrorCode::OutputVerifyFailed,
        format!("The document that was written could not be checked: {detail}."),
    )
}

/// Opens a package and checks for its content types, its main part and, for presentations and
/// workbooks, the number of slides or sheets.
fn verify_package(path: &Path, parts: u32) -> Result<(), AppError> {
    let file = File::open(path).map_err(|e| AppError::from_io("check the document", &e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|_| bad_output("it is not a valid package"))?;
    let mut names: Vec<String> = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|_| bad_output("a part of it cannot be read"))?;
        names.push(entry.name().map(|n| n.into_owned()).unwrap_or_default());
    }
    let has = |n: &str| names.iter().any(|x| x == n);
    if !has("[Content_Types].xml") {
        return Err(bad_output("its content types are missing"));
    }
    let counted = if has("word/document.xml") {
        None
    } else if has("ppt/presentation.xml") {
        Some("ppt/slides/slide")
    } else if has("xl/workbook.xml") {
        Some("xl/worksheets/sheet")
    } else {
        return Err(bad_output("its main part is missing"));
    };
    if let (Some(prefix), true) = (counted, parts > 0) {
        let found = names
            .iter()
            .filter(|n| n.starts_with(prefix) && n.ends_with(".xml"))
            .count();
        if found != parts as usize {
            return Err(bad_output(&format!(
                "it holds {found} pages instead of {parts}"
            )));
        }
    }
    Ok(())
}

/// A zip file being written.
pub(crate) struct Package {
    zip: ZipWriter<BufWriter<File>>,
    options: SimpleFileOptions,
}

fn zip_failed() -> AppError {
    AppError::internal("writing the document")
}

impl Package {
    /// Starts a package at `dest`.
    pub(crate) fn create(dest: &Path) -> Result<Self, AppError> {
        let file = File::create(dest).map_err(|e| AppError::from_io("write the document", &e))?;
        Ok(Self {
            zip: ZipWriter::new(BufWriter::new(file)),
            options: SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        })
    }

    /// Adds a part.
    pub(crate) fn add(&mut self, name: &str, data: &[u8]) -> Result<(), AppError> {
        self.zip
            .start_file(name, self.options)
            .map_err(|_| zip_failed())?;
        self.zip
            .write_all(data)
            .map_err(|e| AppError::from_io("write the document", &e))
    }

    /// Adds a text part.
    pub(crate) fn add_text(&mut self, name: &str, text: &str) -> Result<(), AppError> {
        self.add(name, text.as_bytes())
    }

    /// Writes the end of the package and flushes it to disk.
    pub(crate) fn finish(self) -> Result<(), AppError> {
        let mut out = self.zip.finish().map_err(|_| zip_failed())?;
        out.flush()
            .map_err(|e| AppError::from_io("write the document", &e))
    }
}

/// Text made safe for XML: markup characters escaped, characters XML cannot hold dropped.
pub(crate) fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// Points as English Metric Units, which drawings use.
pub(crate) fn emu(points: f32) -> i64 {
    (f64::from(points) * 12_700.0).round() as i64
}

/// Points as twentieths of a point, which Word uses.
pub(crate) fn twips(points: f32) -> i64 {
    (f64::from(points) * 20.0).round() as i64
}

/// True for the error that must stop the run instead of being worked around.
pub(crate) fn is_cancel(error: &AppError) -> bool {
    error.code == ErrorCode::JobCancelled
}

const XML_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

/// The XML declaration every part starts with.
pub(crate) fn xml_head() -> &'static str {
    XML_HEAD
}

/// The relationships part for the package root: the main part and the title.
pub(crate) fn root_rels(main_type: &str, main_target: &str) -> String {
    format!(
        "{XML_HEAD}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{main_type}\" Target=\"{main_target}\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
</Relationships>"
    )
}

/// The core properties part: the title.
pub(crate) fn core_properties(title: &str) -> String {
    format!(
        "{XML_HEAD}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
<dc:title>{}</dc:title><dc:creator>Quirewise</dc:creator></cp:coreProperties>",
        xml_escape(title)
    )
}

#[cfg(test)]
pub(crate) mod testing {
    //! Helpers for reading back what the writers made.

    use std::io::Read;
    use std::path::Path;

    /// Names of the parts of a package.
    pub fn parts(path: &Path) -> Vec<String> {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().unwrap().into_owned())
            .collect()
    }

    /// The text of one part.
    pub fn part(path: &Path, name: &str) -> String {
        let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        let mut text = String::new();
        archive
            .by_name(name)
            .unwrap_or_else(|_| panic!("part {name} is missing"))
            .read_to_string(&mut text)
            .unwrap();
        text
    }

    /// Checks that every XML part of the package is well formed.
    pub fn assert_well_formed(path: &Path) {
        for name in parts(path) {
            if name.ends_with(".xml") || name.ends_with(".rels") {
                let text = part(path, &name);
                roxmltree::Document::parse(&text)
                    .unwrap_or_else(|e| panic!("{name} is not well formed: {e}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escape_handles_markup_and_drops_control_characters() {
        assert_eq!(
            xml_escape("a < b & \"c\" > d"),
            "a &lt; b &amp; &quot;c&quot; &gt; d"
        );
        assert_eq!(xml_escape("x\u{0}y\u{8}z\tw"), "xyz\tw");
        assert_eq!(xml_escape("caf\u{e9} \u{4e2d}"), "caf\u{e9} \u{4e2d}");
    }

    #[test]
    fn units_convert_points() {
        assert_eq!(emu(72.0), 914_400);
        assert_eq!(twips(72.0), 1_440);
    }
}
