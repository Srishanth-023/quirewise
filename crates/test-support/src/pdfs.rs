//! A small builder for test PDFs with the features the Organize tools need: custom page text,
//! page sizes and rotation, bookmarks and a form field.

/// What one generated page looks like.
#[derive(Debug, Clone)]
pub struct PageSpec {
    /// Text drawn near the top-left corner, at 24 pt.
    pub text: String,
    /// Page width in points.
    pub width: u32,
    /// Page height in points.
    pub height: u32,
    /// `/Rotate` value: 0, 90, 180 or 270.
    pub rotate: u32,
}

impl PageSpec {
    /// A Letter page showing `text`.
    pub fn letter(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            width: 612,
            height: 792,
            rotate: 0,
        }
    }
}

/// A bookmark: title, zero-based page, level (1 is the top level).
pub type Bookmark<'a> = (&'a str, usize, u32);

/// Builds the PDF bytes. Objects: 1 catalog, 2 page tree, 3 font, then a (page, content) pair per
/// page, then any extra objects. `extra` receives the id of the first extra object so the catalog
/// and pages can refer to it; it returns `(catalog_entries, extra_object_bodies)`.
pub fn build(
    pages: &[PageSpec],
    extra: impl FnOnce(u32) -> (String, Vec<String>),
    page_entries: impl Fn(usize) -> String,
) -> Vec<u8> {
    let n = pages.len().max(1);
    let first_extra = u32::try_from(4 + 2 * n).unwrap_or(u32::MAX);
    let (catalog_extra, extra_objects) = extra(first_extra);

    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + i * 2)).collect();
    let mut objects: Vec<String> = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>"),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    for (i, page) in pages.iter().enumerate() {
        let escaped = page
            .text
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        let content = format!(
            "BT /F1 24 Tf 40 {} Td ({escaped}) Tj ET",
            page.height.saturating_sub(60)
        );
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Rotate {} /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R {} >>",
            page.width,
            page.height,
            page.rotate,
            5 + i * 2,
            page_entries(i)
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    objects.extend(extra_objects);

    serialize(&objects, "")
}

/// Writes numbered objects (1-based, in order) with a cross-reference table. `trailer_extra` is
/// added to the trailer, for example `/Info 9 0 R`.
pub fn serialize(objects: &[String], trailer_extra: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R {trailer_extra} >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A PDF with one Letter page per entry, each showing the given text.
pub fn pdf_with_texts(texts: &[&str]) -> Vec<u8> {
    let pages: Vec<PageSpec> = texts.iter().map(|t| PageSpec::letter(*t)).collect();
    build(&pages, |_| (String::new(), Vec::new()), |_| String::new())
}

/// A PDF whose pages have the given sizes and rotations (`(width, height, rotate)`).
pub fn pdf_with_geometry(pages: &[(u32, u32, u32)]) -> Vec<u8> {
    let specs: Vec<PageSpec> = pages
        .iter()
        .enumerate()
        .map(|(i, (width, height, rotate))| PageSpec {
            text: format!("Page {}", i + 1),
            width: *width,
            height: *height,
            rotate: *rotate,
        })
        .collect();
    build(&specs, |_| (String::new(), Vec::new()), |_| String::new())
}

/// A PDF with `pages` pages and a bookmark tree. Items must be listed in reading order.
pub fn pdf_with_outline(pages: usize, bookmarks: &[Bookmark<'_>]) -> Vec<u8> {
    let specs: Vec<PageSpec> = (0..pages)
        .map(|i| PageSpec::letter(format!("Page {}", i + 1)))
        .collect();
    build(
        &specs,
        |first| {
            // Object `first` is the outline root; items follow in order.
            let count = bookmarks.len();
            let id = |i: usize| first + 1 + u32::try_from(i).unwrap_or(0);
            // Parent of item i is the closest earlier item with a smaller level.
            let parent_of = |i: usize| -> Option<usize> {
                (0..i).rev().find(|&j| bookmarks[j].2 < bookmarks[i].2)
            };
            let children = |p: Option<usize>| -> Vec<usize> {
                (0..count).filter(|&i| parent_of(i) == p).collect()
            };
            let mut objects = Vec::new();
            let top = children(None);
            let (first_top, last_top) = (top.first().copied(), top.last().copied());
            objects.push(match (first_top, last_top) {
                (Some(f), Some(l)) => format!(
                    "<< /Type /Outlines /First {} 0 R /Last {} 0 R /Count {count} >>",
                    id(f),
                    id(l)
                ),
                _ => "<< /Type /Outlines /Count 0 >>".to_owned(),
            });
            for (i, (title, page, _)) in bookmarks.iter().enumerate() {
                let siblings = children(parent_of(i));
                let pos = siblings.iter().position(|&s| s == i).unwrap_or(0);
                let kids = children(Some(i));
                let mut body = format!(
                    "<< /Title ({title}) /Parent {} 0 R /Dest [{} 0 R /Fit]",
                    parent_of(i).map_or(first, id),
                    4 + 2 * page
                );
                if pos > 0 {
                    body.push_str(&format!(" /Prev {} 0 R", id(siblings[pos - 1])));
                }
                if pos + 1 < siblings.len() {
                    body.push_str(&format!(" /Next {} 0 R", id(siblings[pos + 1])));
                }
                if let (Some(f), Some(l)) = (kids.first(), kids.last()) {
                    body.push_str(&format!(
                        " /First {} 0 R /Last {} 0 R /Count {}",
                        id(*f),
                        id(*l),
                        kids.len()
                    ));
                }
                body.push_str(" >>");
                objects.push(body);
            }
            (format!("/Outlines {first} 0 R"), objects)
        },
        |_| String::new(),
    )
}

/// A one-page PDF with a text form field named `name` holding `value`, including its appearance.
pub fn pdf_with_form_field(name: &str, value: &str) -> Vec<u8> {
    let specs = [PageSpec::letter("Form page")];
    // The page must list the widget in /Annots; ids are known once the extra id is.
    build(
        &specs,
        |first| {
            let field = format!(
                "<< /Type /Annot /Subtype /Widget /FT /Tx /T ({name}) /V ({value}) /Rect [100 600 300 630] /F 4 /P 4 0 R /DA (/Helv 12 Tf 0 g) /AP << /N {} 0 R >> >>",
                first + 1
            );
            let appearance = format!(
                "<< /Type /XObject /Subtype /Form /BBox [0 0 200 30] /Resources << /Font << /Helv 3 0 R >> >> /Length {} >>\nstream\nBT /Helv 12 Tf 2 10 Td ({value}) Tj ET\nendstream",
                "BT /Helv 12 Tf 2 10 Td () Tj ET".len() + value.len()
            );
            (
                format!(
                    "/AcroForm << /Fields [{first} 0 R] /DA (/Helv 12 Tf 0 g) /DR << /Font << /Helv 3 0 R >> >> >>"
                ),
                vec![field, appearance],
            )
        },
        |_| format!("/Annots [{} 0 R]", 4 + 2),
    )
}

/// Deterministic bytes that do not compress, so file sizes in tests are predictable.
fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as u8
        })
        .collect()
}

/// A PDF whose pages each carry about `bytes_per_page` bytes of incompressible data (a page
/// thumbnail image), so size-based tools have something to measure. `heavy` lists the pages that
/// get the data; the others stay tiny.
pub fn pdf_with_heavy_pages(pages: usize, bytes_per_page: usize, heavy: &[usize]) -> Vec<u8> {
    let specs: Vec<PageSpec> = (0..pages)
        .map(|i| PageSpec::letter(format!("Page {}", i + 1)))
        .collect();
    build(
        &specs,
        |_first| {
            let objects = (0..pages)
                .map(|i| {
                    let data = if heavy.contains(&i) { noise(bytes_per_page, i as u64 + 1) } else { vec![0u8; 16] };
                    // Raw bytes cannot be written through a String, so the stream body is hex coded.
                    let hex: String = data.iter().map(|b| format!("{b:02x}")).collect();
                    format!(
                        "<< /Type /XObject /Subtype /Image /Width {} /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /ASCIIHexDecode /Length {} >>\nstream\n{hex}>\nendstream",
                        data.len(),
                        hex.len() + 1
                    )
                })
                .collect();
            (String::new(), objects)
        },
        |i| format!("/Thumb {} 0 R", 4 + 2 * pages + i),
    )
}

/// A stream object body.
fn stream(dict: &str, content: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// One Letter page with document information (title and author) and no XMP.
pub fn pdf_with_metadata(title: &str, author: &str) -> Vec<u8> {
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".to_owned(),
        stream("", "BT /F1 24 Tf 40 700 Td (Page 1) Tj ET"),
        format!("<< /Title ({title}) /Author ({author}) /Producer (Test Producer) >>"),
    ];
    serialize(&objects, "/Info 6 0 R")
}

/// One Letter page without any document information.
pub fn minimal_pdf_without_info() -> Vec<u8> {
    pdf_with_texts(&["Page 1"])
}

/// Like [`pdf_with_metadata`], plus an XMP packet that repeats the title and author.
pub fn pdf_with_metadata_and_xmp(title: &str, author: &str) -> Vec<u8> {
    let xmp = format!(
        "<?xpacket begin=\"x\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{title}</rdf:li></rdf:Alt></dc:title><dc:creator><rdf:Seq><rdf:li>{author}</rdf:li></rdf:Seq></dc:creator></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
    );
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R /Metadata 7 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".to_owned(),
        stream("", "BT /F1 24 Tf 40 700 Td (Page 1) Tj ET"),
        format!("<< /Title ({title}) /Author ({author}) /Producer (Test Producer) >>"),
        stream("/Type /Metadata /Subtype /XML", &xmp),
    ];
    serialize(&objects, "/Info 6 0 R")
}

/// Two pages with annotations of every kind Remove annotations knows. Page 1 holds a URI link, a
/// highlight, a strikeout, a sticky note with its popup and a form field; page 2 holds an
/// internal link and a sticky note.
pub fn pdf_with_annotations() -> Vec<u8> {
    let rect = "/Rect [100 600 200 620]";
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [4 0 R 6 0 R] /Count 2 >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R /Annots [8 0 R 9 0 R 10 0 R 11 0 R 12 0 R 13 0 R] >>".to_owned(),
        stream("", "BT /F1 24 Tf 40 700 Td (Page 1) Tj ET"),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents 7 0 R /Annots [14 0 R 15 0 R] >>".to_owned(),
        stream("", "BT /F1 24 Tf 40 700 Td (Page 2) Tj ET"),
        format!("<< /Type /Annot /Subtype /Link {rect} /A << /S /URI /URI (http://example.com) >> >>"),
        format!("<< /Type /Annot /Subtype /Highlight {rect} /QuadPoints [100 620 200 620 100 600 200 600] /Contents (hl) >>"),
        format!("<< /Type /Annot /Subtype /StrikeOut {rect} /QuadPoints [100 620 200 620 100 600 200 600] >>"),
        format!("<< /Type /Annot /Subtype /Text {rect} /Contents (note) /Popup 12 0 R >>"),
        format!("<< /Type /Annot /Subtype /Popup {rect} /Parent 11 0 R >>"),
        format!("<< /Type /Annot /Subtype /Widget /FT /Tx /T (field) /V (value) {rect} >>"),
        format!("<< /Type /Annot /Subtype /Link {rect} /Dest [4 0 R /Fit] >>"),
        format!("<< /Type /Annot /Subtype /Text {rect} /Contents (second note) >>"),
    ];
    serialize(&objects, "")
}

/// One page packed with everything Sanitize removes: document information, XMP, JavaScript (open
/// action, name tree and page action), an attached file (name tree and attachment annotation), a
/// hidden layer with text, a link that opens a web address, a thumbnail and private application
/// data. The visible text is "Visible text"; the hidden layer holds "Hidden text".
pub fn pdf_with_risky_content() -> Vec<u8> {
    let content = "BT /F1 24 Tf 40 700 Td (Visible text) Tj ET\n\
                   /OC /OC2 BDC BT /F1 24 Tf 40 650 Td (Hidden text) Tj ET EMC\n\
                   /OC /OC1 BDC BT /F1 24 Tf 40 600 Td (Shown layer) Tj ET EMC";
    let objects = vec![
        // 1 catalog
        "<< /Type /Catalog /Pages 2 0 R /Metadata 12 0 R \
         /OpenAction << /S /JavaScript /JS (app.alert\\(1\\);) >> \
         /Names << /JavaScript << /Names [(init) 6 0 R] >> /EmbeddedFiles << /Names [(a.txt) 7 0 R] >> >> \
         /OCProperties << /OCGs [9 0 R 10 0 R] /D << /Order [9 0 R 10 0 R] /OFF [10 0 R] >> >> \
         /PieceInfo << /App << /Private (secret) >> >> >>".to_owned(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        // 4 page
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Resources << /Font << /F1 3 0 R >> /Properties << /OC1 9 0 R /OC2 10 0 R >> >> \
         /Contents 5 0 R /Thumb 14 0 R /AA << /O 6 0 R >> \
         /PieceInfo << /App << /Private (page secret) >> >> /Annots [13 0 R 15 0 R] >>".to_owned(),
        stream("", content),
        // 6 javascript action
        "<< /S /JavaScript /JS (app.alert\\(1\\);) >>".to_owned(),
        // 7 file specification, 8 embedded file
        "<< /Type /Filespec /F (a.txt) /EF << /F 8 0 R >> >>".to_owned(),
        stream("/Type /EmbeddedFile", "attached secret"),
        // 9 visible layer, 10 hidden layer
        "<< /Type /OCG /Name (Visible layer) >>".to_owned(),
        "<< /Type /OCG /Name (Hidden layer) >>".to_owned(),
        // 11 information, 12 XMP
        "<< /Title (Secret title) /Author (Secret author) /Producer (Test) >>".to_owned(),
        stream("/Type /Metadata /Subtype /XML", "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><secret/></x:xmpmeta>"),
        // 13 web link, 14 thumbnail, 15 attachment annotation
        "<< /Type /Annot /Subtype /Link /Rect [10 10 100 30] /A << /S /URI /URI (http://example.com) >> >>".to_owned(),
        stream("/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", "x"),
        "<< /Type /Annot /Subtype /FileAttachment /Rect [10 40 30 60] /FS 7 0 R /Name /Paperclip >>".to_owned(),
    ];
    serialize(&objects, "/Info 11 0 R")
}

/// A syntactically valid PDF whose page tree holds no pages.
pub fn pdf_without_pages() -> Vec<u8> {
    let body = "%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n";
    let xref = body.len();
    format!(
        "{body}xref\n0 3\n0000000000 65535 f \n0000000009 00000 n \n0000000058 00000 n \ntrailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    )
    .into_bytes()
}

/// A PDF with one Letter page per entry, each made of lines of `(text, font size)` from the top.
pub fn pdf_with_sized_lines(pages: &[Vec<(&str, f32)>]) -> Vec<u8> {
    let n = pages.len().max(1);
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + i * 2)).collect();
    let mut objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    for (i, lines) in pages.iter().enumerate() {
        let mut y = 740.0f32;
        let mut content = String::new();
        for (text, size) in lines {
            let escaped = text
                .replace('\\', "\\\\")
                .replace('(', "\\(")
                .replace(')', "\\)");
            content.push_str(&format!("BT /F1 {size} Tf 50 {y} Td ({escaped}) Tj ET\n"));
            y -= size * 1.6;
        }
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            5 + i * 2
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    serialize(&objects, "")
}

/// A one-page Letter PDF with each text placed at `(x, y)` (from the lower left), size 12.
/// The texts must not contain parentheses or backslashes.
pub fn pdf_with_positioned_text(items: &[(f32, f32, &str)]) -> Vec<u8> {
    let mut content = String::new();
    for (x, y, text) in items {
        content.push_str(&format!("BT /F1 12 Tf {x} {y} Td ({text}) Tj ET\n"));
    }
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    serialize(&objects, "")
}

/// A one-page Letter PDF of text lines turned counter-clockwise by `degrees` about the center,
/// like a page that went into the scanner crooked. `lines` of text fill the page.
pub fn pdf_with_tilted_lines(degrees: f32, lines: usize) -> Vec<u8> {
    let (s, c) = degrees.to_radians().sin_cos();
    let mut content = format!(
        "1 0 0 1 306 396 cm\n{c:.5} {s:.5} {ns:.5} {c:.5} 0 0 cm\n1 0 0 1 -306 -396 cm\n",
        ns = -s
    );
    for i in 0..lines {
        let y = 720.0 - 20.0 * i as f32;
        content.push_str(&format!(
            "BT /F1 12 Tf 60 {y} Td (Line {i} of the quarterly report with enough words to look like a real page of text) Tj ET\n"
        ));
    }
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    serialize(&objects, "")
}

/// A one-page PDF with a red filled box, a green outline and blue text.
pub fn pdf_with_colors() -> Vec<u8> {
    let content = "1 0 0 rg 100 400 200 200 re f\n0 1 0 RG 6 w 350 400 200 200 re S\n0 0 1 rg BT /F1 36 Tf 100 300 Td (COLORS) Tj ET";
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    serialize(&objects, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_become_pages() {
        let pdf = pdf_with_texts(&["INV-1", "INV-2 (copy)"]);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Count 2"));
        assert!(text.contains("(INV-1) Tj"));
        assert!(text.contains("INV-2 \\(copy\\)"), "parentheses are escaped");
    }

    #[test]
    fn geometry_is_written_per_page() {
        let pdf = pdf_with_geometry(&[(300, 400, 90), (612, 792, 0)]);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/MediaBox [0 0 300 400] /Rotate 90"));
    }

    #[test]
    fn outlines_reference_pages_and_nest() {
        let pdf = pdf_with_outline(4, &[("One", 0, 1), ("One A", 1, 2), ("Two", 2, 1)]);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Type /Outlines"));
        assert!(text.contains("/Title (One A)"));
        assert!(
            text.contains("/Dest [8 0 R /Fit]"),
            "page index 2 is object 8"
        );
    }
}
