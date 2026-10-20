//! Documents of flowing text, such as a summary or a translation without the original layout:
//! a title, headings, paragraphs and bullets over as many pages as needed.

use application::AppError;
use application::ports::{DocBlock, TextDocWriter};
use lopdf::{Dictionary, Document, Object, Stream, dictionary};
use std::path::Path;

use crate::fonts::Face;
use crate::pageres::num;
use crate::paint::wrap;
use crate::stamp::literal;

const MARGIN: f32 = 56.0;
const BODY_PT: f32 = 11.0;
const LINE: f32 = 1.35;
/// Space after a paragraph or bullet, in points.
const GAP: f32 = 6.0;
/// Indent of bullets and the width taken by the bullet sign, in points.
const BULLET: f32 = 14.0;
/// Pages are never made smaller than this.
const MIN_SIDE: f32 = 200.0;

/// [`TextDocWriter`] over lopdf.
#[derive(Debug, Clone, Copy, Default)]
pub struct LopdfTextDoc;

struct Line {
    text: String,
    size: f32,
    bold: bool,
    x: f32,
    /// Space left above the line.
    before: f32,
    /// A bullet sign is drawn in the margin of this line.
    bullet: bool,
}

fn lay_out(blocks: &[DocBlock], regular: &Face, bold: &Face, width: f32) -> Vec<Line> {
    let mut lines = Vec::new();
    let full = width - 2.0 * MARGIN;
    for block in blocks {
        let (text, size, is_bold, indent, before) = match block {
            DocBlock::Title(t) => (t, 20.0, true, 0.0, 0.0),
            DocBlock::Heading(t) => (t, 14.0, true, 0.0, 10.0),
            DocBlock::Paragraph(t) => (t, BODY_PT, false, 0.0, GAP),
            DocBlock::Bullet(t) => (t, BODY_PT, false, BULLET, GAP / 2.0),
        };
        let face = if is_bold { bold } else { regular };
        let is_bullet = matches!(block, DocBlock::Bullet(_));
        for (i, line) in wrap(face, text, size, full - indent)
            .into_iter()
            .enumerate()
        {
            let first = i == 0;
            lines.push(Line {
                text: line,
                size,
                bold: is_bold,
                x: MARGIN + indent,
                before: if first { before } else { 0.0 },
                bullet: first && is_bullet,
            });
        }
    }
    lines
}

impl TextDocWriter for LopdfTextDoc {
    fn write(
        &self,
        blocks: &[DocBlock],
        title: &str,
        width_pt: f32,
        height_pt: f32,
        dest: &Path,
    ) -> Result<(), AppError> {
        let (width, height) = (width_pt.max(MIN_SIDE), height_pt.max(MIN_SIDE));
        let all: String = blocks
            .iter()
            .map(|b| match b {
                DocBlock::Title(t)
                | DocBlock::Heading(t)
                | DocBlock::Paragraph(t)
                | DocBlock::Bullet(t) => t.as_str(),
            })
            .chain(std::iter::once("\u{2022}"))
            .collect::<Vec<_>>()
            .join("\n");
        let regular = Face::covering(&all, false, false)?;
        let bold = Face::covering(&all, true, false).unwrap_or_else(|_| regular.clone());
        let lines = lay_out(blocks, &regular, &bold, width);

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let regular_id = regular.write(&mut doc);
        let bold_id = bold.write(&mut doc);
        let resources = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => regular_id, "F2" => bold_id },
        });

        // Fill pages from the top; a line that does not fit starts the next page.
        let mut pages: Vec<String> = vec![String::new()];
        let mut y = height - MARGIN;
        let mut first_on_page = true;
        for line in &lines {
            let advance = line.size * LINE;
            let needed = advance + if first_on_page { 0.0 } else { line.before };
            if y - needed < MARGIN && !first_on_page {
                pages.push(String::new());
                y = height - MARGIN;
                first_on_page = true;
            }
            if !first_on_page {
                y -= line.before;
            }
            y -= advance;
            first_on_page = false;
            if line.text.is_empty() && !line.bullet {
                continue;
            }
            let face = if line.bold { &bold } else { &regular };
            let font = if line.bold { "F2" } else { "F1" };
            let bytes = face.encode(&line.text).map_err(|c| {
                AppError::new(
                    domain::ErrorCode::OptionInvalid,
                    format!("The character \"{c}\" cannot be drawn."),
                )
            })?;
            if let Some(page) = pages.last_mut() {
                if line.bullet {
                    let sign = regular.encode("•").unwrap_or_default();
                    page.push_str(&format!(
                        "BT
/F1 {} Tf
{} {} Td
{} Tj
ET
",
                        num(line.size),
                        num(MARGIN),
                        num(y + line.size * (LINE - 1.0) * 0.5),
                        literal(&sign)
                    ));
                }
                page.push_str(&format!(
                    "BT\n/{font} {} Tf\n{} {} Td\n{} Tj\nET\n",
                    num(line.size),
                    num(line.x),
                    num(y + line.size * (LINE - 1.0) * 0.5),
                    literal(&bytes)
                ));
            }
        }

        let mut kids: Vec<Object> = Vec::new();
        for content in pages {
            let content_id = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
            let page = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), width.into(), height.into()],
                "Resources" => resources,
                "Contents" => content_id,
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
        if !title.trim().is_empty() {
            let info = doc.add_object(dictionary! {
                "Title" => Object::string_literal(title.replace(['\r', '\n'], " ")),
                "Producer" => Object::string_literal("Quirewise"),
            });
            doc.trailer.set("Info", info);
        }
        doc.save(dest)
            .map_err(|e| AppError::from_io("write the document", &e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(paragraphs: usize) -> Vec<DocBlock> {
        let mut out = vec![
            DocBlock::Title("Summary".into()),
            DocBlock::Heading("Part one".into()),
        ];
        for i in 0..paragraphs {
            out.push(DocBlock::Paragraph(format!(
                "Paragraph {i}. {}",
                "The quick brown fox jumps over the lazy dog. ".repeat(6)
            )));
            out.push(DocBlock::Bullet(format!("Point {i}")));
        }
        out
    }

    #[test]
    fn text_flows_over_pages_and_every_line_is_inside_the_margins() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("t.pdf");
        LopdfTextDoc
            .write(&blocks(40), "Summary", 595.0, 842.0, &dest)
            .unwrap();
        let doc = Document::load(&dest).unwrap();
        let pages = doc.get_pages();
        assert!(pages.len() > 1, "{}", pages.len());
        for (_, id) in pages {
            let content = String::from_utf8_lossy(&doc.get_page_content(id)).into_owned();
            for line in content.lines().filter(|l| l.ends_with(" Td")) {
                let mut it = line.split_whitespace();
                let _x: f32 = it.next().unwrap().parse().unwrap();
                let y: f32 = it.next().unwrap().parse().unwrap();
                assert!((MARGIN - 2.0..=842.0 - MARGIN + 2.0).contains(&y), "{line}");
            }
        }
        assert_eq!(
            doc.trailer
                .get(b"Info")
                .ok()
                .and_then(|i| doc.get_dictionary(i.as_reference().unwrap()).ok())
                .and_then(|d| d.get(b"Title").ok())
                .and_then(|t| t.as_str().ok())
                .map(<[u8]>::to_vec),
            Some(b"Summary".to_vec())
        );
    }

    #[test]
    fn accented_latin_text_that_winansi_lacks_uses_an_installed_font() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("pl.pdf");
        let text = vec![DocBlock::Paragraph(
            "Zażółć gęślą jaźń. Şiş, čeština, łódź.".into(),
        )];
        match LopdfTextDoc.write(&text, "", 595.0, 842.0, &dest) {
            Ok(()) => {
                let bytes = std::fs::read(&dest).unwrap();
                assert!(
                    bytes.windows(8).any(|w| w == b"Identity"),
                    "an embedded composite font"
                );
                Document::load(&dest).unwrap();
            }
            // A machine without any TrueType font that has these letters.
            Err(e) => assert_eq!(e.code, domain::ErrorCode::ComponentMissing),
        }
    }

    #[test]
    fn an_empty_document_still_has_a_page() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("e.pdf");
        LopdfTextDoc.write(&[], "", 595.0, 842.0, &dest).unwrap();
        assert_eq!(Document::load(&dest).unwrap().get_pages().len(), 1);
    }
}
