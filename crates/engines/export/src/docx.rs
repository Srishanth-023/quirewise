//! Word documents. Flowing mode writes headings, paragraphs, lists, tables and pictures that
//! reflow; exact mode places every block where it was on its page, in a frame.

use application::AppError;
use application::ports::{PictureBytes, PictureSource};
use domain::layout::{Block, PageLayout, PictureBox, Rect, Span, Table};
use std::path::Path;

use crate::{Package, core_properties, emu, is_cancel, root_rels, twips, xml_escape, xml_head};

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"";

/// Margin of a flowing document, in points.
const MARGIN: f32 = 72.0;
/// Page used when there are no pages.
const LETTER: (f32, f32) = (612.0, 792.0);

/// The size most characters have, rounded to half a point.
fn modal_size(pages: &[PageLayout]) -> f32 {
    let mut counts: Vec<(f32, usize)> = Vec::new();
    for page in pages {
        for block in &page.blocks {
            if let Block::Paragraph { spans, .. } | Block::ListItem { spans, .. } = block {
                for s in spans {
                    let size = (s.size * 2.0).round() / 2.0;
                    let n = s.text.chars().count();
                    match counts.iter_mut().find(|(c, _)| (*c - size).abs() < 0.01) {
                        Some((_, total)) => *total += n,
                        None => counts.push((size, n)),
                    }
                }
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map_or(11.0, |(s, _)| s)
        .clamp(6.0, 36.0)
}

struct Builder<'a, 'p> {
    body_size: f32,
    exact: bool,
    body: String,
    media: Vec<(String, Vec<u8>)>,
    image_rels: Vec<(String, String)>,
    drawing_id: u32,
    /// Start numbers of the ordered lists, one entry per numbering instance (id = index + 2).
    ordered: Vec<u32>,
    current_ordered: Option<u32>,
    pictures: &'a mut PictureSource<'p>,
}

impl Builder<'_, '_> {
    fn run(&self, span: &Span, heading: bool) -> String {
        if span.text.is_empty() {
            return String::new();
        }
        let mut props = String::new();
        if span.bold && !heading {
            props.push_str("<w:b/>");
        }
        if span.italic {
            props.push_str("<w:i/>");
        }
        let different = (span.size - self.body_size).abs() > 0.6;
        if (self.exact || heading || different) && span.size > 0.0 {
            let half = (span.size * 2.0).round().clamp(2.0, 3276.0) as u32;
            props.push_str(&format!(
                "<w:sz w:val=\"{half}\"/><w:szCs w:val=\"{half}\"/>"
            ));
        }
        let rpr = if props.is_empty() {
            String::new()
        } else {
            format!("<w:rPr>{props}</w:rPr>")
        };
        format!(
            "<w:r>{rpr}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
            xml_escape(&span.text)
        )
    }

    fn runs(&self, spans: &[Span], heading: bool) -> String {
        spans.iter().map(|s| self.run(s, heading)).collect()
    }

    fn marker_run(&self, text: &str, size: f32) -> String {
        self.run(
            &Span {
                text: format!("{text} "),
                bold: false,
                italic: false,
                size,
            },
            false,
        )
    }

    // ----- flowing -----------------------------------------------------------------------

    fn flowing_block(
        &mut self,
        page: &PageLayout,
        block: &Block,
        text_width: f32,
    ) -> Result<(), AppError> {
        if !matches!(block, Block::ListItem { .. }) {
            self.current_ordered = None;
        }
        match block {
            Block::Heading { level, spans, .. } => {
                let l = (*level).clamp(1, 3);
                self.body.push_str(&format!(
                    "<w:p><w:pPr><w:pStyle w:val=\"Heading{l}\"/></w:pPr>{}</w:p>",
                    self.runs(spans, true)
                ));
            }
            Block::Paragraph { spans, .. } => {
                self.body
                    .push_str(&format!("<w:p>{}</w:p>", self.runs(spans, false)));
            }
            Block::ListItem {
                ordered,
                marker,
                level,
                spans,
                ..
            } => {
                let num = if *ordered {
                    match self.current_ordered {
                        Some(id) => id,
                        None => {
                            let start = marker
                                .trim_matches(|c: char| !c.is_ascii_digit())
                                .parse::<u32>()
                                .unwrap_or(1)
                                .max(1);
                            self.ordered.push(start);
                            let id = u32::try_from(self.ordered.len() + 1).unwrap_or(u32::MAX);
                            self.current_ordered = Some(id);
                            id
                        }
                    }
                } else {
                    self.current_ordered = None;
                    1
                };
                self.body.push_str(&format!(
                    "<w:p><w:pPr><w:pStyle w:val=\"ListParagraph\"/><w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{num}\"/></w:numPr></w:pPr>{}</w:p>",
                    (*level).min(4),
                    self.runs(spans, false)
                ));
            }
            Block::Table(table) => {
                self.table(table, None, Some(text_width));
                // Word wants a paragraph between tables and after the last one.
                self.body.push_str("<w:p/>");
            }
            Block::Picture(p) => {
                if let Some(drawing) = self.picture(page, p, Some(text_width), None)? {
                    self.body
                        .push_str(&format!("<w:p><w:r>{drawing}</w:r></w:p>"));
                }
            }
        }
        Ok(())
    }

    // ----- exact -------------------------------------------------------------------------

    fn frame_open(rect: &Rect) -> String {
        let width = twips(rect.width() * 1.04 + 4.0);
        format!(
            "<w:framePr w:w=\"{width}\" w:wrap=\"notBeside\" w:vAnchor=\"page\" w:hAnchor=\"page\" w:x=\"{}\" w:y=\"{}\"/>",
            twips(rect.x0),
            twips(rect.y0)
        )
    }

    fn exact_block(&mut self, page: &PageLayout, block: &Block) -> Result<(), AppError> {
        const TIGHT: &str =
            "<w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/>";
        match block {
            Block::Heading { spans, rect, .. } | Block::Paragraph { spans, rect, .. } => {
                self.body.push_str(&format!(
                    "<w:p><w:pPr>{}{TIGHT}</w:pPr>{}</w:p>",
                    Self::frame_open(rect),
                    self.runs(spans, false)
                ));
            }
            Block::ListItem {
                ordered,
                marker,
                spans,
                rect,
                ..
            } => {
                let text = if *ordered {
                    marker.clone()
                } else {
                    "\u{2022}".to_owned()
                };
                let size = spans.first().map_or(self.body_size, |s| s.size);
                self.body.push_str(&format!(
                    "<w:p><w:pPr>{}{TIGHT}</w:pPr>{}{}</w:p>",
                    Self::frame_open(rect),
                    self.marker_run(&text, size),
                    self.runs(spans, false)
                ));
            }
            Block::Table(table) => {
                self.table(table, Some((table.rect.x0, table.rect.y0)), None);
            }
            Block::Picture(p) => {
                if let Some(drawing) = self.picture(page, p, None, Some((p.rect.x0, p.rect.y0)))? {
                    self.body.push_str(&format!(
                        "<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"20\" w:lineRule=\"exact\"/></w:pPr><w:r>{drawing}</w:r></w:p>"
                    ));
                }
            }
        }
        Ok(())
    }

    // ----- shared parts ------------------------------------------------------------------

    fn table(&mut self, table: &Table, at: Option<(f32, f32)>, text_width: Option<f32>) {
        let columns = table.columns();
        if columns == 0 {
            return;
        }
        let mut widths: Vec<f32> = (0..columns)
            .map(|c| {
                let left = table.column_edges.get(c).copied();
                let right = table
                    .column_edges
                    .get(c + 1)
                    .copied()
                    .unwrap_or(table.rect.x1);
                left.map_or(table.rect.width() / columns as f32, |l| {
                    (right - l).max(12.0)
                })
            })
            .collect();
        let total: f32 = widths.iter().sum();
        if let Some(limit) = text_width
            && total > limit
        {
            for w in &mut widths {
                *w *= limit / total;
            }
        }
        let float = at.map_or(String::new(), |(x, y)| {
            format!(
                "<w:tblpPr w:leftFromText=\"0\" w:rightFromText=\"0\" w:vertAnchor=\"page\" w:horzAnchor=\"page\" w:tblpX=\"{}\" w:tblpY=\"{}\"/>",
                twips(x),
                twips(y)
            )
        });
        let sum: i64 = widths.iter().map(|w| twips(*w)).sum();
        let mut xml = format!(
            "<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/>{float}<w:tblW w:w=\"{sum}\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/></w:tblPr><w:tblGrid>"
        );
        for w in &widths {
            xml.push_str(&format!("<w:gridCol w:w=\"{}\"/>", twips(*w)));
        }
        xml.push_str("</w:tblGrid>");
        for row in &table.rows {
            xml.push_str("<w:tr>");
            for (c, cell) in row.iter().enumerate() {
                let width = twips(widths.get(c).copied().unwrap_or(60.0));
                let span = Span {
                    text: cell.text.clone(),
                    bold: cell.bold,
                    italic: false,
                    size: self.body_size,
                };
                xml.push_str(&format!(
                    "<w:tc><w:tcPr><w:tcW w:w=\"{width}\" w:type=\"dxa\"/></w:tcPr><w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\"/></w:pPr>{}</w:p></w:tc>",
                    self.run(&span, false)
                ));
            }
            xml.push_str("</w:tr>");
        }
        xml.push_str("</w:tbl>");
        self.body.push_str(&xml);
    }

    /// The drawing for a picture, or none when the picture cannot be read.
    fn picture(
        &mut self,
        page: &PageLayout,
        picture: &PictureBox,
        max_width: Option<f32>,
        at: Option<(f32, f32)>,
    ) -> Result<Option<String>, AppError> {
        let data: PictureBytes = match (self.pictures)(page.number, picture.index) {
            Ok(d) => d,
            Err(e) if is_cancel(&e) => return Err(e),
            Err(_) => return Ok(None),
        };
        let (mut w, mut h) = (
            picture.rect.width().max(1.0),
            picture.rect.height().max(1.0),
        );
        if let Some(limit) = max_width
            && w > limit
        {
            h *= limit / w;
            w = limit;
        }
        let n = self.media.len() + 1;
        let extension = if data.jpeg { "jpg" } else { "png" };
        let name = format!("image{n}.{extension}");
        let rid = format!("rIdImage{n}");
        self.media.push((format!("word/media/{name}"), data.bytes));
        self.image_rels.push((rid.clone(), format!("media/{name}")));
        self.drawing_id += 1;
        let id = self.drawing_id;
        let (cx, cy) = (emu(w), emu(h));
        let graphic = format!(
            "<wp:docPr id=\"{id}\" name=\"Picture {id}\" descr=\"Picture from page {}\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"{name}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>",
            page.number
        );
        Ok(Some(match at {
            None => format!(
                "<w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/>{graphic}</wp:inline></w:drawing>"
            ),
            Some((x, y)) => format!(
                "<w:drawing><wp:anchor distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\" simplePos=\"0\" relativeHeight=\"{id}\" behindDoc=\"0\" locked=\"0\" layoutInCell=\"1\" allowOverlap=\"1\"><wp:simplePos x=\"0\" y=\"0\"/><wp:positionH relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset></wp:positionH><wp:positionV relativeFrom=\"page\"><wp:posOffset>{}</wp:posOffset></wp:positionV><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:wrapNone/>{graphic}</wp:anchor></w:drawing>",
                emu(x),
                emu(y)
            ),
        }))
    }
}

fn sect(width: f32, height: f32, margin: f32) -> String {
    let (w, h) = (twips(width), twips(height));
    let orient = if w > h { " w:orient=\"landscape\"" } else { "" };
    let m = twips(margin);
    format!(
        "<w:sectPr><w:pgSz w:w=\"{w}\" w:h=\"{h}\"{orient}/><w:pgMar w:top=\"{m}\" w:right=\"{m}\" w:bottom=\"{m}\" w:left=\"{m}\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/></w:sectPr>"
    )
}

fn styles(body_size: f32) -> String {
    let half = (body_size * 2.0).round() as u32;
    let mut s = format!(
        "{}<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:eastAsia=\"Calibri\" w:cs=\"Calibri\"/><w:sz w:val=\"{half}\"/><w:szCs w:val=\"{half}\"/><w:lang w:val=\"en-US\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"120\" w:line=\"259\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>",
        xml_head()
    );
    s.push_str("<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>");
    s.push_str("<w:style w:type=\"character\" w:default=\"1\" w:styleId=\"DefaultParagraphFont\"><w:name w:val=\"Default Paragraph Font\"/><w:uiPriority w:val=\"1\"/><w:semiHidden/></w:style>");
    s.push_str("<w:style w:type=\"table\" w:default=\"1\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:tblPr><w:tblInd w:w=\"0\" w:type=\"dxa\"/><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>");
    s.push_str("<w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:basedOn w:val=\"TableNormal\"/><w:uiPriority w:val=\"39\"/><w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/></w:tblBorders></w:tblPr></w:style>");
    for (level, size) in [(1u8, 40u32), (2, 32), (3, 28)] {
        s.push_str(&format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading{level}\"><w:name w:val=\"heading {level}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"80\"/><w:outlineLvl w:val=\"{}\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/></w:rPr></w:style>",
            level - 1
        ));
    }
    s.push_str("<w:style w:type=\"paragraph\" w:styleId=\"ListParagraph\"><w:name w:val=\"List Paragraph\"/><w:basedOn w:val=\"Normal\"/><w:uiPriority w:val=\"34\"/><w:qFormat/><w:pPr><w:spacing w:after=\"40\"/><w:ind w:left=\"720\"/><w:contextualSpacing/></w:pPr></w:style>");
    s.push_str("</w:styles>");
    s
}

fn numbering(ordered: &[u32]) -> String {
    let mut s = format!(
        "{}<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
        xml_head()
    );
    s.push_str(
        "<w:abstractNum w:abstractNumId=\"0\"><w:multiLevelType w:val=\"hybridMultilevel\"/>",
    );
    for i in 0..5 {
        s.push_str(&format!(
            "<w:lvl w:ilvl=\"{i}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"\u{2022}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:hint=\"default\"/></w:rPr></w:lvl>",
            720 + 360 * i
        ));
    }
    s.push_str("</w:abstractNum><w:abstractNum w:abstractNumId=\"1\"><w:multiLevelType w:val=\"hybridMultilevel\"/>");
    for (i, format) in [
        "decimal",
        "lowerLetter",
        "lowerRoman",
        "decimal",
        "lowerLetter",
    ]
    .iter()
    .enumerate()
    {
        s.push_str(&format!(
            "<w:lvl w:ilvl=\"{i}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{format}\"/><w:lvlText w:val=\"%{}.\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
            i + 1,
            720 + 360 * i
        ));
    }
    s.push_str("</w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>");
    for (i, start) in ordered.iter().enumerate() {
        s.push_str(&format!(
            "<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"1\"/><w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"{start}\"/></w:lvlOverride></w:num>",
            i + 2
        ));
    }
    s.push_str("</w:numbering>");
    s
}

pub(crate) fn write(
    pages: &[PageLayout],
    title: &str,
    exact: bool,
    pictures: &mut PictureSource<'_>,
    dest: &Path,
) -> Result<(), AppError> {
    let body_size = modal_size(pages);
    let mut builder = Builder {
        body_size,
        exact,
        body: String::new(),
        media: Vec::new(),
        image_rels: Vec::new(),
        drawing_id: 0,
        ordered: Vec::new(),
        current_ordered: None,
        pictures,
    };
    let first = pages.first().map_or(LETTER, |p| (p.width, p.height));
    let last_section = if exact {
        const BREAK: &str =
            "<w:spacing w:before=\"0\" w:after=\"0\" w:line=\"20\" w:lineRule=\"exact\"/>";
        let mut last = sect(first.0, first.1, 0.0);
        for (i, page) in pages.iter().enumerate() {
            for block in &page.blocks {
                builder.exact_block(page, block)?;
            }
            last = sect(page.width, page.height, 0.0);
            if i + 1 < pages.len() {
                builder
                    .body
                    .push_str(&format!("<w:p><w:pPr>{BREAK}{last}</w:pPr></w:p>"));
            } else {
                builder
                    .body
                    .push_str(&format!("<w:p><w:pPr>{BREAK}</w:pPr></w:p>"));
            }
        }
        if pages.is_empty() {
            builder.body.push_str("<w:p/>");
        }
        last
    } else {
        let text_width = (first.0 - 2.0 * MARGIN).max(72.0);
        for page in pages {
            for block in &page.blocks {
                builder.flowing_block(page, block, text_width)?;
            }
        }
        if builder.body.is_empty() || builder.body.ends_with("</w:tbl>") {
            builder.body.push_str("<w:p/>");
        }
        sect(first.0, first.1, MARGIN)
    };

    let document = format!(
        "{}<w:document {NS}><w:body>{}{last_section}</w:body></w:document>",
        xml_head(),
        builder.body
    );

    let mut rels = format!(
        "{}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/><Relationship Id=\"rIdNumbering\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>",
        xml_head()
    );
    for (id, target) in &builder.image_rels {
        rels.push_str(&format!(
            "<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"{target}\"/>"
        ));
    }
    rels.push_str("</Relationships>");

    let types = format!(
        "{}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpg\" ContentType=\"image/jpeg\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>",
        xml_head()
    );

    let mut package = Package::create(dest)?;
    package.add_text("[Content_Types].xml", &types)?;
    package.add_text(
        "_rels/.rels",
        &root_rels("officeDocument", "word/document.xml"),
    )?;
    package.add_text("docProps/core.xml", &core_properties(title))?;
    package.add_text("word/document.xml", &document)?;
    package.add_text("word/_rels/document.xml.rels", &rels)?;
    package.add_text("word/styles.xml", &styles(body_size))?;
    package.add_text("word/numbering.xml", &numbering(&builder.ordered))?;
    for (name, bytes) in &builder.media {
        package.add(name, bytes)?;
    }
    package.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{assert_well_formed, part, parts};
    use domain::layout::{Cell, PictureBox};

    fn span(text: &str, size: f32, bold: bool) -> Span {
        Span {
            text: text.to_owned(),
            bold,
            italic: false,
            size,
        }
    }

    fn rect(x: f32, y: f32) -> Rect {
        Rect::new(x, y, x + 200.0, y + 14.0)
    }

    fn sample() -> Vec<PageLayout> {
        let cell = |t: &str, bold: bool| Cell {
            text: t.to_owned(),
            bold,
        };
        vec![
            PageLayout {
                number: 1,
                width: 612.0,
                height: 792.0,
                blocks: vec![
                    Block::Heading {
                        level: 1,
                        spans: vec![span("Annual <Report>", 24.0, true)],
                        rect: rect(72.0, 72.0),
                    },
                    Block::Paragraph {
                        spans: vec![span("Body text of ", 11.0, false), span("bold", 11.0, true)],
                        rect: rect(72.0, 120.0),
                    },
                    Block::ListItem {
                        ordered: false,
                        marker: String::new(),
                        level: 0,
                        spans: vec![span("first", 11.0, false)],
                        rect: rect(72.0, 150.0),
                    },
                    Block::ListItem {
                        ordered: true,
                        marker: "3.".to_owned(),
                        level: 0,
                        spans: vec![span("three", 11.0, false)],
                        rect: rect(72.0, 170.0),
                    },
                    Block::ListItem {
                        ordered: true,
                        marker: "4.".to_owned(),
                        level: 0,
                        spans: vec![span("four", 11.0, false)],
                        rect: rect(72.0, 190.0),
                    },
                    Block::Table(Table {
                        rect: Rect::new(72.0, 220.0, 272.0, 260.0),
                        rows: vec![
                            vec![cell("Name", true), cell("Qty", true)],
                            vec![cell("Pens & ink", false), cell("12", false)],
                        ],
                        column_edges: vec![72.0, 172.0],
                        row_edges: vec![220.0, 240.0],
                    }),
                    Block::Picture(PictureBox {
                        rect: Rect::new(72.0, 300.0, 172.0, 400.0),
                        index: 0,
                    }),
                ],
            },
            PageLayout {
                number: 2,
                width: 612.0,
                height: 792.0,
                blocks: vec![Block::Paragraph {
                    spans: vec![span("Second page", 11.0, false)],
                    rect: rect(72.0, 72.0),
                }],
            },
        ]
    }

    fn pictures(_: u32, _: usize) -> Result<PictureBytes, AppError> {
        Ok(PictureBytes {
            width: 1,
            height: 1,
            jpeg: false,
            bytes: vec![0x89, b'P', b'N', b'G'],
        })
    }

    #[test]
    fn a_flowing_document_has_styles_lists_tables_and_pictures() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.docx");
        write(&sample(), "Report", false, &mut pictures, &path).unwrap();
        assert_well_formed(&path);
        let names = parts(&path);
        for part_name in [
            "word/document.xml",
            "word/styles.xml",
            "word/numbering.xml",
            "word/media/image1.png",
            "docProps/core.xml",
        ] {
            assert!(
                names.contains(&part_name.to_owned()),
                "{part_name} in {names:?}"
            );
        }
        let doc = part(&path, "word/document.xml");
        assert!(doc.contains("<w:pStyle w:val=\"Heading1\"/>"));
        assert!(doc.contains("Annual &lt;Report&gt;"));
        assert!(
            doc.contains("<w:b/><w:t xml:space=\"preserve\">bold</w:t>")
                || doc.contains("<w:rPr><w:b/></w:rPr><w:r>")
                || doc.contains("<w:rPr><w:b/></w:rPr><w:t")
        );
        assert!(doc.contains("<w:numId w:val=\"1\"/>"), "bullet list");
        assert!(doc.contains("<w:numId w:val=\"2\"/>"), "numbered list");
        assert!(doc.contains("<w:tbl>") && doc.contains("Pens &amp; ink"));
        assert!(doc.contains("<w:gridCol w:w=\"2000\"/>"), "{doc}");
        assert!(doc.contains("<wp:inline"));
        assert!(doc.contains("Second page"));
        assert!(doc.contains("<w:pgSz w:w=\"12240\" w:h=\"15840\"/>"));
        // The ordered list starts at the number it had.
        let numbering = part(&path, "word/numbering.xml");
        assert!(numbering.contains("<w:startOverride w:val=\"3\"/>"));
        // No frames in flowing text.
        assert!(!doc.contains("framePr"));
    }

    #[test]
    fn an_exact_document_places_blocks_in_frames_with_a_section_per_page() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("e.docx");
        write(&sample(), "Report", true, &mut pictures, &path).unwrap();
        assert_well_formed(&path);
        let doc = part(&path, "word/document.xml");
        // 72 points from the left and top are 1440 twips.
        assert!(doc.contains("w:x=\"1440\" w:y=\"1440\""), "{doc}");
        assert!(doc.contains("<w:tblpPr"));
        assert!(doc.contains("<wp:anchor"));
        assert_eq!(doc.matches("<w:sectPr>").count(), 2, "one per page");
        assert!(doc.contains("<w:sz w:val=\"22\"/>"), "sizes are explicit");
        // Exact lists carry their marker as text.
        assert!(doc.contains("3. </w:t>") || doc.contains(">3. <"), "{doc}");
    }

    #[test]
    fn a_picture_that_cannot_be_read_is_skipped_but_cancelling_stops() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.docx");
        let mut broken = |_: u32, _: usize| -> Result<PictureBytes, AppError> {
            Err(AppError::internal("a picture"))
        };
        write(&sample(), "t", false, &mut broken, &path).unwrap();
        assert!(!part(&path, "word/document.xml").contains("<wp:inline"));
        let mut cancelled = |_: u32, _: usize| -> Result<PictureBytes, AppError> {
            Err(AppError::new(domain::ErrorCode::JobCancelled, "stopped"))
        };
        let err = write(&sample(), "t", false, &mut cancelled, &path).unwrap_err();
        assert_eq!(err.code, domain::ErrorCode::JobCancelled);
    }

    #[test]
    fn an_empty_document_is_still_valid_and_wide_tables_are_squeezed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.docx");
        write(&[], "t", false, &mut pictures, &path).unwrap();
        assert_well_formed(&path);
        assert!(part(&path, "word/document.xml").contains("<w:p/>"));

        let wide = PageLayout {
            number: 1,
            width: 612.0,
            height: 792.0,
            blocks: vec![Block::Table(Table {
                rect: Rect::new(10.0, 10.0, 610.0, 50.0),
                rows: vec![vec![
                    Cell {
                        text: "a".into(),
                        bold: false,
                    },
                    Cell {
                        text: "b".into(),
                        bold: false,
                    },
                ]],
                column_edges: vec![10.0, 310.0],
                row_edges: vec![10.0],
            })],
        };
        let path = dir.path().join("wide.docx");
        write(&[wide], "t", false, &mut pictures, &path).unwrap();
        // 600 points of columns shrink to the 468 points between the margins (9360 twips).
        assert!(part(&path, "word/document.xml").contains("<w:tblW w:w=\"9360\""));
    }
}
