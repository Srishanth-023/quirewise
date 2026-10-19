//! PowerPoint presentations: one slide per page, either with text boxes, tables and pictures
//! that can be edited, or with the page as one picture.

use application::AppError;
use application::ports::{PictureSource, RenderedImage};
use domain::layout::{Block, PageLayout, Rect, Span, Table};
use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder};
use std::path::Path;

use crate::{Package, core_properties, emu, is_cancel, root_rels, xml_escape, xml_head};

const NS: &str = "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"";

const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// Slides this many points wide or tall are the most PowerPoint opens.
const MAX_SLIDE: f32 = 4000.0;
/// Points of marker space of a list item.
const MARKER_SPACE: f32 = 18.0;

/// A slide being built.
struct Slide {
    shapes: String,
    rels: Vec<(String, String)>,
    next_id: u32,
}

impl Slide {
    fn new() -> Self {
        Self {
            shapes: String::new(),
            rels: Vec::new(),
            next_id: 2,
        }
    }

    fn id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

/// Where a page goes on its slide: a scale and an offset in points.
#[derive(Clone, Copy)]
struct Fit {
    k: f32,
    dx: f32,
    dy: f32,
}

impl Fit {
    fn new(slide: (f32, f32), page: (f32, f32)) -> Self {
        let k = (slide.0 / page.0.max(1.0)).min(slide.1 / page.1.max(1.0));
        Self {
            k,
            dx: (slide.0 - page.0 * k) / 2.0,
            dy: (slide.1 - page.1 * k) / 2.0,
        }
    }

    /// Offset and size of a box, in EMU.
    fn xfrm(&self, rect: &Rect, slack: f32) -> String {
        let w =
            (rect.width() * self.k).max(1.0) * (1.0 + slack) + if slack > 0.0 { 4.0 } else { 0.0 };
        let h = (rect.height() * self.k).max(1.0);
        format!(
            "<a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>",
            emu(self.dx + rect.x0 * self.k),
            emu(self.dy + rect.y0 * self.k),
            emu(w),
            emu(h)
        )
    }
}

fn run(span: &Span, k: f32) -> String {
    if span.text.is_empty() {
        return String::new();
    }
    let size = ((span.size * k * 100.0).round() as i64).clamp(100, 400_000);
    let mut attrs = format!("lang=\"en-US\" sz=\"{size}\" dirty=\"0\"");
    if span.bold {
        attrs.push_str(" b=\"1\"");
    }
    if span.italic {
        attrs.push_str(" i=\"1\"");
    }
    format!(
        "<a:r><a:rPr {attrs}/><a:t>{}</a:t></a:r>",
        xml_escape(&span.text)
    )
}

fn text_box(slide: &mut Slide, fit: Fit, rect: &Rect, ppr: &str, spans: &[Span], name: &str) {
    let id = slide.id();
    let runs: String = spans.iter().map(|s| run(s, fit.k)).collect();
    slide.shapes.push_str(&format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name} {id}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap=\"square\" lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" rtlCol=\"0\"><a:noAutofit/></a:bodyPr><a:lstStyle/><a:p>{ppr}{runs}</a:p></p:txBody></p:sp>",
        fit.xfrm(rect, 0.08)
    ));
}

fn table_frame(slide: &mut Slide, fit: Fit, table: &Table) {
    let columns = table.columns();
    if columns == 0 {
        return;
    }
    let id = slide.id();
    let widths: Vec<f32> = (0..columns)
        .map(|c| {
            let right = table
                .column_edges
                .get(c + 1)
                .copied()
                .unwrap_or(table.rect.x1);
            table
                .column_edges
                .get(c)
                .map_or(table.rect.width() / columns as f32, |l| {
                    (right - l).max(8.0)
                })
                * fit.k
        })
        .collect();
    let heights: Vec<f32> = (0..table.rows.len())
        .map(|r| {
            let bottom = table.row_edges.get(r + 1).copied().unwrap_or(table.rect.y1);
            table
                .row_edges
                .get(r)
                .map_or(table.rect.height() / table.rows.len() as f32, |t| {
                    (bottom - t).max(8.0)
                })
                * fit.k
        })
        .collect();
    let font = (heights.iter().sum::<f32>() / heights.len().max(1) as f32 * 0.55).clamp(6.0, 14.0);
    let border = "<a:solidFill><a:srgbClr val=\"808080\"/></a:solidFill>";
    let mut xml = format!(
        "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"Table {id}\"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></p:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\"><a:tbl><a:tblPr firstRow=\"1\"/><a:tblGrid>",
        emu(fit.dx + table.rect.x0 * fit.k),
        emu(fit.dy + table.rect.y0 * fit.k),
        emu(widths.iter().sum()),
        emu(heights.iter().sum())
    );
    for w in &widths {
        xml.push_str(&format!("<a:gridCol w=\"{}\"/>", emu(*w)));
    }
    xml.push_str("</a:tblGrid>");
    for (r, row) in table.rows.iter().enumerate() {
        xml.push_str(&format!("<a:tr h=\"{}\">", emu(heights[r])));
        for cell in row {
            let body = if cell.text.is_empty() {
                format!(
                    "<a:p><a:endParaRPr lang=\"en-US\" sz=\"{}\"/></a:p>",
                    (font * 100.0) as i64
                )
            } else {
                format!(
                    "<a:p>{}</a:p>",
                    run(
                        &Span {
                            text: cell.text.clone(),
                            bold: cell.bold,
                            italic: false,
                            size: font / fit.k.max(0.01),
                        },
                        fit.k
                    )
                )
            };
            xml.push_str(&format!(
                "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{body}</a:txBody><a:tcPr marL=\"45720\" marR=\"45720\" marT=\"22860\" marB=\"22860\"><a:lnL w=\"6350\">{border}</a:lnL><a:lnR w=\"6350\">{border}</a:lnR><a:lnT w=\"6350\">{border}</a:lnT><a:lnB w=\"6350\">{border}</a:lnB><a:noFill/></a:tcPr></a:tc>"
            ));
        }
        xml.push_str("</a:tr>");
    }
    xml.push_str("</a:tbl></a:graphicData></a:graphic></p:graphicFrame>");
    slide.shapes.push_str(&xml);
}

fn picture_shape(slide: &mut Slide, fit: Fit, rect: &Rect, media: &str, descr: &str) {
    let id = slide.id();
    let rid = format!("rIdImage{id}");
    slide.rels.push((rid.clone(), format!("../media/{media}")));
    slide.shapes.push_str(&format!(
        "<p:pic><p:nvPicPr><p:cNvPr id=\"{id}\" name=\"Picture {id}\" descr=\"{}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>",
        xml_escape(descr),
        fit.xfrm(rect, 0.0)
    ));
}

const THEME: &str = "<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"Quirewise\"><a:themeElements>\
<a:clrScheme name=\"Office\"><a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1><a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1>\
<a:dk2><a:srgbClr val=\"1F497D\"/></a:dk2><a:lt2><a:srgbClr val=\"EEECE1\"/></a:lt2>\
<a:accent1><a:srgbClr val=\"4F81BD\"/></a:accent1><a:accent2><a:srgbClr val=\"C0504D\"/></a:accent2><a:accent3><a:srgbClr val=\"9BBB59\"/></a:accent3>\
<a:accent4><a:srgbClr val=\"8064A2\"/></a:accent4><a:accent5><a:srgbClr val=\"4BACC6\"/></a:accent5><a:accent6><a:srgbClr val=\"F79646\"/></a:accent6>\
<a:hlink><a:srgbClr val=\"0000FF\"/></a:hlink><a:folHlink><a:srgbClr val=\"800080\"/></a:folHlink></a:clrScheme>\
<a:fontScheme name=\"Office\"><a:majorFont><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme>\
<a:fmtScheme name=\"Office\">\
<a:fillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:fillStyleLst>\
<a:lnStyleLst><a:ln w=\"9525\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln><a:ln w=\"25400\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln><a:ln w=\"38100\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln></a:lnStyleLst>\
<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>\
<a:bgFillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:bgFillStyleLst>\
</a:fmtScheme></a:themeElements></a:theme>";

const GROUP: &str = "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>";

fn master() -> String {
    format!(
        "{}<p:sldMaster {NS}><p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg><p:spTree>{GROUP}</p:spTree></p:cSld><p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/><p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle><a:lvl1pPr><a:defRPr sz=\"4400\"/></a:lvl1pPr></p:titleStyle><p:bodyStyle><a:lvl1pPr><a:defRPr sz=\"2800\"/></a:lvl1pPr></p:bodyStyle><p:otherStyle><a:lvl1pPr><a:defRPr sz=\"1800\"/></a:lvl1pPr></p:otherStyle></p:txStyles></p:sldMaster>",
        xml_head()
    )
}

fn layout() -> String {
    format!(
        "{}<p:sldLayout {NS} type=\"blank\" preserve=\"1\"><p:cSld name=\"Blank\"><p:spTree>{GROUP}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>",
        xml_head()
    )
}

fn rels_xml(items: &[(String, &str, String)]) -> String {
    let mut s = format!(
        "{}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        xml_head()
    );
    for (id, kind, target) in items {
        s.push_str(&format!(
            "<Relationship Id=\"{id}\" Type=\"{REL}/{kind}\" Target=\"{target}\"/>"
        ));
    }
    s.push_str("</Relationships>");
    s
}

/// The finished slides and what goes with them, written as a package.
struct Deck {
    size: (f32, f32),
    slides: Vec<Slide>,
    media: Vec<(String, Vec<u8>)>,
}

impl Deck {
    fn media_name(&mut self, bytes: Vec<u8>, jpeg: bool) -> String {
        let name = format!(
            "image{}.{}",
            self.media.len() + 1,
            if jpeg { "jpg" } else { "png" }
        );
        self.media.push((format!("ppt/media/{name}"), bytes));
        name
    }

    fn finish(self, title: &str, dest: &Path) -> Result<(), AppError> {
        let n = self.slides.len();
        let mut types = format!(
            "{}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpg\" ContentType=\"image/jpeg\"/><Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/><Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/><Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/><Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>",
            xml_head()
        );
        for i in 1..=n {
            types.push_str(&format!(
                "<Override PartName=\"/ppt/slides/slide{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
            ));
        }
        types.push_str("</Types>");

        let mut ids = String::new();
        let mut pres_rels: Vec<(String, &str, String)> = vec![(
            "rId1".to_owned(),
            "slideMaster",
            "slideMasters/slideMaster1.xml".to_owned(),
        )];
        for i in 1..=n {
            ids.push_str(&format!(
                "<p:sldId id=\"{}\" r:id=\"rId{}\"/>",
                255 + i,
                i + 1
            ));
            pres_rels.push((
                format!("rId{}", i + 1),
                "slide",
                format!("slides/slide{i}.xml"),
            ));
        }
        pres_rels.push((
            format!("rId{}", n + 2),
            "theme",
            "theme/theme1.xml".to_owned(),
        ));
        let presentation = format!(
            "{}<p:presentation {NS} saveSubsetFonts=\"1\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx=\"{}\" cy=\"{}\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>",
            xml_head(),
            emu(self.size.0),
            emu(self.size.1)
        );

        let mut package = Package::create(dest)?;
        package.add_text("[Content_Types].xml", &types)?;
        package.add_text(
            "_rels/.rels",
            &root_rels("officeDocument", "ppt/presentation.xml"),
        )?;
        package.add_text("docProps/core.xml", &core_properties(title))?;
        package.add_text("ppt/presentation.xml", &presentation)?;
        package.add_text("ppt/_rels/presentation.xml.rels", &rels_xml(&pres_rels))?;
        package.add_text("ppt/slideMasters/slideMaster1.xml", &master())?;
        package.add_text(
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            &rels_xml(&[
                (
                    "rId1".to_owned(),
                    "slideLayout",
                    "../slideLayouts/slideLayout1.xml".to_owned(),
                ),
                ("rId2".to_owned(), "theme", "../theme/theme1.xml".to_owned()),
            ]),
        )?;
        package.add_text("ppt/slideLayouts/slideLayout1.xml", &layout())?;
        package.add_text(
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            &rels_xml(&[(
                "rId1".to_owned(),
                "slideMaster",
                "../slideMasters/slideMaster1.xml".to_owned(),
            )]),
        )?;
        package.add_text("ppt/theme/theme1.xml", &format!("{}{THEME}", xml_head()))?;
        for (i, slide) in self.slides.iter().enumerate() {
            let xml = format!(
                "{}<p:sld {NS}><p:cSld><p:spTree>{GROUP}{}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>",
                xml_head(),
                slide.shapes
            );
            package.add_text(&format!("ppt/slides/slide{}.xml", i + 1), &xml)?;
            let mut rels: Vec<(String, &str, String)> = vec![(
                "rId1".to_owned(),
                "slideLayout",
                "../slideLayouts/slideLayout1.xml".to_owned(),
            )];
            for (id, target) in &slide.rels {
                rels.push((id.clone(), "image", target.clone()));
            }
            package.add_text(
                &format!("ppt/slides/_rels/slide{}.xml.rels", i + 1),
                &rels_xml(&rels),
            )?;
        }
        for (name, bytes) in &self.media {
            package.add(name, bytes)?;
        }
        package.finish()
    }
}

fn slide_size(first: (f32, f32)) -> (f32, f32) {
    (
        first.0.clamp(72.0, MAX_SLIDE),
        first.1.clamp(72.0, MAX_SLIDE),
    )
}

pub(crate) fn write(
    pages: &[PageLayout],
    title: &str,
    pictures: &mut PictureSource<'_>,
    dest: &Path,
) -> Result<(), AppError> {
    let size = slide_size(
        pages
            .first()
            .map_or((720.0, 540.0), |p| (p.width, p.height)),
    );
    let mut deck = Deck {
        size,
        slides: Vec::new(),
        media: Vec::new(),
    };
    for page in pages {
        let fit = Fit::new(size, (page.width, page.height));
        let mut slide = Slide::new();
        for block in &page.blocks {
            match block {
                Block::Heading { spans, rect, .. } | Block::Paragraph { spans, rect, .. } => {
                    text_box(&mut slide, fit, rect, "", spans, "Text");
                }
                Block::ListItem {
                    ordered,
                    marker,
                    level,
                    spans,
                    rect,
                } => {
                    let left = emu((MARKER_SPACE + f32::from(*level) * MARKER_SPACE) * fit.k);
                    let bullet = if *ordered {
                        let start = marker
                            .trim_matches(|c: char| !c.is_ascii_digit())
                            .parse::<u32>()
                            .unwrap_or(1)
                            .max(1);
                        format!(
                            "<a:buFont typeface=\"+mj-lt\"/><a:buAutoNum type=\"arabicPeriod\" startAt=\"{start}\"/>"
                        )
                    } else {
                        "<a:buFont typeface=\"Arial\"/><a:buChar char=\"\u{2022}\"/>".to_owned()
                    };
                    let ppr = format!(
                        "<a:pPr marL=\"{left}\" indent=\"-{}\">{bullet}</a:pPr>",
                        emu(MARKER_SPACE * fit.k)
                    );
                    text_box(&mut slide, fit, rect, &ppr, spans, "List");
                }
                Block::Table(table) => table_frame(&mut slide, fit, table),
                Block::Picture(p) => {
                    let data = match pictures(page.number, p.index) {
                        Ok(d) => d,
                        Err(e) if is_cancel(&e) => return Err(e),
                        Err(_) => continue,
                    };
                    let name = deck.media_name(data.bytes, data.jpeg);
                    picture_shape(
                        &mut slide,
                        fit,
                        &p.rect,
                        &name,
                        &format!("Picture from page {}", page.number),
                    );
                }
            }
        }
        deck.slides.push(slide);
    }
    deck.finish(title, dest)
}

fn jpeg_of(image: &RenderedImage) -> Result<Vec<u8>, AppError> {
    let mut rgb = Vec::with_capacity(image.rgba.len() / 4 * 3);
    for px in image.rgba.as_chunks::<4>().0 {
        let a = u16::from(px[3]);
        for c in &px[..3] {
            rgb.push(((u16::from(*c) * a + 255 * (255 - a)) / 255) as u8);
        }
    }
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 85)
        .write_image(&rgb, image.width, image.height, ExtendedColorType::Rgb8)
        .map_err(|_| AppError::internal("encoding a slide picture"))?;
    Ok(out)
}

pub(crate) fn write_images(
    sizes: &[(f32, f32)],
    title: &str,
    image: &mut dyn FnMut(usize) -> Result<RenderedImage, AppError>,
    dest: &Path,
) -> Result<(), AppError> {
    let size = slide_size(sizes.first().copied().unwrap_or((720.0, 540.0)));
    let mut deck = Deck {
        size,
        slides: Vec::new(),
        media: Vec::new(),
    };
    for (i, page) in sizes.iter().enumerate() {
        let picture = image(i)?;
        let name = deck.media_name(jpeg_of(&picture)?, true);
        let fit = Fit::new(size, *page);
        let mut slide = Slide::new();
        picture_shape(
            &mut slide,
            fit,
            &Rect::new(0.0, 0.0, page.0, page.1),
            &name,
            &format!("Page {}", i + 1),
        );
        deck.slides.push(slide);
    }
    deck.finish(title, dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{assert_well_formed, part, parts};
    use application::ports::PictureBytes;
    use domain::layout::{Cell, PictureBox};

    fn span(text: &str, size: f32, bold: bool) -> Span {
        Span {
            text: text.to_owned(),
            bold,
            italic: false,
            size,
        }
    }

    fn pages() -> Vec<PageLayout> {
        let cell = |t: &str| Cell {
            text: t.to_owned(),
            bold: false,
        };
        vec![
            PageLayout {
                number: 1,
                width: 720.0,
                height: 540.0,
                blocks: vec![
                    Block::Heading {
                        level: 1,
                        spans: vec![span("Quarterly <results>", 32.0, true)],
                        rect: Rect::new(50.0, 40.0, 400.0, 80.0),
                    },
                    Block::ListItem {
                        ordered: false,
                        marker: String::new(),
                        level: 0,
                        spans: vec![span("Growth", 18.0, false)],
                        rect: Rect::new(50.0, 120.0, 300.0, 145.0),
                    },
                    Block::ListItem {
                        ordered: true,
                        marker: "2.".to_owned(),
                        level: 1,
                        spans: vec![span("Costs", 18.0, false)],
                        rect: Rect::new(70.0, 150.0, 300.0, 175.0),
                    },
                    Block::Table(Table {
                        rect: Rect::new(50.0, 200.0, 350.0, 260.0),
                        rows: vec![vec![cell("A"), cell("B")], vec![cell("1"), cell("")]],
                        column_edges: vec![50.0, 200.0],
                        row_edges: vec![200.0, 230.0],
                    }),
                    Block::Picture(PictureBox {
                        rect: Rect::new(400.0, 200.0, 600.0, 400.0),
                        index: 0,
                    }),
                ],
            },
            PageLayout {
                number: 2,
                width: 720.0,
                height: 540.0,
                blocks: vec![],
            },
        ]
    }

    fn pictures(_: u32, _: usize) -> Result<PictureBytes, AppError> {
        Ok(PictureBytes {
            width: 1,
            height: 1,
            jpeg: true,
            bytes: vec![0xFF, 0xD8, 0xFF],
        })
    }

    #[test]
    fn editable_slides_have_text_lists_tables_and_pictures() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("e.pptx");
        write(&pages(), "Deck", &mut pictures, &path).unwrap();
        assert_well_formed(&path);
        let names = parts(&path);
        for n in [
            "ppt/presentation.xml",
            "ppt/slides/slide1.xml",
            "ppt/slides/slide2.xml",
            "ppt/slideMasters/slideMaster1.xml",
            "ppt/slideLayouts/slideLayout1.xml",
            "ppt/theme/theme1.xml",
            "ppt/media/image1.jpg",
        ] {
            assert!(names.contains(&n.to_owned()), "{n} in {names:?}");
        }
        let pres = part(&path, "ppt/presentation.xml");
        // 720 by 540 points in EMU.
        assert!(
            pres.contains("<p:sldSz cx=\"9144000\" cy=\"6858000\"/>"),
            "{pres}"
        );
        assert_eq!(pres.matches("<p:sldId ").count(), 2);
        let slide = part(&path, "ppt/slides/slide1.xml");
        assert!(slide.contains("Quarterly &lt;results&gt;"));
        assert!(slide.contains("sz=\"3200\"") && slide.contains("b=\"1\""));
        assert!(slide.contains("<a:buChar char=\"\u{2022}\"/>"));
        assert!(slide.contains("<a:buAutoNum type=\"arabicPeriod\" startAt=\"2\"/>"));
        assert!(
            slide.contains("<a:tbl>") && slide.contains("<a:gridCol w=\"1905000\"/>"),
            "{slide}"
        );
        assert!(slide.contains("<p:pic>") && slide.contains("r:embed=\"rIdImage"));
        let rels = part(&path, "ppt/slides/_rels/slide1.xml.rels");
        assert!(rels.contains("../media/image1.jpg"));
    }

    #[test]
    fn pages_of_other_sizes_are_scaled_into_the_slide() {
        let fit = Fit::new((720.0, 540.0), (360.0, 540.0));
        assert!((fit.k - 1.0).abs() < 1e-4);
        assert!((fit.dx - 180.0).abs() < 1e-3 && fit.dy.abs() < 1e-3);
        let big = Fit::new((720.0, 540.0), (1440.0, 1080.0));
        assert!((big.k - 0.5).abs() < 1e-4);
    }

    #[test]
    fn image_slides_hold_one_jpeg_each() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("i.pptx");
        let mut render = |_: usize| -> Result<RenderedImage, AppError> {
            Ok(RenderedImage {
                width: 8,
                height: 6,
                rgba: vec![200; 8 * 6 * 4],
            })
        };
        write_images(
            &[(612.0, 792.0), (612.0, 792.0)],
            "Scans",
            &mut render,
            &path,
        )
        .unwrap();
        assert_well_formed(&path);
        let names = parts(&path);
        assert!(
            names.contains(&"ppt/media/image1.jpg".to_owned())
                && names.contains(&"ppt/media/image2.jpg".to_owned())
        );
        let slide = part(&path, "ppt/slides/slide2.xml");
        assert!(slide.contains("<p:pic>") && !slide.contains("<p:sp>"));
        assert!(part(&path, "ppt/presentation.xml").contains("cx=\"7772400\" cy=\"10058400\""));
    }

    #[test]
    fn a_presentation_without_pages_is_still_well_formed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("n.pptx");
        write(&[], "t", &mut pictures, &path).unwrap();
        assert_well_formed(&path);
    }
}
