//! End-to-end tests for the conversion tools that need no outside program (Images to PDF, PDF to
//! images, Extract images, PDF to text, TIFF to and from PDF): the real RunTool, worker engines
//! and file store, run on generated files and read back with independent decoders.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use application::{InputDoc, Password};
use common::{Lab, name};
use domain::tools::convert::{
    ExtractImagesOptions, ImageMargin, ImageOrientation, ImagePageSize, ImagesToPdfOptions,
    PdfToImagesOptions, PdfToTextOptions, PdfToTiffOptions, PictureColor, PictureFormat,
    TextEncoding, TextLayout, TiffCompression,
};
use domain::{Dpi, ErrorCode, PageRange, ToolId, ToolOptions};
use image::{Rgb, RgbImage};
use std::fs;
use std::io::BufReader;
use std::path::Path;
use test_support::{encrypt_pdf, pdfs};

fn jpeg_bytes(width: u32, height: u32) -> Vec<u8> {
    let img = RgbImage::from_fn(width, height, |x, y| {
        Rgb([(x * 2) as u8, (y * 3) as u8, 120])
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88)
        .encode(img.as_raw(), width, height, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let img = RgbImage::from_fn(width, height, |x, _| Rgb([200, (x % 255) as u8, 30]));
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn options_images(f: impl FnOnce(&mut ImagesToPdfOptions)) -> ToolOptions {
    let mut options = ImagesToPdfOptions::default();
    f(&mut options);
    ToolOptions::ImagesToPdf(options)
}

fn options_pages(f: impl FnOnce(&mut PdfToImagesOptions)) -> ToolOptions {
    let mut options = PdfToImagesOptions::default();
    f(&mut options);
    ToolOptions::PdfToImages(options)
}

fn options_text(f: impl FnOnce(&mut PdfToTextOptions)) -> ToolOptions {
    let mut options = PdfToTextOptions::default();
    f(&mut options);
    ToolOptions::PdfToText(options)
}

fn frames(path: &Path) -> Vec<tiff::ColorType> {
    let file = fs::File::open(path).unwrap();
    let mut decoder = tiff::decoder::Decoder::new(BufReader::new(file)).unwrap();
    let mut out = vec![decoder.colortype().unwrap()];
    while decoder.more_images() {
        decoder.next_image().unwrap();
        out.push(decoder.colortype().unwrap());
    }
    out
}

// ---- Images to PDF ------------------------------------------------------------------------

#[test]
fn pictures_become_one_pdf_with_a_page_each_and_the_inputs_stay_untouched() {
    let lab = Lab::new();
    let a = lab.doc("a.png", &png_bytes(300, 150));
    let b = lab.doc("b.jpg", &jpeg_bytes(120, 200));
    let (before_a, before_b) = (fs::read(&a.path).unwrap(), fs::read(&b.path).unwrap());
    let out = lab
        .run_all(
            ToolOptions::defaults_for(ToolId::ImagesToPdf),
            vec![a.clone(), b.clone()],
        )
        .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(name(&out[0]), "a (images).pdf");
    assert_eq!(out[0].pages, Some(2));
    let sizes = lab.sizes(&out[0].path);
    assert_eq!(sizes.len(), 2);
    assert!(
        (sizes[0].0 - 144.0).abs() < 0.5 && (sizes[0].1 - 72.0).abs() < 0.5,
        "{sizes:?}"
    );
    assert_eq!(fs::read(&a.path).unwrap(), before_a);
    assert_eq!(fs::read(&b.path).unwrap(), before_b);
    lab.tidy();
}

#[test]
fn pictures_can_go_on_paper_with_margins_or_one_pdf_each() {
    let lab = Lab::new();
    let a = lab.doc("a.png", &png_bytes(300, 150));
    let b = lab.doc("b.png", &png_bytes(150, 300));
    let paper = options_images(|o| {
        o.page_size = ImagePageSize::A4;
        o.margin = ImageMargin::Big;
        o.orientation = ImageOrientation::Auto;
    });
    let out = lab.run_all(paper, vec![a.clone(), b.clone()]).unwrap();
    let sizes = lab.sizes(&out[0].path);
    assert!(
        sizes[0].0 > sizes[0].1,
        "wide picture on a landscape page: {sizes:?}"
    );
    assert!(
        sizes[1].0 < sizes[1].1,
        "tall picture on a portrait page: {sizes:?}"
    );
    assert!((sizes[1].0 - 595.3).abs() < 0.6 && (sizes[1].1 - 841.9).abs() < 0.6);

    let apart = options_images(|o| o.one_pdf = false);
    let out = lab.run_all(apart, vec![a, b]).unwrap();
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|o| o.pages == Some(1)));
    assert_ne!(name(&out[0]), name(&out[1]));
    lab.tidy();
}

#[test]
fn a_file_that_is_not_a_picture_is_refused_before_anything_is_written() {
    let lab = Lab::new();
    let pdf = lab.doc("a.pdf", &pdfs::pdf_with_texts(&["x"]));
    let err = lab
        .run(ToolOptions::defaults_for(ToolId::ImagesToPdf), pdf)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InputNotPdf);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}

// ---- PDF to images ------------------------------------------------------------------------

#[test]
fn each_page_becomes_a_picture_of_the_resolution_asked_for() {
    let lab = Lab::new();
    let source = lab.doc("doc.pdf", &pdfs::pdf_with_texts(&["one", "two", "three"]));
    let before = fs::read(&source.path).unwrap();
    let options = options_pages(|o| {
        o.dpi = Dpi::new(72).unwrap();
        o.pages = PageRange::parse("1,3").unwrap();
    });
    let out = lab.run(options, source.clone()).unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(name(&out[0]), "doc (page 1).jpg");
    assert_eq!(name(&out[1]), "doc (page 3).jpg");
    let decoded = image::open(&out[0].path).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (612, 792));
    assert_eq!(fs::read(&source.path).unwrap(), before);
    lab.tidy();
}

#[test]
fn colors_and_formats_follow_the_options() {
    let lab = Lab::new();
    let source = lab.doc("c.pdf", &pdfs::pdf_with_colors());
    let color = lab
        .run(
            options_pages(|o| {
                o.format = PictureFormat::Png;
                o.dpi = Dpi::new(50).unwrap();
            }),
            source.clone(),
        )
        .unwrap();
    assert!(color[0].path.extension().is_some_and(|e| e == "png"));
    assert!(image::open(&color[0].path).unwrap().color().has_color());

    let gray = lab
        .run(
            options_pages(|o| {
                o.format = PictureFormat::Png;
                o.color = PictureColor::Gray;
                o.dpi = Dpi::new(50).unwrap();
            }),
            source.clone(),
        )
        .unwrap();
    assert!(!image::open(&gray[0].path).unwrap().color().has_color());

    let bw = lab
        .run(
            options_pages(|o| {
                o.format = PictureFormat::Png;
                o.color = PictureColor::BlackWhite;
                o.dpi = Dpi::new(50).unwrap();
            }),
            source,
        )
        .unwrap();
    let pixels = image::open(&bw[0].path).unwrap().to_luma8();
    assert!(pixels.pixels().all(|p| p.0[0] == 0 || p.0[0] == 255));
    lab.tidy();
}

#[test]
fn one_multi_page_tiff_can_hold_the_whole_range() {
    let lab = Lab::new();
    let source = lab.doc("d.pdf", &pdfs::pdf_with_texts(&["a", "b", "c", "d"]));
    let out = lab
        .run(
            options_pages(|o| {
                o.format = PictureFormat::Tiff;
                o.single_tiff = true;
                o.dpi = Dpi::new(50).unwrap();
                o.pages = PageRange::parse("2-4").unwrap();
            }),
            source,
        )
        .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].pages, Some(3));
    assert_eq!(frames(&out[0].path).len(), 3);
    lab.tidy();
}

#[test]
fn an_encrypted_pdf_converts_with_its_password() {
    let lab = Lab::new();
    let plain = lab.put("a.pdf", &pdfs::pdf_with_texts(&["secret"]));
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "pw");
    let mut input = InputDoc::new(&locked);
    input.password = Some(Password::new("pw"));
    let out = lab
        .run(options_pages(|o| o.dpi = Dpi::new(50).unwrap()), input)
        .unwrap();
    assert_eq!(out.len(), 1);
    let wrong = {
        let mut input = InputDoc::new(&locked);
        input.password = Some(Password::new("nope"));
        lab.run(options_pages(|_| {}), input).unwrap_err()
    };
    assert_eq!(wrong.code, ErrorCode::PdfWrongPassword);
    lab.tidy();
}

// ---- Extract images -----------------------------------------------------------------------

#[test]
fn a_jpeg_goes_into_a_pdf_and_comes_out_byte_for_byte() {
    let lab = Lab::new();
    let jpeg = jpeg_bytes(160, 120);
    let picture = lab.doc("photo.jpg", &jpeg);
    let made = lab
        .run(ToolOptions::defaults_for(ToolId::ImagesToPdf), picture)
        .unwrap();
    let out = lab
        .run(
            ToolOptions::defaults_for(ToolId::ExtractImages),
            InputDoc::new(&made[0].path),
        )
        .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(name(&out[0]), "photo (images) (page 1 image 1).jpg");
    assert_eq!(fs::read(&out[0].path).unwrap(), jpeg);
    lab.tidy();
}

#[test]
fn small_pictures_are_skipped_and_a_png_can_be_asked_for() {
    let lab = Lab::new();
    let big = lab.doc("big.jpg", &jpeg_bytes(100, 80));
    let tiny = lab.doc("tiny.png", &png_bytes(20, 20));
    let made = lab
        .run_all(
            ToolOptions::defaults_for(ToolId::ImagesToPdf),
            vec![big, tiny],
        )
        .unwrap();
    let kept = lab
        .run(
            ToolOptions::defaults_for(ToolId::ExtractImages),
            InputDoc::new(&made[0].path),
        )
        .unwrap();
    assert_eq!(
        kept.len(),
        1,
        "the 20 x 20 picture is below the 50 x 50 filter"
    );

    let all_png = lab
        .run(
            ToolOptions::ExtractImages(ExtractImagesOptions {
                keep_original_format: false,
                min_width: 1,
                min_height: 1,
            }),
            InputDoc::new(&made[0].path),
        )
        .unwrap();
    assert_eq!(all_png.len(), 2);
    assert!(
        all_png
            .iter()
            .all(|o| o.path.extension().is_some_and(|e| e == "png"))
    );
    lab.tidy();
}

#[test]
fn a_document_without_pictures_says_so_and_an_encrypted_one_opens_with_its_password() {
    let lab = Lab::new();
    let text_only = lab.doc("t.pdf", &pdfs::pdf_with_texts(&["words"]));
    let err = lab
        .run(ToolOptions::defaults_for(ToolId::ExtractImages), text_only)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(err.message.contains("no pictures"), "{}", err.message);

    let picture = lab.doc("p.jpg", &jpeg_bytes(90, 90));
    let made = lab
        .run(ToolOptions::defaults_for(ToolId::ImagesToPdf), picture)
        .unwrap();
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&made[0].path, &locked, "pw");
    let mut input = InputDoc::new(&locked);
    input.password = Some(Password::new("pw"));
    let out = lab
        .run(ToolOptions::defaults_for(ToolId::ExtractImages), input)
        .unwrap();
    assert_eq!(out.len(), 1);
    lab.tidy();
}

// ---- PDF to text --------------------------------------------------------------------------

#[test]
fn text_comes_out_with_page_separators_and_windows_line_breaks() {
    let lab = Lab::new();
    let source = lab.doc("doc.pdf", &pdfs::pdf_with_texts(&["alpha", "beta"]));
    let out = lab
        .run(ToolOptions::defaults_for(ToolId::PdfToText), source)
        .unwrap();
    assert_eq!(name(&out[0]), "doc (text).txt");
    let text = fs::read_to_string(&out[0].path).unwrap();
    assert!(text.starts_with("--- Page 1 ---\r\nalpha"), "{text:?}");
    assert!(text.contains("\r\n\r\n--- Page 2 ---\r\nbeta"), "{text:?}");
    lab.tidy();
}

#[test]
fn separators_encodings_and_page_ranges_are_options() {
    let lab = Lab::new();
    let source = lab.doc(
        "doc.pdf",
        &pdfs::pdf_with_texts(&["alpha", "beta", "gamma"]),
    );
    let plain = lab
        .run(
            options_text(|o| {
                o.page_separators = false;
                o.pages = PageRange::parse("2-3").unwrap();
            }),
            source.clone(),
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(&plain[0].path).unwrap(),
        "beta\r\n\r\ngamma\r\n"
    );

    let utf16 = lab
        .run(
            options_text(|o| o.encoding = TextEncoding::Utf16),
            source.clone(),
        )
        .unwrap();
    let bytes = fs::read(&utf16[0].path).unwrap();
    assert_eq!(&bytes[..2], [0xFF, 0xFE]);
    let units: Vec<u16> = bytes[2..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    assert!(String::from_utf16(&units).unwrap().contains("gamma"));

    let bom = lab
        .run(options_text(|o| o.encoding = TextEncoding::Utf8Bom), source)
        .unwrap();
    assert_eq!(&fs::read(&bom[0].path).unwrap()[..3], [0xEF, 0xBB, 0xBF]);
    lab.tidy();
}

#[test]
fn layout_mode_keeps_columns_in_their_place() {
    let lab = Lab::new();
    let source = lab.doc(
        "cols.pdf",
        &pdfs::pdf_with_positioned_text(&[
            (50.0, 700.0, "Item"),
            (300.0, 700.0, "Price"),
            (50.0, 686.0, "Pens"),
            (300.0, 686.0, "12.50"),
        ]),
    );
    let out = lab
        .run(
            options_text(|o| {
                o.layout = TextLayout::Layout;
                o.page_separators = false;
            }),
            source,
        )
        .unwrap();
    let text = fs::read_to_string(&out[0].path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text:?}");
    assert_eq!(lines[0].find("Price"), lines[1].find("12.50"));
    lab.tidy();
}

// ---- TIFF ---------------------------------------------------------------------------------

#[test]
fn a_pdf_goes_to_a_multi_page_tiff_and_back_with_every_compression() {
    let lab = Lab::new();
    let source = lab.doc("doc.pdf", &pdfs::pdf_with_texts(&["one", "two", "three"]));
    for compression in [
        TiffCompression::Lzw,
        TiffCompression::Deflate,
        TiffCompression::CcittG4,
    ] {
        let tiff = lab
            .run(
                ToolOptions::PdfToTiff(PdfToTiffOptions {
                    compression,
                    dpi: Dpi::new(100).unwrap(),
                    color: PictureColor::Gray,
                    pages: PageRange::all(),
                }),
                source.clone(),
            )
            .unwrap();
        assert_eq!(tiff[0].pages, Some(3), "{compression:?}");
        let colors = frames(&tiff[0].path);
        assert_eq!(colors.len(), 3);
        if compression == TiffCompression::CcittG4 {
            assert!(
                colors.iter().all(|c| *c == tiff::ColorType::Gray(1)),
                "{colors:?}"
            );
        }
        let back = lab
            .run(
                ToolOptions::defaults_for(ToolId::TiffToPdf),
                InputDoc::new(&tiff[0].path),
            )
            .unwrap();
        assert_eq!(back[0].pages, Some(3), "{compression:?}");
        let sizes = lab.sizes(&back[0].path);
        // Letter at 100 dpi, written back at the resolution stored in the file.
        assert!(
            (sizes[0].0 - 612.0).abs() < 1.0 && (sizes[0].1 - 792.0).abs() < 1.0,
            "{sizes:?}"
        );
        for output in tiff.iter().chain(back.iter()) {
            fs::remove_file(&output.path).unwrap();
        }
    }
    lab.tidy();
}

#[test]
fn a_tiff_that_is_damaged_is_rejected_cleanly() {
    let lab = Lab::new();
    let mut bytes = vec![b'I', b'I', 42, 0, 8, 0, 0, 0];
    bytes.extend_from_slice(&[0xFF; 20]);
    let bad = lab.doc("bad.tif", &bytes);
    let err = lab
        .run(ToolOptions::defaults_for(ToolId::TiffToPdf), bad)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InputNotPdf);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}
