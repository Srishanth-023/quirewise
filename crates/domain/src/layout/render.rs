//! Markdown, HTML and CSV from analyzed pages.

use super::model::{Block, PageLayout, PictureBox, Span, Table, plain};

/// Where a picture is saved or embedded: the address to write, or none to leave it out.
pub type PictureRef<'a> = &'a dyn Fn(u32, &PictureBox) -> Option<String>;

// ----- Markdown ----------------------------------------------------------------------------

fn escape_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Keeps a block from being read as other Markdown by its first characters.
fn escape_line_start(text: String) -> String {
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    let numbered = digits > 0 && text[digits..].starts_with(['.', ')']);
    if text.starts_with(['#', '+', '-', '=']) || numbered {
        format!("\\{text}")
    } else {
        text
    }
}

fn emphasize(text: &str, bold: bool, italic: bool) -> String {
    let core = text.trim();
    if core.is_empty() || (!bold && !italic) {
        return escape_markdown(text);
    }
    let lead = &text[..text.len() - text.trim_start().len()];
    let trail = &text[text.trim_end().len()..];
    let mark = match (bold, italic) {
        (true, true) => "***",
        (true, false) => "**",
        _ => "*",
    };
    format!("{lead}{mark}{}{mark}{trail}", escape_markdown(core))
}

fn spans_markdown(spans: &[Span], all_bold: bool) -> String {
    spans
        .iter()
        .map(|s| emphasize(&s.text, s.bold && !all_bold, s.italic))
        .collect()
}

fn table_markdown(table: &Table) -> String {
    let columns = table.columns();
    if columns == 0 {
        return String::new();
    }
    let cell = |text: &str| escape_markdown(text).replace('\n', " ");
    let mut out = String::new();
    for (n, row) in table.rows.iter().enumerate() {
        out.push('|');
        for c in row {
            out.push(' ');
            out.push_str(&cell(&c.text));
            out.push_str(" |");
        }
        out.push('\n');
        if n == 0 {
            out.push('|');
            out.push_str(&" --- |".repeat(columns));
            out.push('\n');
        }
    }
    out
}

/// The pages as one Markdown document.
pub fn markdown(pages: &[PageLayout], picture: PictureRef<'_>) -> String {
    let mut out = String::new();
    let mut ordered_count = 0u32;
    let mut in_list = false;
    for page in pages {
        for block in &page.blocks {
            let is_list = matches!(block, Block::ListItem { .. });
            if !is_list {
                ordered_count = 0;
                if in_list {
                    out.push('\n');
                    in_list = false;
                }
            } else {
                in_list = true;
            }
            match block {
                Block::Heading { level, spans, .. } => {
                    out.push_str(&"#".repeat(usize::from((*level).clamp(1, 6))));
                    out.push(' ');
                    out.push_str(&spans_markdown(spans, true));
                    out.push_str("\n\n");
                }
                Block::Paragraph { spans, .. } => {
                    out.push_str(&escape_line_start(spans_markdown(spans, false)));
                    out.push_str("\n\n");
                }
                Block::ListItem {
                    ordered,
                    marker,
                    level,
                    spans,
                    ..
                } => {
                    ordered_count += 1;
                    let number = marker
                        .trim_matches(|c: char| !c.is_ascii_digit())
                        .parse::<u32>()
                        .unwrap_or(ordered_count);
                    out.push_str(&"  ".repeat(usize::from(*level)));
                    if *ordered {
                        out.push_str(&format!("{number}. "));
                    } else {
                        out.push_str("- ");
                    }
                    out.push_str(&spans_markdown(spans, false));
                    out.push('\n');
                }
                Block::Table(t) => {
                    out.push_str(&table_markdown(t));
                    out.push('\n');
                }
                Block::Picture(p) => {
                    if let Some(src) = picture(page.number, p) {
                        out.push_str(&format!("![Picture on page {}]({src})\n\n", page.number));
                    }
                }
            }
        }
    }
    let trimmed = out.trim_end().len();
    out.truncate(trimmed);
    out.push('\n');
    out
}

// ----- HTML --------------------------------------------------------------------------------

/// Text made safe for HTML content and attribute values.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn spans_html(spans: &[Span], all_bold: bool) -> String {
    let mut out = String::new();
    for s in spans {
        let mut text = escape_html(&s.text);
        if s.italic {
            text = format!("<em>{text}</em>");
        }
        if s.bold && !all_bold {
            text = format!("<strong>{text}</strong>");
        }
        out.push_str(&text);
    }
    out
}

fn table_html(table: &Table) -> String {
    let mut out = String::from("<table>\n");
    for (n, row) in table.rows.iter().enumerate() {
        out.push_str("<tr>");
        let tag = if n == 0 { "th" } else { "td" };
        for c in row {
            out.push_str(&format!("<{tag}>{}</{tag}>", escape_html(&c.text)));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</table>\n");
    out
}

const STYLE: &str = "body{font-family:Segoe UI,Arial,sans-serif;line-height:1.5;max-width:46rem;margin:2rem auto;padding:0 1rem;color:#1a1a1a}\
table{border-collapse:collapse;margin:1rem 0}th,td{border:1px solid #888;padding:.3rem .6rem;text-align:left}img{max-width:100%}";

/// The pages as one HTML document with a small built-in style and no scripts.
pub fn html(pages: &[PageLayout], title: &str, picture: PictureRef<'_>) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str(&format!(
        "<title>{}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n",
        escape_html(title)
    ));
    // Open lists, by whether each is numbered.
    let mut stack: Vec<bool> = Vec::new();
    let close_lists = |stack: &mut Vec<bool>, out: &mut String| {
        while let Some(ordered) = stack.pop() {
            out.push_str(if ordered {
                "</li></ol>\n"
            } else {
                "</li></ul>\n"
            });
        }
    };
    for page in pages {
        for block in &page.blocks {
            if !matches!(block, Block::ListItem { .. }) {
                close_lists(&mut stack, &mut out);
            }
            match block {
                Block::Heading { level, spans, .. } => {
                    let l = (*level).clamp(1, 6);
                    out.push_str(&format!("<h{l}>{}</h{l}>\n", spans_html(spans, true)));
                }
                Block::Paragraph { spans, .. } => {
                    out.push_str(&format!("<p>{}</p>\n", spans_html(spans, false)));
                }
                Block::ListItem {
                    ordered,
                    level,
                    spans,
                    ..
                } => {
                    let depth = (usize::from(*level) + 1).min(stack.len() + 1);
                    while stack.len() > depth {
                        let o = stack.pop().unwrap_or(false);
                        out.push_str(if o { "</li></ol>\n" } else { "</li></ul>\n" });
                    }
                    if stack.len() == depth {
                        if stack.last() == Some(ordered) {
                            out.push_str("</li>\n");
                        } else {
                            let o = stack.pop().unwrap_or(false);
                            out.push_str(if o { "</li></ol>\n" } else { "</li></ul>\n" });
                        }
                    }
                    while stack.len() < depth {
                        stack.push(*ordered);
                        out.push_str(if *ordered { "<ol>\n" } else { "<ul>\n" });
                    }
                    out.push_str(&format!("<li>{}", spans_html(spans, false)));
                }
                Block::Table(t) => out.push_str(&table_html(t)),
                Block::Picture(p) => {
                    if let Some(src) = picture(page.number, p) {
                        out.push_str(&format!(
                            "<p><img src=\"{}\" alt=\"Picture on page {}\"></p>\n",
                            escape_html(&src),
                            page.number
                        ));
                    }
                }
            }
        }
    }
    close_lists(&mut stack, &mut out);
    out.push_str("</body>\n</html>\n");
    out
}

// ----- CSV ---------------------------------------------------------------------------------

fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_owned()
    }
}

/// A table as CSV (RFC 4180): fields with commas, quotes or line breaks are quoted, rows end with
/// CR LF.
pub fn csv(table: &Table) -> String {
    let mut out = String::new();
    for row in &table.rows {
        let fields: Vec<String> = row.iter().map(|c| csv_field(&c.text)).collect();
        out.push_str(&fields.join(","));
        out.push_str("\r\n");
    }
    out
}

/// Standard base64 with padding, for pictures inside a web page.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(chunk.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(chunk.get(2).copied().unwrap_or(0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The text of a block, for places that need it without styles.
pub fn block_text(block: &Block) -> String {
    match block {
        Block::Heading { spans, .. }
        | Block::Paragraph { spans, .. }
        | Block::ListItem { spans, .. } => plain(spans),
        Block::Table(t) => t
            .rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|c| c.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\t")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Picture(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::model::{Cell, Rect};

    fn span(text: &str, bold: bool, italic: bool) -> Span {
        Span {
            text: text.to_owned(),
            bold,
            italic,
            size: 11.0,
        }
    }

    fn rect() -> Rect {
        Rect::new(0.0, 0.0, 10.0, 10.0)
    }

    fn table() -> Table {
        let cell = |t: &str| Cell {
            text: t.to_owned(),
            bold: false,
        };
        Table {
            rect: rect(),
            rows: vec![
                vec![cell("Name"), cell("Note")],
                vec![cell("a|b"), cell("say \"hi\", ok")],
            ],
            column_edges: vec![0.0, 5.0],
            row_edges: vec![0.0, 5.0],
        }
    }

    fn page(blocks: Vec<Block>) -> PageLayout {
        PageLayout {
            number: 1,
            width: 612.0,
            height: 792.0,
            blocks,
        }
    }

    fn no_pictures(_: u32, _: &PictureBox) -> Option<String> {
        None
    }

    #[test]
    fn markdown_has_headings_emphasis_and_escapes() {
        let pages = vec![page(vec![
            Block::Heading {
                level: 2,
                spans: vec![span("Results", true, false)],
                rect: rect(),
            },
            Block::Paragraph {
                spans: vec![
                    span("Plain ", false, false),
                    span("bold ", true, false),
                    span("1*2", false, true),
                ],
                rect: rect(),
            },
        ])];
        let md = markdown(&pages, &no_pictures);
        assert_eq!(
            md,
            "## Results\n\nPlain **bold** *1\\*2*\n\n"
                .trim_end()
                .to_owned()
                + "\n"
        );
    }

    #[test]
    fn markdown_keeps_a_paragraph_that_looks_like_markdown_literal() {
        let pages = vec![page(vec![
            Block::Paragraph {
                spans: vec![span("# not a heading", false, false)],
                rect: rect(),
            },
            Block::Paragraph {
                spans: vec![span("1. not a list", false, false)],
                rect: rect(),
            },
        ])];
        let md = markdown(&pages, &no_pictures);
        assert!(md.contains("\\# not a heading"));
        assert!(md.contains("\\1. not a list"));
    }

    #[test]
    fn markdown_lists_nest_and_number() {
        let item = |ordered: bool, marker: &str, level: u8, text: &str| Block::ListItem {
            ordered,
            marker: marker.to_owned(),
            level,
            spans: vec![span(text, false, false)],
            rect: rect(),
        };
        let pages = vec![page(vec![
            item(false, "", 0, "one"),
            item(false, "", 1, "nested"),
            Block::Paragraph {
                spans: vec![span("after", false, false)],
                rect: rect(),
            },
            item(true, "3.", 0, "three"),
            item(true, "4.", 0, "four"),
        ])];
        let md = markdown(&pages, &no_pictures);
        assert_eq!(md, "- one\n  - nested\n\nafter\n\n3. three\n4. four\n");
    }

    #[test]
    fn markdown_tables_are_pipe_tables_with_escaped_pipes() {
        let md = markdown(&[page(vec![Block::Table(table())])], &no_pictures);
        assert_eq!(
            md,
            "| Name | Note |\n| --- | --- |\n| a\\|b | say \"hi\", ok |\n"
        );
    }

    #[test]
    fn markdown_pictures_use_the_given_address_or_are_left_out() {
        let blocks = vec![Block::Picture(PictureBox {
            rect: rect(),
            index: 3,
        })];
        let with = markdown(&[page(blocks.clone())], &|_, p| {
            Some(format!("img{}.png", p.index))
        });
        assert!(with.contains("![Picture on page 1](img3.png)"));
        let without = markdown(&[page(blocks)], &no_pictures);
        assert_eq!(without.trim(), "");
    }

    #[test]
    fn html_escapes_text_and_marks_up_styles() {
        let pages = vec![page(vec![
            Block::Heading {
                level: 1,
                spans: vec![span("A & B", true, false)],
                rect: rect(),
            },
            Block::Paragraph {
                spans: vec![
                    span("x < y ", false, false),
                    span("strong", true, false),
                    span("slant", false, true),
                ],
                rect: rect(),
            },
        ])];
        let out = html(&pages, "T <1>", &no_pictures);
        assert!(out.contains("<title>T &lt;1&gt;</title>"));
        assert!(out.contains("<h1>A &amp; B</h1>"));
        assert!(out.contains("<p>x &lt; y <strong>strong</strong><em>slant</em></p>"));
        assert!(!out.contains("<script"));
    }

    #[test]
    fn html_lists_nest_properly() {
        let item = |level: u8, text: &str| Block::ListItem {
            ordered: false,
            marker: String::new(),
            level,
            spans: vec![span(text, false, false)],
            rect: rect(),
        };
        let out = html(
            &[page(vec![item(0, "a"), item(1, "b"), item(0, "c")])],
            "t",
            &no_pictures,
        );
        assert!(
            out.contains("<ul>\n<li>a<ul>\n<li>b</li></ul>\n</li>\n<li>c</li></ul>\n"),
            "{out}"
        );
    }

    #[test]
    fn html_tables_have_a_header_row() {
        let out = html(&[page(vec![Block::Table(table())])], "t", &no_pictures);
        assert!(out.contains("<tr><th>Name</th><th>Note</th></tr>"));
        assert!(out.contains("<tr><td>a|b</td><td>say &quot;hi&quot;, ok</td></tr>"));
    }

    #[test]
    fn base64_matches_the_standard_test_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected);
        }
    }

    #[test]
    fn csv_quotes_where_needed_and_ends_rows_with_crlf() {
        assert_eq!(csv(&table()), "Name,Note\r\na|b,\"say \"\"hi\"\", ok\"\r\n");
    }
}
