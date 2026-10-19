//! Small Office documents for tests, written by hand so no Office program is needed to make them.

use std::io::{Cursor, Write};

const CONTENT_TYPES_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>";

const REL_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">";

fn zip_of(files: &[(&str, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(Cursor::new(&mut out));
        let options = zip::write::SimpleFileOptions::default();
        for (name, content) in files {
            writer.start_file(*name, options).expect("zip entry");
            writer.write_all(content.as_bytes()).expect("zip data");
        }
        writer.finish().expect("zip end");
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A Word document with one paragraph per entry, each on its own page.
pub fn docx_with_pages(pages: &[&str]) -> Vec<u8> {
    let mut body = String::new();
    for (i, text) in pages.iter().enumerate() {
        if i > 0 {
            body.push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>");
        }
        body.push_str(&format!(
            "<w:p><w:r><w:t>{}</w:t></w:r></w:p>",
            escape(text)
        ));
    }
    zip_of(&[
        (
            "[Content_Types].xml",
            format!(
                "{CONTENT_TYPES_HEAD}<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>"
            ),
        ),
        (
            "_rels/.rels",
            format!(
                "{REL_HEAD}<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>"
            ),
        ),
        (
            "word/document.xml",
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>"
            ),
        ),
    ])
}

/// An Excel workbook: each sheet has a name and rows of text cells.
pub fn xlsx_with_sheets(sheets: &[(&str, Vec<Vec<&str>>)]) -> Vec<u8> {
    let mut content_types = String::from(CONTENT_TYPES_HEAD);
    content_types.push_str("<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>");
    let mut workbook_sheets = String::new();
    let mut workbook_rels = String::from(REL_HEAD);
    let mut files: Vec<(String, String)> = Vec::new();
    for (i, (name, rows)) in sheets.iter().enumerate() {
        let n = i + 1;
        content_types.push_str(&format!(
            "<Override PartName=\"/xl/worksheets/sheet{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"
        ));
        workbook_sheets.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{n}\" r:id=\"rId{n}\"/>",
            escape(name)
        ));
        workbook_rels.push_str(&format!(
            "<Relationship Id=\"rId{n}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{n}.xml\"/>"
        ));
        let mut data = String::new();
        for (r, row) in rows.iter().enumerate() {
            data.push_str(&format!("<row r=\"{}\">", r + 1));
            for (c, text) in row.iter().enumerate() {
                let column = char::from(b'A' + u8::try_from(c % 26).unwrap_or(0));
                data.push_str(&format!(
                    "<c r=\"{column}{}\" t=\"inlineStr\"><is><t>{}</t></is></c>",
                    r + 1,
                    escape(text)
                ));
            }
            data.push_str("</row>");
        }
        files.push((
            format!("xl/worksheets/sheet{n}.xml"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>{data}</sheetData></worksheet>"
            ),
        ));
    }
    content_types.push_str("</Types>");
    workbook_rels.push_str("</Relationships>");
    let mut all: Vec<(&str, String)> = vec![
        ("[Content_Types].xml", content_types),
        (
            "_rels/.rels",
            format!(
                "{REL_HEAD}<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>"
            ),
        ),
        (
            "xl/workbook.xml",
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets>{workbook_sheets}</sheets></workbook>"
            ),
        ),
        ("xl/_rels/workbook.xml.rels", workbook_rels),
    ];
    for (name, content) in &files {
        all.push((name.as_str(), content.clone()));
    }
    zip_of(&all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(bytes: &[u8]) -> Vec<String> {
        let archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        archive
            .file_names()
            .filter_map(|n| n.ok().map(|n| n.into_owned()))
            .collect()
    }

    #[test]
    fn the_documents_are_zip_packages_with_their_parts() {
        let docx = docx_with_pages(&["a", "b"]);
        assert!(names(&docx).contains(&"word/document.xml".to_owned()));
        assert!(docx.starts_with(b"PK"));
        let xlsx = xlsx_with_sheets(&[("One", vec![vec!["x"]]), ("Two", vec![vec!["y"]])]);
        let parts = names(&xlsx);
        assert!(parts.contains(&"xl/worksheets/sheet2.xml".to_owned()));
    }
}
