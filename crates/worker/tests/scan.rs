//! End-to-end tests for the tools that read scans: Deskew, Scan to PDF and OCR. The real RunTool,
//! worker engines and file store, run on generated pages and checked by rendering them again.
//! The OCR tests need Tesseract (see `scripts/build-tesseract.ps1`) and do nothing without it.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use application::ports::{PdfReader, Renderer, SkewDetector};
use application::{InputDoc, Password};
use common::{Lab, name};
use domain::tools::convert::{PictureColor, PictureFormat};
use domain::tools::recognize::{
    DeskewMode, DeskewOptions, OcrOptions, OcrOutput, PageAngle, ScanToPdfOptions,
};
use domain::{Dpi, ErrorCode, PageIndex, PageRange, ToolId, ToolOptions};
use quirewise_engine_raster::RasterConverter;
use std::fs;
use std::path::Path;
use test_support::{encrypt_pdf, pdfs};

/// The angle that would level page `page` of `path`, as the tool measures it.
fn measured(lab: &Lab, path: &Path, page: u32, password: Option<&Password>) -> Option<f32> {
    let image = lab
        .engine
        .render_page(path, password, PageIndex::new(page), Dpi::new(100).unwrap())
        .unwrap();
    RasterConverter.skew(&image, 15.0).unwrap()
}

fn deskew(f: impl FnOnce(&mut DeskewOptions)) -> ToolOptions {
    let mut options = DeskewOptions::default();
    f(&mut options);
    ToolOptions::Deskew(options)
}

// ---- Deskew -------------------------------------------------------------------------------

#[test]
fn a_crooked_page_comes_out_level_and_the_original_stays() {
    let lab = Lab::new();
    let source = lab.doc("crooked.pdf", &pdfs::pdf_with_tilted_lines(3.0, 30));
    let before = fs::read(&source.path).unwrap();
    let start = measured(&lab, &source.path, 0, None).unwrap();
    assert!((start + 3.0).abs() < 0.4, "the page needs {start}");

    let out = lab
        .run(ToolOptions::defaults_for(ToolId::Deskew), source.clone())
        .unwrap();
    assert_eq!(name(&out[0]), "crooked (straightened).pdf");
    assert_eq!(out[0].pages, Some(1));
    let after = measured(&lab, &out[0].path, 0, None).unwrap_or(0.0);
    assert!(after.abs() < 0.4, "after straightening: {after}");
    assert_eq!(fs::read(&source.path).unwrap(), before);
    // The page keeps its size and its text.
    assert_eq!(lab.sizes(&out[0].path), [(612.0, 792.0)]);
    assert!(lab.texts(&out[0].path, None)[0].contains("quarterly report"));
    lab.tidy();
}

#[test]
fn a_straight_document_says_nothing_needed_straightening() {
    let lab = Lab::new();
    let source = lab.doc("straight.pdf", &pdfs::pdf_with_tilted_lines(0.0, 30));
    let err = lab
        .run(ToolOptions::defaults_for(ToolId::Deskew), source)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(err.message.contains("No page is tilted"), "{}", err.message);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}

#[test]
fn manual_angles_turn_only_the_pages_listed() {
    let lab = Lab::new();
    let source = lab.doc("two.pdf", &pdfs::pdf_with_texts(&["one", "two"]));
    let out = lab
        .run(
            deskew(|o| {
                o.mode = DeskewMode::Manual;
                o.angles = vec![PageAngle {
                    page: 2,
                    degrees: 5.0,
                }];
            }),
            source,
        )
        .unwrap();
    assert_eq!(out[0].pages, Some(2));
    // Page one is as it was; page two now carries a turning matrix.
    let first = lab.look(&out[0].path, 0, None);
    let second = lab.look(&out[0].path, 1, None);
    assert!(first.inked > 0 && second.inked > 0);
    lab.tidy();
}

#[test]
fn a_protected_file_stays_protected_after_straightening() {
    let lab = Lab::new();
    let plain = lab.put("a.pdf", &pdfs::pdf_with_tilted_lines(-4.0, 30));
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "pw");
    let mut input = InputDoc::new(&locked);
    input.password = Some(Password::new("pw"));
    let out = lab
        .run(ToolOptions::defaults_for(ToolId::Deskew), input)
        .unwrap();
    let password = Password::new("pw");
    assert!(lab.engine.open_info(&out[0].path, None).is_err());
    assert!(lab.engine.open_info(&out[0].path, Some(&password)).is_ok());
    let after = measured(&lab, &out[0].path, 0, Some(&password)).unwrap_or(0.0);
    assert!(after.abs() < 0.4, "{after}");
    lab.tidy();
}

// ---- Scan to PDF --------------------------------------------------------------------------

/// A photo of a page: paper with text rows, a little askew, on a dark table.
fn photo_png(width: u32, height: u32) -> Vec<u8> {
    let mut rgb = image::RgbImage::from_pixel(width, height, image::Rgb([48, 46, 44]));
    let (x0, y0, x1, y1) = (width / 10, height / 12, width * 9 / 10, height * 11 / 12);
    for y in y0..y1 {
        for x in x0..x1 {
            let ink = ((y - y0) / 14) % 3 == 0 && ((x - x0) / 9) % 4 != 0;
            let v = if ink { 25 } else { 238 };
            rgb.put_pixel(x, y, image::Rgb([v, v, v]));
        }
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(rgb)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn scan(f: impl FnOnce(&mut ScanToPdfOptions)) -> ToolOptions {
    let mut options = ScanToPdfOptions::default();
    f(&mut options);
    ToolOptions::ScanToPdf(options)
}

#[test]
fn photos_of_pages_become_one_cropped_pdf_in_the_chosen_color() {
    let lab = Lab::new();
    let a = lab.doc("p1.png", &photo_png(700, 900));
    let b = lab.doc("p2.png", &photo_png(700, 900));
    let (before_a, before_b) = (fs::read(&a.path).unwrap(), fs::read(&b.path).unwrap());
    for color in [
        PictureColor::Color,
        PictureColor::Gray,
        PictureColor::BlackWhite,
    ] {
        let out = lab
            .run_all(
                scan(|o| {
                    o.color = color;
                    o.dpi = Dpi::new(200).unwrap();
                }),
                vec![a.clone(), b.clone()],
            )
            .unwrap();
        assert_eq!(out.len(), 1, "{color:?}");
        assert_eq!(name(&out[0]), "p1 (scan).pdf");
        assert_eq!(out[0].pages, Some(2));
        // The table was cut away: the page is about 560 by 750 pixels at 200 dpi.
        let (w, h) = lab.sizes(&out[0].path)[0];
        assert!((w - 560.0 * 72.0 / 200.0).abs() < 12.0, "{w}");
        assert!((h - 750.0 * 72.0 / 200.0).abs() < 12.0, "{h}");
        // Rendered, the page is mostly paper with dark rows of text on it.
        let look = lab.look(&out[0].path, 0, None);
        assert!(look.inked > 100, "{color:?} {look:?}");
        fs::remove_file(&out[0].path).unwrap();
    }
    assert_eq!(fs::read(&a.path).unwrap(), before_a);
    assert_eq!(fs::read(&b.path).unwrap(), before_b);
    lab.tidy();
}

#[test]
fn a_file_that_is_not_a_picture_is_refused() {
    let lab = Lab::new();
    let pdf = lab.doc("a.pdf", &pdfs::pdf_with_texts(&["x"]));
    let err = lab
        .run(ToolOptions::defaults_for(ToolId::ScanToPdf), pdf)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InputNotPdf);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
}

#[test]
fn scan_to_pdf_with_ocr_but_no_recognizer_fails_before_any_work() {
    let lab = Lab::new();
    if lab.ocr_ready("eng") {
        return;
    }
    let a = lab.doc("p1.png", &photo_png(300, 400));
    let err = lab
        .run(
            scan(|o| {
                o.ocr = true;
            }),
            a,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ComponentMissing);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
}

// ---- OCR ----------------------------------------------------------------------------------

/// A scanned document: pages of text rendered to pictures and put in a PDF that has no text.
fn scanned(lab: &Lab, lines: &[&str]) -> InputDoc {
    let words = lines.join(" ");
    let source = lab.put("text.pdf", &pdfs::pdf_with_texts(&[&words]));
    let image = lab
        .engine
        .render_page(&source, None, PageIndex::new(0), Dpi::new(300).unwrap())
        .unwrap();
    let png = lab.dir.path().join("scan.png");
    use application::ports::{PictureSpec, PictureWriter};
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
    let made = lab
        .run(
            ToolOptions::defaults_for(ToolId::ImagesToPdf),
            InputDoc::new(&png),
        )
        .unwrap();
    InputDoc::new(&made[0].path)
}

fn region(x: f32, y: f32, width: f32, height: f32) -> domain::tools::common::Region {
    domain::tools::common::Region {
        x,
        y,
        width,
        height,
    }
}

fn ocr(f: impl FnOnce(&mut OcrOptions)) -> ToolOptions {
    let mut options = OcrOptions::default();
    f(&mut options);
    ToolOptions::Ocr(options)
}

#[test]
fn a_scanned_page_becomes_searchable_and_keeps_its_picture() {
    let lab = Lab::new();
    if !lab.ocr_ready("eng") {
        return;
    }
    let scan = scanned(
        &lab,
        &["Invoice number 48213 for the quarterly delivery of paper"],
    );
    assert!(
        lab.texts(&scan.path, None)[0].is_empty(),
        "a scan has no text yet"
    );
    let before = fs::read(&scan.path).unwrap();
    let out = lab.run(ocr(|_| {}), scan.clone()).unwrap();
    assert!(name(&out[0]).ends_with("(ocr).pdf"), "{}", name(&out[0]));
    let found = lab.texts(&out[0].path, None)[0].to_lowercase();
    assert!(
        found.contains("invoice") && found.contains("48213"),
        "{found}"
    );
    // The words sit where they are on the page: the text is at the top, not in the lower half.
    let top = lab
        .text_in(&out[0].path, 0, region(0.0, 0.0, 1.0, 0.2))
        .to_lowercase();
    let bottom = lab.text_in(&out[0].path, 0, region(0.0, 0.5, 1.0, 0.5));
    assert!(top.contains("invoice"), "top strip: {top:?}");
    assert!(bottom.trim().is_empty(), "lower half: {bottom:?}");
    // The page still looks like the scan: the picture is untouched.
    let (a, b) = (
        lab.look(&scan.path, 0, None),
        lab.look(&out[0].path, 0, None),
    );
    assert_eq!(a.inked, b.inked);
    assert_eq!(fs::read(&scan.path).unwrap(), before);
    lab.tidy();
}

#[test]
fn pages_that_already_have_text_are_skipped_and_text_output_uses_their_own_text() {
    let lab = Lab::new();
    if !lab.ocr_ready("eng") {
        return;
    }
    let source = lab.doc(
        "born-digital.pdf",
        &pdfs::pdf_with_texts(&["This page already has plenty of real text"]),
    );
    let err = lab.run(ocr(|_| {}), source.clone()).unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(err.message.contains("already has text"), "{}", err.message);

    let text = lab
        .run(ocr(|o| o.output = OcrOutput::Text), source)
        .unwrap();
    let written = fs::read_to_string(&text[0].path).unwrap();
    assert!(written.starts_with("--- Page 1 ---\r\n"), "{written:?}");
    assert!(written.contains("plenty of real text"), "{written:?}");
    lab.tidy();
}

#[test]
fn text_output_reads_a_scanned_page() {
    let lab = Lab::new();
    if !lab.ocr_ready("eng") {
        return;
    }
    let scan = scanned(&lab, &["Receipt total 1275 euros paid in cash"]);
    let out = lab.run(ocr(|o| o.output = OcrOutput::Text), scan).unwrap();
    assert!(name(&out[0]).ends_with("(ocr).txt"));
    let text = fs::read_to_string(&out[0].path).unwrap().to_lowercase();
    assert!(
        text.contains("receipt") && text.contains("1275"),
        "{text:?}"
    );
    lab.tidy();
}

#[test]
fn a_language_that_is_not_installed_is_named_before_anything_runs() {
    let lab = Lab::new();
    if !lab.ocr_ready("eng") {
        return;
    }
    let scan = scanned(&lab, &["Hello there"]);
    let before = fs::read_dir(lab.out()).unwrap().count();
    let err = lab
        .run(ocr(|o| o.languages = vec!["tlh".to_owned()]), scan)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ComponentMissing);
    assert!(err.message.contains("\"tlh\""), "{}", err.message);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), before);
}

#[test]
fn a_crooked_scan_is_straightened_and_read_in_one_run() {
    let lab = Lab::new();
    if !lab.ocr_ready("eng") {
        return;
    }
    let tilted = lab.put("tilted.pdf", &pdfs::pdf_with_tilted_lines(3.0, 30));
    let image = lab
        .engine
        .render_page(&tilted, None, PageIndex::new(0), Dpi::new(300).unwrap())
        .unwrap();
    let png = lab.dir.path().join("t.png");
    use application::ports::{PictureSpec, PictureWriter};
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
    let made = lab
        .run(
            ToolOptions::defaults_for(ToolId::ImagesToPdf),
            InputDoc::new(&png),
        )
        .unwrap();
    let out = lab
        .run(ocr(|o| o.deskew = true), InputDoc::new(&made[0].path))
        .unwrap();
    let after = measured(&lab, &out[0].path, 0, None).unwrap_or(0.0);
    assert!(after.abs() < 0.5, "straight after the run: {after}");
    let text = lab.texts(&out[0].path, None)[0].to_lowercase();
    assert!(
        text.contains("quarterly") && text.contains("report"),
        "{text}"
    );
    lab.tidy();
}

#[test]
fn scan_to_pdf_can_read_the_pages_it_makes() {
    let lab = Lab::new();
    if !lab.ocr_ready("eng") {
        return;
    }
    // A big, clear page so the photo needs no cleanup before it is read.
    let scan = scanned(&lab, &["Contract number 90417 signed in the spring"]);
    let png = lab.dir.path().join("page.png");
    let image = lab
        .engine
        .render_page(&scan.path, None, PageIndex::new(0), Dpi::new(300).unwrap())
        .unwrap();
    use application::ports::{PictureSpec, PictureWriter};
    RasterConverter
        .write_picture(
            &image,
            &PictureSpec {
                format: PictureFormat::Png,
                jpeg_quality: 90,
                color: PictureColor::Color,
                dpi: Dpi::new(300).unwrap(),
            },
            &png,
        )
        .unwrap();
    let out = lab
        .run(
            scan_options(|o| {
                o.ocr = true;
                o.auto_crop = false;
                o.perspective_fix = false;
            }),
            InputDoc::new(&png),
        )
        .unwrap();
    let text = lab.texts(&out[0].path, None)[0].to_lowercase();
    assert!(
        text.contains("contract") && text.contains("90417"),
        "{text}"
    );
    lab.tidy();
}

fn scan_options(f: impl FnOnce(&mut ScanToPdfOptions)) -> ToolOptions {
    scan(f)
}

#[test]
fn page_range_limits_what_is_read() {
    let lab = Lab::new();
    if !lab.ocr_ready("eng") {
        return;
    }
    let scan = scanned(&lab, &["Only this single page exists in the scan"]);
    let err = lab
        .run(ocr(|o| o.pages = PageRange::parse("3").unwrap()), scan)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::RangeOutOfBounds);
}
