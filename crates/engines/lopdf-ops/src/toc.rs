//! A generated table of contents: a few text pages listing files and their start pages.

use application::AppError;
use application::ports::{TocBuilder, TocLine};
use lopdf::{Document, Object, Stream, dictionary};
use std::path::Path;

const MARGIN: f32 = 54.0;
const LINE_HEIGHT: f32 = 16.0;
const FONT_SIZE: f32 = 11.0;
const HEADING_SIZE: f32 = 18.0;
/// Text lines that fit on one page, counting the heading as two lines on the first page.
const LINES_PER_PAGE: usize = 35;
const FIRST_PAGE_LINES: usize = LINES_PER_PAGE - 2;
/// Pages are never made smaller than this, so the fixed layout always fits.
const MIN_WIDTH: f32 = 300.0;
const MIN_HEIGHT: f32 = MARGIN * 2.0 + LINES_PER_PAGE as f32 * LINE_HEIGHT;

/// [`TocBuilder`] over lopdf.
#[derive(Debug, Clone, Copy, Default)]
pub struct LopdfToc;

/// Encodes text for the standard Helvetica font (WinAnsi): Latin-1 characters pass through and
/// anything else becomes `?`. Parentheses and backslashes are escaped for a PDF string.
fn encode(text: &str, max_chars: usize) -> Vec<u8> {
    let mut chars: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
    if chars.len() > max_chars {
        chars.truncate(max_chars.saturating_sub(3));
        chars.extend("...".chars());
    }
    let mut out = Vec::new();
    for c in chars {
        let byte = u8::try_from(u32::from(c)).unwrap_or(b'?');
        if matches!(byte, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(byte);
    }
    out
}

fn page_content(lines: &[TocLine], width: f32, height: f32, heading: bool) -> Vec<u8> {
    let mut content: Vec<u8> = Vec::new();
    let mut y = height - MARGIN;
    if heading {
        content.extend_from_slice(
            format!("BT /F2 {HEADING_SIZE} Tf {MARGIN} {y:.1} Td (Contents) Tj ET\n").as_bytes(),
        );
        y -= LINE_HEIGHT * 2.0;
    }
    let max_chars = (((width - 2.0 * MARGIN - 48.0) / (FONT_SIZE * 0.5)) as usize).max(8);
    for line in lines {
        y -= LINE_HEIGHT;
        content.extend_from_slice(format!("BT /F1 {FONT_SIZE} Tf {MARGIN} {y:.1} Td (").as_bytes());
        content.extend(encode(&line.title, max_chars));
        content.extend_from_slice(b") Tj ET\n");
        let number = line.page_number.to_string();
        let x = width - MARGIN - number.len() as f32 * FONT_SIZE * 0.56;
        content.extend_from_slice(
            format!("BT /F1 {FONT_SIZE} Tf {x:.1} {y:.1} Td ({number}) Tj ET\n").as_bytes(),
        );
    }
    content
}

impl TocBuilder for LopdfToc {
    fn page_count(&self, lines: usize) -> u32 {
        let pages = if lines <= FIRST_PAGE_LINES {
            1
        } else {
            1 + (lines - FIRST_PAGE_LINES).div_ceil(LINES_PER_PAGE)
        };
        u32::try_from(pages).unwrap_or(u32::MAX)
    }

    fn build(
        &self,
        lines: &[TocLine],
        width_pt: f32,
        height_pt: f32,
        dest: &Path,
    ) -> Result<(), AppError> {
        let (width, height) = (width_pt.max(MIN_WIDTH), height_pt.max(MIN_HEIGHT));
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let regular = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let bold = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-Bold",
            "Encoding" => "WinAnsiEncoding",
        });
        let resources = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => regular, "F2" => bold },
        });

        // Split the lines over pages exactly as `page_count` promises.
        let mut chunks: Vec<&[TocLine]> = Vec::new();
        let (first, mut rest) = lines.split_at(lines.len().min(FIRST_PAGE_LINES));
        chunks.push(first);
        while !rest.is_empty() {
            let (chunk, tail) = rest.split_at(rest.len().min(LINES_PER_PAGE));
            chunks.push(chunk);
            rest = tail;
        }

        let mut kids: Vec<Object> = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            let content = doc.add_object(Stream::new(
                dictionary! {},
                page_content(chunk, width, height, i == 0),
            ));
            let page = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), width.into(), height.into()],
                "Resources" => resources,
                "Contents" => content,
            });
            kids.push(page.into());
        }
        let count = i64::try_from(kids.len()).unwrap_or(0);
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => kids, "Count" => count,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        doc.save(dest)
            .map_err(|e| AppError::from_io("write the table of contents", &e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(n: usize) -> Vec<TocLine> {
        (0..n)
            .map(|i| TocLine {
                title: format!("File {i}"),
                page_number: u32::try_from(i + 1).unwrap(),
            })
            .collect()
    }

    #[test]
    fn page_counts_follow_the_layout() {
        let toc = LopdfToc;
        assert_eq!(toc.page_count(0), 1);
        assert_eq!(toc.page_count(FIRST_PAGE_LINES), 1);
        assert_eq!(toc.page_count(FIRST_PAGE_LINES + 1), 2);
        assert_eq!(toc.page_count(FIRST_PAGE_LINES + LINES_PER_PAGE), 2);
        assert_eq!(toc.page_count(FIRST_PAGE_LINES + LINES_PER_PAGE + 1), 3);
    }

    #[test]
    fn built_documents_have_exactly_the_promised_pages() {
        let dir = tempfile::tempdir().unwrap();
        for n in [1, FIRST_PAGE_LINES, FIRST_PAGE_LINES + 1, 100] {
            let dest = dir.path().join(format!("toc-{n}.pdf"));
            LopdfToc.build(&lines(n), 612.0, 792.0, &dest).unwrap();
            let doc = Document::load(&dest).unwrap();
            assert_eq!(
                doc.get_pages().len() as u32,
                LopdfToc.page_count(n),
                "{n} lines"
            );
        }
    }

    #[test]
    fn titles_are_escaped_truncated_and_latin1() {
        assert_eq!(encode("a(b)c\\", 40), b"a\\(b\\)c\\\\".to_vec());
        assert_eq!(encode("caf\u{e9}", 40), vec![b'c', b'a', b'f', 0xE9]);
        assert_eq!(encode("\u{4e2d}", 40), b"?".to_vec());
        assert_eq!(encode(&"x".repeat(100), 10), b"xxxxxxx...".to_vec());
    }

    #[test]
    fn small_pages_are_enlarged_so_the_layout_fits() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("small.pdf");
        LopdfToc.build(&lines(3), 100.0, 100.0, &dest).unwrap();
        let doc = Document::load(&dest).unwrap();
        let (_, id) = doc.get_pages().into_iter().next().unwrap();
        let [_, _, w, h] = crate::boxes::visible_box(&doc, id);
        assert!(w >= MIN_WIDTH && h >= MIN_HEIGHT);
    }
}
