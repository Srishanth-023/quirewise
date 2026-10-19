//! End-to-end tests for PDF to Word, PowerPoint, Excel, Markdown and HTML: the real RunTool, the
//! worker engines and the file store, run on generated PDFs. Where Microsoft Office is installed
//! the written files are opened by it again (converted back to PDF) to prove they are real
//! documents; those checks do nothing on a computer without Office. The OCR tests need Tesseract.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use application::ports::{OfficeConverter, OfficeKind, PictureSpec, PictureWriter, Renderer};
use application::{InputDoc, OutputArtifact, Password};
use common::{Lab, name};
use domain::layout::Detection;
use domain::tools::common::Region;
use domain::tools::convert::{PictureColor, PictureFormat};
use domain::tools::export::{
    ExportOcr, PdfToExcelOptions, PdfToHtmlOptions, PdfToMarkdownOptions, PdfToPowerPointOptions,
    PdfToWordOptions, SheetFormat, SheetGrouping, SlideMode, WordLayout,
};
use domain::tools::office::{
    ExcelToPdfOptions, OfficeEngine, PowerPointToPdfOptions, WordToPdfOptions,
};
use domain::{Dpi, ErrorCode, PageIndex, ToolId, ToolOptions};
use quirewise_engine_office_msoffice::MsOffice;
use quirewise_engine_raster::RasterConverter;
use std::fs;
use std::io::Read;
use std::path::Path;
use test_support::{encrypt_pdf, pdfs};

fn have(kind: OfficeKind) -> bool {
    MsOffice::new().is_available(kind)
}

/// A page with a heading, a paragraph, a bullet list and a ruled table.
fn report() -> Vec<u8> {
    let mut content = String::new();
    content.push_str("BT /F2 24 Tf 72 700 Td (Annual Report) Tj ET\n");
    content.push_str(
        "BT /F1 11 Tf 72 660 Td (The year began with steady growth in every region and) Tj ET\n",
    );
    content
        .push_str("BT /F1 11 Tf 72 646 Td (continued that way until the autumn quarter.) Tj ET\n");
    content.push_str("BT /F1 11 Tf 72 610 Td (- First point) Tj ET\n");
    content.push_str("BT /F1 11 Tf 72 596 Td (- Second point) Tj ET\n");
    // A table of two rows and two columns, drawn with lines.
    for y in [500, 470, 440] {
        content.push_str(&format!("0.5 w 72 {y} m 272 {y} l S\n"));
    }
    for x in [72, 172, 272] {
        content.push_str(&format!("0.5 w {x} 440 m {x} 500 l S\n"));
    }
    content.push_str("BT /F2 11 Tf 80 481 Td (Item) Tj ET\nBT /F2 11 Tf 180 481 Td (Qty) Tj ET\n");
    content.push_str("BT /F1 11 Tf 80 451 Td (Pens) Tj ET\nBT /F1 11 Tf 180 451 Td (12) Tj ET\n");
    pdfs::pdf_from_content(&content, 0)
}

/// The report with a picture under the table.
fn report_with_picture() -> Vec<u8> {
    let mut content = String::from("q 100 0 0 100 72 300 cm /Im1 Do Q\n");
    content.push_str("BT /F2 24 Tf 72 700 Td (Annual Report) Tj ET\n");
    content.push_str(
        "BT /F1 11 Tf 72 660 Td (The year began with steady growth in every region and) Tj ET\n",
    );
    content
        .push_str("BT /F1 11 Tf 72 646 Td (continued that way until the autumn quarter.) Tj ET\n");
    content.push_str("BT /F1 11 Tf 72 610 Td (- First point) Tj ET\n");
    content.push_str("BT /F1 11 Tf 72 596 Td (- Second point) Tj ET\n");
    for y in [500, 470, 440] {
        content.push_str(&format!("0.5 w 72 {y} m 272 {y} l S\n"));
    }
    for x in [72, 172, 272] {
        content.push_str(&format!("0.5 w {x} 440 m {x} 500 l S\n"));
    }
    content.push_str("BT /F2 11 Tf 80 481 Td (Item) Tj ET\nBT /F2 11 Tf 180 481 Td (Qty) Tj ET\n");
    content.push_str("BT /F1 11 Tf 80 451 Td (Pens) Tj ET\nBT /F1 11 Tf 180 451 Td (12) Tj ET\n");
    pdfs::pdf_from_content(&content, 0)
}

fn part(path: &Path, name: &str) -> String {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut text = String::new();
    archive
        .by_name(name)
        .unwrap_or_else(|_| panic!("{name} is missing"))
        .read_to_string(&mut text)
        .unwrap();
    text
}

fn has_part(path: &Path, name: &str) -> bool {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    archive.by_name(name).is_ok()
}

fn word(f: impl FnOnce(&mut PdfToWordOptions)) -> ToolOptions {
    let mut o = PdfToWordOptions::default();
    f(&mut o);
    ToolOptions::PdfToWord(o)
}

fn slides(f: impl FnOnce(&mut PdfToPowerPointOptions)) -> ToolOptions {
    let mut o = PdfToPowerPointOptions::default();
    f(&mut o);
    ToolOptions::PdfToPowerpoint(o)
}

fn excel(f: impl FnOnce(&mut PdfToExcelOptions)) -> ToolOptions {
    let mut o = PdfToExcelOptions::default();
    f(&mut o);
    ToolOptions::PdfToExcel(o)
}

fn markdown(f: impl FnOnce(&mut PdfToMarkdownOptions)) -> ToolOptions {
    let mut o = PdfToMarkdownOptions::default();
    f(&mut o);
    ToolOptions::PdfToMarkdown(o)
}

fn html(f: impl FnOnce(&mut PdfToHtmlOptions)) -> ToolOptions {
    let mut o = PdfToHtmlOptions::default();
    f(&mut o);
    ToolOptions::PdfToHtml(o)
}

fn region(x: f32, y: f32, width: f32, height: f32) -> Region {
    Region {
        x,
        y,
        width,
        height,
    }
}

/// Opens a written Word file with Word and returns the PDF it makes.
fn through_word(lab: &Lab, docx: &OutputArtifact) -> OutputArtifact {
    lab.run(
        ToolOptions::WordToPdf(WordToPdfOptions {
            engine: OfficeEngine::MsOffice,
            ..WordToPdfOptions::default()
        }),
        InputDoc::new(&docx.path),
    )
    .unwrap()
    .remove(0)
}

// ---- Word ---------------------------------------------------------------------------------

#[test]
fn a_page_becomes_a_flowing_word_document_and_the_original_stays() {
    let lab = Lab::new();
    let source = lab.doc("report.pdf", &report());
    let before = fs::read(&source.path).unwrap();
    let out = lab.run(word(|_| {}), source.clone()).unwrap();
    assert_eq!(name(&out[0]), "report (word).docx");
    let doc = part(&out[0].path, "word/document.xml");
    assert!(doc.contains("<w:pStyle w:val=\"Heading1\"/>"), "{doc}");
    assert!(doc.contains("Annual Report"));
    // The two lines of the paragraph are one paragraph.
    assert!(
        doc.contains("steady growth in every region and continued that way"),
        "{doc}"
    );
    assert!(doc.contains("<w:numId w:val=\"1\"/>") && doc.contains("First point"));
    assert!(
        !doc.contains("- First point"),
        "the bullet became a list marker"
    );
    assert!(doc.contains("<w:tbl>") && doc.contains("Pens"));
    assert_eq!(fs::read(&source.path).unwrap(), before);
    lab.tidy();
}

#[test]
fn word_opens_the_flowing_document_and_finds_the_text() {
    if !have(OfficeKind::Word) {
        return;
    }
    let lab = Lab::new();
    let out = lab
        .run(word(|_| {}), lab.doc("report.pdf", &report()))
        .unwrap();
    let back = through_word(&lab, &out[0]);
    let text = lab.texts(&back.path, None).join("\n");
    for expected in [
        "Annual Report",
        "steady growth",
        "First point",
        "Second point",
        "Pens",
    ] {
        assert!(text.contains(expected), "{expected} in {text:?}");
    }
    lab.tidy();
}

#[test]
fn exact_layout_keeps_blocks_where_they_were_when_word_opens_it() {
    if !have(OfficeKind::Word) {
        return;
    }
    let lab = Lab::new();
    let out = lab
        .run(
            word(|o| o.layout = WordLayout::Exact),
            lab.doc("report.pdf", &report()),
        )
        .unwrap();
    let back = through_word(&lab, &out[0]);
    assert_eq!(back.pages, Some(1));
    // The heading is in the top strip and the table sits between 35% and 47% of the page height.
    let top = lab.text_in(&back.path, 0, region(0.0, 0.0, 1.0, 0.2));
    assert!(top.contains("Annual Report"), "{top:?}");
    let band = lab.text_in(&back.path, 0, region(0.0, 0.33, 0.6, 0.17));
    assert!(band.contains("Pens") && band.contains("Item"), "{band:?}");
    let lower = lab.text_in(&back.path, 0, region(0.0, 0.6, 1.0, 0.4));
    assert!(lower.trim().is_empty(), "{lower:?}");
    lab.tidy();
}

#[test]
fn only_the_pages_asked_for_are_converted_and_bad_pages_are_refused() {
    let lab = Lab::new();
    let source = lab.doc(
        "three.pdf",
        &pdfs::pdf_with_texts(&[
            "Alpha page text here",
            "Bravo page text here",
            "Charlie page text here",
        ]),
    );
    let out = lab
        .run(
            word(|o| o.pages = domain::PageRange::parse("2-3").unwrap()),
            source.clone(),
        )
        .unwrap();
    let doc = part(&out[0].path, "word/document.xml");
    assert!(!doc.contains("Alpha") && doc.contains("Bravo") && doc.contains("Charlie"));
    let err = lab
        .run(
            word(|o| o.pages = domain::PageRange::parse("9").unwrap()),
            source,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::RangeOutOfBounds);
    lab.tidy();
}

// ---- PowerPoint ---------------------------------------------------------------------------

#[test]
fn editable_slides_hold_text_boxes_a_table_and_a_picture() {
    let lab = Lab::new();
    let out = lab
        .run(slides(|_| {}), lab.doc("deck.pdf", &report_with_picture()))
        .unwrap();
    assert_eq!(name(&out[0]), "deck (powerpoint).pptx");
    let slide = part(&out[0].path, "ppt/slides/slide1.xml");
    assert!(
        slide.contains("Annual Report") && slide.contains("<a:tbl>") && slide.contains("<p:pic>")
    );
    assert!(slide.contains("<a:buChar"));
    assert!(has_part(&out[0].path, "ppt/media/image1.png"));
    lab.tidy();
}

#[test]
fn powerpoint_opens_the_slides_and_finds_one_per_page() {
    if !have(OfficeKind::Slides) {
        return;
    }
    let lab = Lab::new();
    let source = lab.doc(
        "two.pdf",
        &pdfs::pdf_with_texts(&["First slide words", "Second slide words"]),
    );
    let out = lab.run(slides(|_| {}), source).unwrap();
    let back = lab
        .run(
            ToolOptions::PowerpointToPdf(PowerPointToPdfOptions {
                engine: OfficeEngine::MsOffice,
                ..PowerPointToPdfOptions::default()
            }),
            InputDoc::new(&out[0].path),
        )
        .unwrap();
    assert_eq!(back[0].pages, Some(2));
    let texts = lab.texts(&back[0].path, None);
    assert!(
        texts[0].contains("First slide") && texts[1].contains("Second slide"),
        "{texts:?}"
    );
    lab.tidy();
}

#[test]
fn image_slides_are_one_picture_per_page() {
    let lab = Lab::new();
    let source = lab.doc("two.pdf", &pdfs::pdf_with_texts(&["One", "Two"]));
    let out = lab
        .run(slides(|o| o.mode = SlideMode::ImageSlides), source)
        .unwrap();
    for n in 1..=2 {
        let slide = part(&out[0].path, &format!("ppt/slides/slide{n}.xml"));
        assert!(slide.contains("<p:pic>") && !slide.contains("<p:sp>"));
        assert!(has_part(&out[0].path, &format!("ppt/media/image{n}.jpg")));
    }
    lab.tidy();
}

// ---- Excel --------------------------------------------------------------------------------

#[test]
fn a_ruled_table_becomes_a_sheet_with_numbers_as_numbers() {
    let lab = Lab::new();
    let out = lab
        .run(excel(|_| {}), lab.doc("report.pdf", &report()))
        .unwrap();
    assert_eq!(name(&out[0]), "report (excel).xlsx");
    let book = part(&out[0].path, "xl/workbook.xml");
    assert!(book.contains("name=\"Page 1\""), "{book}");
    let sheet = part(&out[0].path, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains(">Item<") && sheet.contains(">Pens<"),
        "{sheet}"
    );
    assert!(sheet.contains("<v>12</v>"), "{sheet}");
    lab.tidy();
}

#[test]
fn excel_opens_the_workbook_and_finds_the_cells() {
    if !have(OfficeKind::Sheet) {
        return;
    }
    let lab = Lab::new();
    let out = lab
        .run(excel(|_| {}), lab.doc("report.pdf", &report()))
        .unwrap();
    let back = lab
        .run(
            ToolOptions::ExcelToPdf(ExcelToPdfOptions {
                engine: OfficeEngine::MsOffice,
                ..ExcelToPdfOptions::default()
            }),
            InputDoc::new(&out[0].path),
        )
        .unwrap();
    let text = lab.texts(&back[0].path, None).join("\n");
    assert!(
        text.contains("Item") && text.contains("Pens") && text.contains("12"),
        "{text:?}"
    );
    lab.tidy();
}

#[test]
fn csv_files_have_a_byte_order_mark_and_one_file_per_table() {
    let lab = Lab::new();
    let out = lab
        .run(
            excel(|o| o.format = SheetFormat::Csv),
            lab.doc("report.pdf", &report()),
        )
        .unwrap();
    assert_eq!(out.len(), 1);
    assert!(name(&out[0]).ends_with(".csv"), "{}", name(&out[0]));
    let bytes = fs::read(&out[0].path).unwrap();
    assert_eq!(&bytes[..3], [0xEF, 0xBB, 0xBF]);
    assert_eq!(
        String::from_utf8(bytes[3..].to_vec()).unwrap(),
        "Item,Qty\r\nPens,12\r\n"
    );
    lab.tidy();
}

#[test]
fn a_page_of_plain_text_has_no_tables_and_says_so() {
    let lab = Lab::new();
    let source = lab.doc(
        "prose.pdf",
        &pdfs::pdf_with_texts(&["Just one line of prose on the page"]),
    );
    let err = lab.run(excel(|_| {}), source).unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(err.message.contains("No tables"), "{}", err.message);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}

#[test]
fn text_aligned_in_columns_is_found_without_lines_and_lattice_mode_ignores_it() {
    let lab = Lab::new();
    let mut content = String::new();
    for (i, (a, b, c)) in [
        ("Region", "Q1", "Q2"),
        ("North", "120", "140"),
        ("South", "95", "101"),
        ("East", "80", "77"),
    ]
    .iter()
    .enumerate()
    {
        let y = 600 - 20 * i;
        content.push_str(&format!("BT /F1 11 Tf 72 {y} Td ({a}) Tj ET\nBT /F1 11 Tf 220 {y} Td ({b}) Tj ET\nBT /F1 11 Tf 340 {y} Td ({c}) Tj ET\n"));
    }
    let source = lab.doc("aligned.pdf", &pdfs::pdf_from_content(&content, 0));
    let out = lab.run(excel(|_| {}), source.clone()).unwrap();
    let sheet = part(&out[0].path, "xl/worksheets/sheet1.xml");
    assert!(
        sheet.contains(">Region<") && sheet.contains("<v>140</v>"),
        "{sheet}"
    );
    let err = lab
        .run(excel(|o| o.detection = Detection::Lattice), source)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    lab.tidy();
}

#[test]
fn per_page_grouping_stacks_the_tables_of_a_page_on_one_sheet() {
    let lab = Lab::new();
    let out = lab
        .run(
            excel(|o| o.grouping = SheetGrouping::PerPage),
            lab.doc("report.pdf", &report()),
        )
        .unwrap();
    assert!(has_part(&out[0].path, "xl/worksheets/sheet1.xml"));
    assert!(!has_part(&out[0].path, "xl/worksheets/sheet2.xml"));
    lab.tidy();
}

// ---- Markdown and HTML --------------------------------------------------------------------

#[test]
fn markdown_has_headings_lists_tables_and_saves_the_pictures_beside_it() {
    let lab = Lab::new();
    let out = lab
        .run(
            markdown(|_| {}),
            lab.doc("report.pdf", &report_with_picture()),
        )
        .unwrap();
    assert_eq!(out.len(), 2, "the file and its picture");
    assert_eq!(name(&out[0]), "report (markdown).md");
    let text = fs::read_to_string(&out[0].path).unwrap();
    assert!(text.starts_with("# Annual Report\n\n"), "{text}");
    assert!(
        text.contains("steady growth in every region and continued that way"),
        "{text}"
    );
    assert!(text.contains("- First point\n- Second point\n"), "{text}");
    assert!(
        text.contains("| Item | Qty |\n| --- | --- |\n| Pens | 12 |"),
        "{text}"
    );
    let picture = name(&out[1]);
    assert!(
        text.contains(&format!(
            "![Picture on page 1]({})",
            picture
                .replace(' ', "%20")
                .replace('(', "%28")
                .replace(')', "%29")
        )),
        "{text}"
    );
    assert!(
        fs::read(&out[1].path)
            .unwrap()
            .starts_with(&[0x89, b'P', b'N', b'G'])
    );

    let plain = lab
        .run(
            markdown(|o| o.images = false),
            lab.doc("again.pdf", &report_with_picture()),
        )
        .unwrap();
    assert_eq!(plain.len(), 1);
    assert!(!fs::read_to_string(&plain[0].path).unwrap().contains("!["));
    lab.tidy();
}

#[test]
fn html_is_one_file_with_its_pictures_inside_or_linked() {
    let lab = Lab::new();
    let source = lab.doc("report.pdf", &report_with_picture());
    let one = lab.run(html(|_| {}), source.clone()).unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(name(&one[0]), "report (html).html");
    let text = fs::read_to_string(&one[0].path).unwrap();
    assert!(
        text.contains("<h1>Annual Report</h1>")
            && text.contains("<table>")
            && text.contains("<ul>"),
        "{text}"
    );
    assert!(text.contains("<img src=\"data:image/png;base64,"), "{text}");
    assert!(!text.contains("<script"));

    let linked = lab.run(html(|o| o.inline_images = false), source).unwrap();
    assert_eq!(linked.len(), 2);
    assert!(
        fs::read_to_string(&linked[0].path)
            .unwrap()
            .contains(&format!(
                "<img src=\"{}\"",
                name(&linked[1])
                    .replace(' ', "%20")
                    .replace('(', "%28")
                    .replace(')', "%29")
            ))
    );
    lab.tidy();
}

#[test]
fn a_protected_file_converts_with_its_password() {
    let lab = Lab::new();
    let plain = lab.put("a.pdf", &report());
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "pw");
    let mut input = InputDoc::new(&locked);
    let err = lab.run(markdown(|_| {}), input.clone()).unwrap_err();
    assert!(
        matches!(
            err.code,
            ErrorCode::PdfEncrypted | ErrorCode::PdfWrongPassword
        ),
        "{:?}",
        err.code
    );
    input.password = Some(Password::new("pw"));
    let out = lab.run(markdown(|_| {}), input).unwrap();
    assert!(
        fs::read_to_string(&out[0].path)
            .unwrap()
            .contains("# Annual Report")
    );
    lab.tidy();
}

#[test]
fn running_headers_and_page_numbers_do_not_end_up_in_the_text() {
    let lab = Lab::new();
    let pages: Vec<String> = (1..=4)
        .map(|n| {
            format!(
                "BT /F1 9 Tf 72 765 Td (ACME Annual Report) Tj ET
BT /F1 11 Tf 72 500 Td (Body of page {n} with real content to keep here) Tj ET
BT /F1 9 Tf 300 30 Td (Page {n}) Tj ET
"
            )
        })
        .collect();
    let refs: Vec<&str> = pages.iter().map(String::as_str).collect();
    let source = lab.doc("four.pdf", &pdfs::pdf_from_pages(&refs, 0));
    let out = lab.run(markdown(|_| {}), source).unwrap();
    let text = fs::read_to_string(&out[0].path).unwrap();
    assert!(!text.contains("ACME"), "{text}");
    assert!(
        !text.contains("Page 1") && !text.contains("Page 4"),
        "{text}"
    );
    for n in 1..=4 {
        assert!(
            text.contains(&format!("Body of page {n} with real content")),
            "{text}"
        );
    }
    // Two pages are too few to tell a header from text, so they are kept.
    let two = lab.doc("two.pdf", &pdfs::pdf_from_pages(&refs[..2], 0));
    let kept = lab.run(markdown(|_| {}), two).unwrap();
    assert!(fs::read_to_string(&kept[0].path).unwrap().contains("ACME"));
    lab.tidy();
}

// ---- OCR ----------------------------------------------------------------------------------

/// A scanned document: a page of text rendered to a picture and put in a PDF that has no text.
fn scanned(lab: &Lab, text: &str) -> InputDoc {
    let source = lab.put("text.pdf", &pdfs::pdf_with_texts(&[text]));
    let image = lab
        .engine
        .render_page(&source, None, PageIndex::new(0), Dpi::new(300).unwrap())
        .unwrap();
    let png = lab.dir.path().join("scan.png");
    RasterConverter
        .write_picture(
            &image,
            &PictureSpec {
                format: PictureFormat::Png,
                jpeg_quality: 90,
                color: PictureColor::Gray,
                dpi: Dpi::new(300).unwrap(),
            },
            &png,
        )
        .unwrap();
    lab.run(
        ToolOptions::ScanToPdf(domain::tools::recognize::ScanToPdfOptions::default()),
        InputDoc::new(&png),
    )
    .unwrap();
    let made = fs::read_dir(lab.out())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "pdf"))
        .unwrap();
    let kept = lab.dir.path().join("scan.pdf");
    fs::rename(made, &kept).unwrap();
    InputDoc::new(kept)
}

#[test]
fn a_scanned_page_is_read_and_becomes_editable_text() {
    let lab = Lab::new();
    if !lab.ocr_ready("eng") {
        return;
    }
    let scan = scanned(
        &lab,
        "Invoice number 48213 for the quarterly delivery of paper",
    );
    assert!(lab.texts(&scan.path, None)[0].is_empty());
    let out = lab.run(word(|_| {}), scan.clone()).unwrap();
    let doc = part(&out[0].path, "word/document.xml").to_lowercase();
    assert!(doc.contains("invoice") && doc.contains("48213"), "{doc}");
    // The picture of the scan is not repeated behind the text.
    assert!(!doc.contains("<wp:inline"), "{doc}");

    let md = lab.run(markdown(|_| {}), scan.clone()).unwrap();
    assert!(
        fs::read_to_string(&md[0].path)
            .unwrap()
            .to_lowercase()
            .contains("invoice")
    );

    // With OCR off the page stays a picture.
    let off = lab.run(word(|o| o.ocr = ExportOcr::Off), scan).unwrap();
    let doc = part(&off[0].path, "word/document.xml");
    assert!(doc.contains("<wp:inline") && !doc.to_lowercase().contains("invoice"));
    lab.tidy();
}

#[test]
fn a_searchable_scan_is_converted_by_its_text_without_repeating_the_scan() {
    let lab = Lab::new();
    if !lab.ocr_ready("eng") {
        return;
    }
    let scan = scanned(
        &lab,
        "Invoice number 48213 for the quarterly delivery of paper",
    );
    let searchable = lab
        .run(
            ToolOptions::Ocr(domain::tools::recognize::OcrOptions::default()),
            scan,
        )
        .unwrap();
    let out = lab
        .run(word(|_| {}), InputDoc::new(&searchable[0].path))
        .unwrap();
    let doc = part(&out[0].path, "word/document.xml").to_lowercase();
    assert!(doc.contains("invoice") && doc.contains("48213"), "{doc}");
    assert!(
        !doc.contains("<wp:inline"),
        "the scan is not repeated behind the text"
    );
    lab.tidy();
}

#[test]
fn a_scanned_page_without_the_recognizer_says_which_component_is_missing() {
    let lab = Lab::new();
    if lab.ocr_ready("eng") {
        return;
    }
    let scan = scanned(&lab, "Some words on a scanned page of the report");
    let err = lab.run(word(|_| {}), scan).unwrap_err();
    assert_eq!(err.code, ErrorCode::ComponentMissing);
    assert_eq!(
        fs::read_dir(lab.out())
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "docx"))
            .count(),
        0
    );
}

#[test]
fn the_tools_are_listed_with_their_ids() {
    for id in [
        ToolId::PdfToWord,
        ToolId::PdfToPowerpoint,
        ToolId::PdfToExcel,
        ToolId::PdfToMarkdown,
        ToolId::PdfToHtml,
    ] {
        assert!(ToolId::ALL.contains(&id));
    }
    assert_eq!(ToolId::PdfToPowerpoint.as_str(), "pdf-to-powerpoint");
    let _ = Dpi::new(150);
}
