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
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
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

/// A syntactically valid PDF whose page tree holds no pages.
pub fn pdf_without_pages() -> Vec<u8> {
    let body = "%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n";
    let xref = body.len();
    format!(
        "{body}xref\n0 3\n0000000000 65535 f \n0000000009 00000 n \n0000000058 00000 n \ntrailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    )
    .into_bytes()
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
