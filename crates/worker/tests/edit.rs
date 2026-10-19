//! End-to-end tests for the Edit tools of Phase 5 (stamps, page geometry, structure): the real
//! RunTool, worker engines and file store, run on generated PDFs and checked by rendering and
//! reading them back with PDFium and qpdf.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use application::ports::{OutlineReader, PdfReader};
use application::{InputDoc, Password};
use common::{Lab, name};
use domain::PageRange;
use domain::fonts::FontFamily;
use domain::stamp::{Layer, NumberStyle, Position};
use domain::tools::common::Region;
use domain::tools::layout::{
    CropMethod, CropOptions, FlipDirection, FlipOptions, GrayscaleOptions, GrayscaleScope,
    LengthUnit, Margins, NUpOptions, PaperSize, ResizeOptions, SheetCount,
};
use domain::tools::stamp::{
    BatesOptions, HeaderFooterOptions, PageNumbersOptions, WatermarkKind, WatermarkOptions,
};
use domain::tools::structure::{
    AttachmentAction, AttachmentsOptions, BookmarkEntry, BookmarksOptions, LabelRange,
    PageLabelsOptions, RenameByContentOptions,
};
use domain::{ErrorCode, ToolId, ToolOptions};
use std::fs;
use std::path::Path;
use std::process::Command;
use test_support::{encrypt_pdf, pdfs, qpdf_exe};

fn region(x: f32, y: f32, width: f32, height: f32) -> Region {
    Region {
        x,
        y,
        width,
        height,
    }
}

const BOTTOM_STRIP: fn() -> Region = || region(0.0, 0.88, 1.0, 0.12);
const TOP_STRIP: fn() -> Region = || region(0.0, 0.0, 1.0, 0.12);

// ---- Page numbers -------------------------------------------------------------------------

#[test]
fn page_numbers_are_drawn_at_the_bottom_and_the_page_text_stays() {
    let lab = Lab::new();
    let source = lab.doc("book.pdf", &pdfs::pdf_with_texts(&["one", "two", "three"]));
    let before = fs::read(&source.path).unwrap();
    let out = lab
        .run(
            ToolOptions::defaults_for(ToolId::PageNumbers),
            source.clone(),
        )
        .unwrap();
    assert_eq!(name(&out[0]), "book (numbered).pdf");
    assert_eq!(out[0].pages, Some(3));
    let texts = lab.texts(&out[0].path, None);
    for (i, original) in ["one", "two", "three"].iter().enumerate() {
        assert!(texts[i].contains(original), "{texts:?}");
        assert!(texts[i].contains(&(i + 1).to_string()), "{texts:?}");
    }
    assert!(lab.text_in(&out[0].path, 1, BOTTOM_STRIP()).contains('2'));
    assert!(!lab.text_in(&out[0].path, 1, TOP_STRIP()).contains('2'));
    assert_eq!(
        fs::read(&source.path).unwrap(),
        before,
        "the input is untouched"
    );
    lab.tidy();
}

#[test]
fn page_numbers_follow_template_style_start_and_the_skipped_cover() {
    let lab = Lab::new();
    let source = lab.doc("doc.pdf", &pdfs::pdf_with_texts(&["cover", "a", "b"]));
    let out = lab
        .run(
            ToolOptions::PageNumbers(PageNumbersOptions {
                template: "Page {page} of {pages}".into(),
                style: NumberStyle::RomanLower,
                skip_first_page: true,
                start_number: 1,
                position: Position::BottomRight,
                font: FontFamily::Times,
                ..PageNumbersOptions::default()
            }),
            source,
        )
        .unwrap();
    let texts = lab.texts(&out[0].path, None);
    assert!(
        !texts[0].contains("Page"),
        "the cover has no number: {texts:?}"
    );
    assert!(texts[1].contains("Page i of ii"), "{texts:?}");
    assert!(texts[2].contains("Page ii of ii"), "{texts:?}");
    let right = lab.text_in(&out[0].path, 1, region(0.5, 0.88, 0.5, 0.12));
    let left = lab.text_in(&out[0].path, 1, region(0.0, 0.88, 0.5, 0.12));
    assert!(
        right.contains("Page i") && !left.contains("Page"),
        "{right:?} {left:?}"
    );
}

#[test]
fn numbers_on_a_turned_page_sit_at_the_displayed_bottom() {
    let lab = Lab::new();
    let source = lab.doc("turned.pdf", &pdfs::pdf_with_geometry(&[(612, 792, 90)]));
    let out = lab
        .run(ToolOptions::defaults_for(ToolId::PageNumbers), source)
        .unwrap();
    assert!(
        lab.text_in(&out[0].path, 0, BOTTOM_STRIP())
            .trim()
            .contains('1'),
        "the number is at the bottom as displayed"
    );
    assert!(!lab.text_in(&out[0].path, 0, TOP_STRIP()).contains('1'));
}

#[test]
fn facing_pages_put_the_number_on_the_outer_edge() {
    let lab = Lab::new();
    let source = lab.doc("duplex.pdf", &pdfs::pdf_with_texts(&["a", "b"]));
    let out = lab
        .run(
            ToolOptions::PageNumbers(PageNumbersOptions {
                position: Position::BottomRight,
                facing_pages: true,
                ..PageNumbersOptions::default()
            }),
            source,
        )
        .unwrap();
    let right_of = |page| lab.text_in(&out[0].path, page, region(0.5, 0.88, 0.5, 0.12));
    assert!(right_of(0).contains('1'), "page 1 is a right-hand page");
    assert!(
        lab.text_in(&out[0].path, 1, region(0.0, 0.88, 0.5, 0.12))
            .contains('2'),
        "page 2 mirrors to the left"
    );
}

#[test]
fn a_bad_template_is_rejected_before_anything_is_written() {
    let lab = Lab::new();
    let err = lab
        .run(
            ToolOptions::PageNumbers(PageNumbersOptions {
                template: "{oops}".into(),
                ..PageNumbersOptions::default()
            }),
            lab.doc("a.pdf", &pdfs::pdf_with_texts(&["x"])),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
}

#[test]
fn stamping_a_protected_file_keeps_it_protected() {
    let lab = Lab::new();
    let plain = lab.put("a.pdf", &pdfs::pdf_with_texts(&["secret", "pages"]));
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "pw");
    let mut input = InputDoc::new(&locked);
    input.password = Some(Password::new("pw"));
    let out = lab
        .run(ToolOptions::defaults_for(ToolId::PageNumbers), input)
        .unwrap();
    let password = Password::new("pw");
    let texts = lab.texts(&out[0].path, Some(&password));
    assert!(
        texts[0].contains("secret") && texts[0].contains('1'),
        "{texts:?}"
    );
    assert_eq!(
        lab.engine.open_info(&out[0].path, None).unwrap_err().code,
        ErrorCode::PdfEncrypted,
        "the result still needs the password"
    );
    lab.tidy();
}

// ---- Header and footer --------------------------------------------------------------------

#[test]
fn header_and_footer_fill_their_slots_with_the_tokens() {
    let lab = Lab::new();
    let source = lab.doc("report.pdf", &pdfs::pdf_with_texts(&["x", "y"]));
    let out = lab
        .run(
            ToolOptions::HeaderFooter(HeaderFooterOptions {
                header_left: "{filename}".into(),
                header_right: "Draft".into(),
                footer_center: "{page}/{pages}".into(),
                ..HeaderFooterOptions::default()
            }),
            source,
        )
        .unwrap();
    assert_eq!(name(&out[0]), "report (header and footer).pdf");
    let top_left = lab.text_in(&out[0].path, 1, region(0.0, 0.0, 0.5, 0.1));
    let top_right = lab.text_in(&out[0].path, 1, region(0.5, 0.0, 0.5, 0.1));
    assert!(top_left.contains("report"), "{top_left:?}");
    assert!(top_right.contains("Draft"), "{top_right:?}");
    assert!(lab.text_in(&out[0].path, 1, BOTTOM_STRIP()).contains("2/2"));
}

// ---- Bates --------------------------------------------------------------------------------

#[test]
fn bates_numbers_continue_across_files_in_list_order() {
    let lab = Lab::new();
    let first = lab.doc("a.pdf", &pdfs::pdf_with_texts(&["a1", "a2", "a3"]));
    let second = lab.doc("b.pdf", &pdfs::pdf_with_texts(&["b1", "b2"]));
    let out = lab
        .run_all(
            ToolOptions::Bates(BatesOptions {
                prefix: "ACME-".into(),
                digits: 4,
                ..BatesOptions::default()
            }),
            vec![first, second],
        )
        .unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(name(&out[0]), "a (bates numbered).pdf");
    let a = lab.texts(&out[0].path, None);
    let b = lab.texts(&out[1].path, None);
    assert!(
        a[0].contains("ACME-0001") && a[2].contains("ACME-0003"),
        "{a:?}"
    );
    assert!(
        b[0].contains("ACME-0004") && b[1].contains("ACME-0005"),
        "{b:?}"
    );
    assert!(
        lab.text_in(&out[0].path, 0, region(0.5, 0.88, 0.5, 0.12))
            .contains("ACME-0001")
    );
}

#[test]
fn bates_can_restart_for_every_file() {
    let lab = Lab::new();
    let out = lab
        .run_all(
            ToolOptions::Bates(BatesOptions {
                continue_across_files: false,
                start: 10,
                digits: 3,
                ..BatesOptions::default()
            }),
            vec![
                lab.doc("a.pdf", &pdfs::pdf_with_texts(&["x"])),
                lab.doc("b.pdf", &pdfs::pdf_with_texts(&["y"])),
            ],
        )
        .unwrap();
    assert!(lab.texts(&out[0].path, None)[0].contains("010"));
    assert!(lab.texts(&out[1].path, None)[0].contains("010"));
}

// ---- Watermark ----------------------------------------------------------------------------

#[test]
fn a_text_watermark_adds_ink_in_the_middle_and_keeps_the_text() {
    let lab = Lab::new();
    let source = lab.doc("w.pdf", &pdfs::pdf_with_texts(&["page text"]));
    let before = lab.look(&source.path, 0, None);
    let out = lab
        .run(
            ToolOptions::Watermark(WatermarkOptions {
                text: "SECRET".into(),
                rotation_deg: 45.0,
                transparency: 30,
                ..WatermarkOptions::default()
            }),
            source,
        )
        .unwrap();
    assert_eq!(name(&out[0]), "w (watermarked).pdf");
    let text = lab.texts(&out[0].path, None);
    assert!(
        text[0].contains("SECRET") && text[0].contains("page text"),
        "{text:?}"
    );
    let after = lab.look(&out[0].path, 0, None);
    assert!(after.inked > before.inked, "{before:?} {after:?}");
    assert!(
        (after.center_y - 0.5).abs() < (before.center_y - 0.5).abs(),
        "pulled toward the middle"
    );
}

#[test]
fn a_watermark_below_the_content_leaves_the_page_text_on_top() {
    let lab = Lab::new();
    let source = lab.doc("w.pdf", &pdfs::pdf_with_texts(&["one", "two"]));
    let out = lab
        .run(
            ToolOptions::Watermark(WatermarkOptions {
                layer: Layer::Below,
                pages: PageRange::parse("2").unwrap(),
                tile: true,
                ..WatermarkOptions::default()
            }),
            source,
        )
        .unwrap();
    let texts = lab.texts(&out[0].path, None);
    assert!(!texts[0].contains("CONFIDENTIAL"), "page 1 is not marked");
    assert!(texts[1].contains("CONFIDENTIAL"));
}

#[test]
fn a_picture_watermark_brings_its_color() {
    let lab = Lab::new();
    let logo = lab.dir.path().join("logo.png");
    image::RgbaImage::from_pixel(120, 60, image::Rgba([220, 20, 20, 255]))
        .save(&logo)
        .unwrap();
    let source = lab.doc("p.pdf", &pdfs::pdf_with_texts(&["plain"]));
    assert_eq!(lab.look(&source.path, 0, None).colored, 0);
    let out = lab
        .run(
            ToolOptions::Watermark(WatermarkOptions {
                kind: WatermarkKind::Image,
                image_path: Some(logo),
                image_width_percent: 40,
                transparency: 10,
                ..WatermarkOptions::default()
            }),
            source,
        )
        .unwrap();
    assert!(lab.look(&out[0].path, 0, None).colored > 100);
}

#[test]
fn a_watermark_picture_that_is_not_a_picture_is_refused() {
    let lab = Lab::new();
    let fake = lab.put("logo.png", b"this is text");
    let err = lab
        .run(
            ToolOptions::Watermark(WatermarkOptions {
                kind: WatermarkKind::Image,
                image_path: Some(fake),
                ..WatermarkOptions::default()
            }),
            lab.doc("a.pdf", &pdfs::pdf_with_texts(&["x"])),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
}

// ---- Crop ---------------------------------------------------------------------------------

#[test]
fn crop_by_margins_shrinks_the_page_by_exactly_those_margins() {
    let lab = Lab::new();
    let source = lab.doc(
        "c.pdf",
        &pdfs::pdf_with_geometry(&[(600, 800, 0), (600, 800, 0)]),
    );
    let out = lab
        .run(
            ToolOptions::Crop(CropOptions {
                margins: Margins {
                    top: 50.0,
                    right: 25.0,
                    bottom: 50.0,
                    left: 75.0,
                },
                unit: LengthUnit::Pt,
                pages: PageRange::parse("1").unwrap(),
                ..CropOptions::default()
            }),
            source,
        )
        .unwrap();
    let sizes = lab.sizes(&out[0].path);
    assert_eq!(sizes[0], (500.0, 700.0));
    assert_eq!(sizes[1], (600.0, 800.0), "page 2 was not selected");
}

#[test]
fn crop_by_a_drawn_region_keeps_that_box() {
    let lab = Lab::new();
    let source = lab.doc("c.pdf", &pdfs::pdf_with_geometry(&[(600, 800, 0)]));
    let out = lab
        .run(
            ToolOptions::Crop(CropOptions {
                method: CropMethod::Region,
                region: Some(region(0.25, 0.25, 0.5, 0.5)),
                apply_to_media_box: true,
                ..CropOptions::default()
            }),
            source,
        )
        .unwrap();
    assert_eq!(lab.sizes(&out[0].path)[0], (300.0, 400.0));
}

#[test]
fn crop_on_a_turned_page_uses_the_displayed_margins() {
    let lab = Lab::new();
    let source = lab.doc("c.pdf", &pdfs::pdf_with_geometry(&[(300, 400, 90)]));
    let out = lab
        .run(
            ToolOptions::Crop(CropOptions {
                margins: Margins {
                    left: 100.0,
                    ..Margins::default()
                },
                unit: LengthUnit::Pt,
                ..CropOptions::default()
            }),
            source,
        )
        .unwrap();
    // Displayed 400 wide by 300 high; cutting 100 off the displayed left leaves 300 by 300.
    assert_eq!(lab.sizes(&out[0].path)[0], (300.0, 300.0));
}

#[test]
fn auto_crop_trims_the_blank_space_around_the_content_and_keeps_the_text() {
    let lab = Lab::new();
    let source = lab.doc("c.pdf", &pdfs::pdf_with_texts(&["trim me"]));
    let out = lab
        .run(
            ToolOptions::Crop(CropOptions {
                method: CropMethod::Auto,
                auto_padding_mm: 3.0,
                ..CropOptions::default()
            }),
            source,
        )
        .unwrap();
    let (w, h) = lab.sizes(&out[0].path)[0];
    assert!(w < 300.0 && h < 80.0, "{w} x {h}");
    assert!(lab.texts(&out[0].path, None)[0].contains("trim me"));
}

#[test]
fn auto_crop_of_a_blank_document_says_there_is_nothing_to_crop_to() {
    let lab = Lab::new();
    let blank = pdfs::pdf_with_texts(&[" "]);
    let err = lab
        .run(
            ToolOptions::Crop(CropOptions {
                method: CropMethod::Auto,
                ..CropOptions::default()
            }),
            lab.doc("blank.pdf", &blank),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
}

// ---- Resize -------------------------------------------------------------------------------

#[test]
fn resize_makes_every_page_a4_and_keeps_the_text() {
    let lab = Lab::new();
    let source = lab.doc("r.pdf", &pdfs::pdf_with_texts(&["one", "two"]));
    let out = lab
        .run(ToolOptions::defaults_for(ToolId::Resize), source)
        .unwrap();
    for (w, h) in lab.sizes(&out[0].path) {
        assert!(
            (w - 595.28).abs() < 0.1 && (h - 841.89).abs() < 0.1,
            "{w} x {h}"
        );
    }
    assert!(lab.texts(&out[0].path, None)[1].contains("two"));
}

#[test]
fn resize_with_a_margin_leaves_blank_space_and_landscape_stays_landscape() {
    let lab = Lab::new();
    let source = lab.doc("r.pdf", &pdfs::pdf_with_geometry(&[(800, 600, 0)]));
    let before = lab.look(&source.path, 0, None);
    let out = lab
        .run(
            ToolOptions::Resize(ResizeOptions {
                size: PaperSize::Letter,
                margin_mm: 20.0,
                ..ResizeOptions::default()
            }),
            source,
        )
        .unwrap();
    let (w, h) = lab.sizes(&out[0].path)[0];
    assert_eq!((w, h), (792.0, 612.0));
    let after = lab.look(&out[0].path, 0, None);
    assert!(after.inked > 0 && before.inked > 0);
}

// ---- Flip ---------------------------------------------------------------------------------

#[test]
fn flip_horizontal_moves_content_from_the_left_to_the_right() {
    let lab = Lab::new();
    let source = lab.doc("f.pdf", &pdfs::pdf_with_texts(&["left text"]));
    let before = lab.look(&source.path, 0, None);
    let out = lab
        .run(ToolOptions::defaults_for(ToolId::Flip), source)
        .unwrap();
    let after = lab.look(&out[0].path, 0, None);
    assert!(
        before.center_x < 0.5 && after.center_x > 0.5,
        "{before:?} {after:?}"
    );
    assert!((before.center_y - after.center_y).abs() < 0.01);
    // Mirroring moves antialiasing by a pixel here and there, but loses nothing.
    assert!(
        after.inked.abs_diff(before.inked) * 50 < before.inked,
        "{before:?} {after:?}"
    );
}

#[test]
fn flip_vertical_moves_content_from_the_top_to_the_bottom() {
    let lab = Lab::new();
    let source = lab.doc("f.pdf", &pdfs::pdf_with_texts(&["top text"]));
    let before = lab.look(&source.path, 0, None);
    let out = lab
        .run(
            ToolOptions::Flip(FlipOptions {
                direction: FlipDirection::Vertical,
                ..FlipOptions::default()
            }),
            source,
        )
        .unwrap();
    let after = lab.look(&out[0].path, 0, None);
    assert!(
        before.center_y < 0.5 && after.center_y > 0.5,
        "{before:?} {after:?}"
    );
}

#[test]
fn flip_on_a_turned_page_mirrors_what_is_displayed() {
    let lab = Lab::new();
    let source = lab.doc("f.pdf", &pdfs::pdf_with_geometry(&[(612, 792, 90)]));
    let before = lab.look(&source.path, 0, None);
    let out = lab
        .run(ToolOptions::defaults_for(ToolId::Flip), source)
        .unwrap();
    let after = lab.look(&out[0].path, 0, None);
    assert!(
        (before.center_x - 0.5) * (after.center_x - 0.5) < 0.0,
        "the displayed side changes: {before:?} {after:?}"
    );
    assert!((before.center_y - after.center_y).abs() < 0.01);
}

// ---- N-up ---------------------------------------------------------------------------------

#[test]
fn n_up_puts_four_pages_on_each_sheet() {
    let lab = Lab::new();
    let texts = ["p1", "p2", "p3", "p4", "p5", "p6"];
    let source = lab.doc("n.pdf", &pdfs::pdf_with_texts(&texts));
    let out = lab
        .run(
            ToolOptions::NUp(NUpOptions {
                per_sheet: SheetCount::Four,
                ..NUpOptions::default()
            }),
            source,
        )
        .unwrap();
    assert_eq!(out[0].pages, Some(2));
    assert_eq!(name(&out[0]), "n (n-up).pdf");
    let sheets = lab.texts(&out[0].path, None);
    for text in &texts[..4] {
        assert!(sheets[0].contains(text), "{sheets:?}");
    }
    assert!(sheets[1].contains("p5") && sheets[1].contains("p6") && !sheets[1].contains("p1"));
    assert_eq!(lab.sizes(&out[0].path)[0], (612.0, 792.0));
    // Four reduced pages cover all four quarters of the sheet.
    let quadrant = |x, y| lab.text_in(&out[0].path, 0, region(x, y, 0.5, 0.5));
    assert!(quadrant(0.0, 0.0).contains("p1"));
    assert!(quadrant(0.5, 0.0).contains("p2"));
    assert!(quadrant(0.0, 0.5).contains("p3"));
    assert!(quadrant(0.5, 0.5).contains("p4"));
}

#[test]
fn two_up_makes_landscape_sheets() {
    let lab = Lab::new();
    let source = lab.doc("n.pdf", &pdfs::pdf_with_texts(&["a", "b", "c"]));
    let out = lab
        .run(ToolOptions::defaults_for(ToolId::NUp), source)
        .unwrap();
    assert_eq!(out[0].pages, Some(2));
    assert_eq!(lab.sizes(&out[0].path)[0], (792.0, 612.0));
}

// ---- Grayscale ----------------------------------------------------------------------------

#[test]
fn grayscale_removes_every_color_from_text_lines_and_pictures() {
    let lab = Lab::new();
    let source = lab.doc("g.pdf", &pdfs::pdf_with_colors());
    let before = lab.look(&source.path, 0, None);
    assert!(before.colored > 1000, "{before:?}");
    let out = lab
        .run(ToolOptions::defaults_for(ToolId::Grayscale), source)
        .unwrap();
    assert_eq!(name(&out[0]), "g (grayscale).pdf");
    let after = lab.look(&out[0].path, 0, None);
    assert_eq!(after.colored, 0, "{after:?}");
    assert!(after.inked > 0);
    assert!(
        lab.texts(&out[0].path, None)[0].contains("COLORS"),
        "text stays text"
    );
    lab.tidy();
}

#[test]
fn grayscale_turns_color_pictures_gray_too() {
    let lab = Lab::new();
    let png = lab.dir.path().join("p.png");
    image::RgbImage::from_fn(300, 200, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 200])
    })
    .save(&png)
    .unwrap();
    let pdf = lab.dir.path().join("p.pdf");
    application::ports::ImageConverter::image_to_pdf(
        &quirewise_engine_raster::RasterConverter,
        &png,
        &pdf,
    )
    .unwrap();
    assert!(lab.look(&pdf, 0, None).colored > 1000);
    let out = lab
        .run(
            ToolOptions::defaults_for(ToolId::Grayscale),
            InputDoc::new(&pdf),
        )
        .unwrap();
    assert_eq!(lab.look(&out[0].path, 0, None).colored, 0);
}

#[test]
fn grayscale_scopes_change_only_what_they_name() {
    let lab = Lab::new();
    let source = lab.doc("g.pdf", &pdfs::pdf_with_colors());
    let images_only = lab
        .run(
            ToolOptions::Grayscale(GrayscaleOptions {
                apply_to: GrayscaleScope::Images,
                ..GrayscaleOptions::default()
            }),
            source.clone(),
        )
        .unwrap();
    assert!(
        lab.look(&images_only[0].path, 0, None).colored > 1000,
        "vector colors stay when only pictures are asked for"
    );
    let text_only = lab
        .run(
            ToolOptions::Grayscale(GrayscaleOptions {
                apply_to: GrayscaleScope::Text,
                ..GrayscaleOptions::default()
            }),
            source,
        )
        .unwrap();
    assert_eq!(lab.look(&text_only[0].path, 0, None).colored, 0);
}

#[test]
fn the_raster_fallback_catches_everything_and_drops_the_text_layer() {
    let lab = Lab::new();
    let source = lab.doc("g.pdf", &pdfs::pdf_with_colors());
    let out = lab
        .run(
            ToolOptions::Grayscale(GrayscaleOptions {
                raster_fallback: true,
                ..GrayscaleOptions::default()
            }),
            source,
        )
        .unwrap();
    assert_eq!(lab.look(&out[0].path, 0, None).colored, 0);
    assert!(lab.texts(&out[0].path, None)[0].is_empty());
    assert_eq!(lab.sizes(&out[0].path)[0], (612.0, 792.0));
}

#[test]
fn grayscale_of_a_protected_file_stays_protected() {
    let lab = Lab::new();
    let plain = lab.put("a.pdf", &pdfs::pdf_with_colors());
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "pw");
    let mut input = InputDoc::new(&locked);
    input.password = Some(Password::new("pw"));
    let out = lab
        .run(ToolOptions::defaults_for(ToolId::Grayscale), input)
        .unwrap();
    let pw = Password::new("pw");
    assert_eq!(lab.look(&out[0].path, 0, Some(&pw)).colored, 0);
    assert_eq!(
        lab.engine.open_info(&out[0].path, None).unwrap_err().code,
        ErrorCode::PdfEncrypted
    );
}

// ---- Bookmarks ----------------------------------------------------------------------------

fn outline_of(lab: &Lab, path: &Path) -> Vec<(String, u32, u32)> {
    lab.engine
        .outline(path, None)
        .unwrap()
        .into_iter()
        .map(|i| (i.title, i.page.map_or(0, |p| p.get() + 1), i.level))
        .collect()
}

#[test]
fn bookmarks_replace_the_outline_with_the_edited_list() {
    let lab = Lab::new();
    let source = lab.doc("b.pdf", &pdfs::pdf_with_outline(5, &[("Old", 0, 1)]));
    let entry = |title: &str, page, level| BookmarkEntry {
        title: title.into(),
        page,
        level,
    };
    let out = lab
        .run(
            ToolOptions::Bookmarks(BookmarksOptions {
                bookmarks: vec![
                    entry("Intro", 1, 1),
                    entry("Scope", 2, 2),
                    entry("Method", 4, 1),
                ],
                ..BookmarksOptions::default()
            }),
            source,
        )
        .unwrap();
    assert_eq!(
        outline_of(&lab, &out[0].path),
        vec![
            ("Intro".to_owned(), 1, 1),
            ("Scope".to_owned(), 2, 2),
            ("Method".to_owned(), 4, 1),
        ]
    );
}

#[test]
fn bookmarks_can_be_generated_from_the_headings() {
    let lab = Lab::new();
    let source = lab.doc(
        "h.pdf",
        &pdfs::pdf_with_sized_lines(&[
            vec![
                ("Annual Report", 28.0),
                (
                    "This is the body text of the report and it runs on for a while.",
                    11.0,
                ),
                (
                    "More body text so that the body size is clearly the common one.",
                    11.0,
                ),
            ],
            vec![
                ("Results", 18.0),
                (
                    "Body text again, long enough to count as the body of the document.",
                    11.0,
                ),
            ],
        ]),
    );
    let out = lab
        .run(
            ToolOptions::Bookmarks(BookmarksOptions {
                auto_generate: true,
                ..BookmarksOptions::default()
            }),
            source,
        )
        .unwrap();
    assert_eq!(
        outline_of(&lab, &out[0].path),
        vec![
            ("Annual Report".to_owned(), 1, 1),
            ("Results".to_owned(), 2, 2)
        ]
    );
}

#[test]
fn generating_bookmarks_from_a_document_without_headings_explains_why() {
    let lab = Lab::new();
    let source = lab.doc(
        "flat.pdf",
        &pdfs::pdf_with_texts(&["same size", "same size too"]),
    );
    let err = lab
        .run(
            ToolOptions::Bookmarks(BookmarksOptions {
                auto_generate: true,
                ..BookmarksOptions::default()
            }),
            source,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(err.message.contains("No headings"), "{}", err.message);
}

// ---- Rename by content --------------------------------------------------------------------

#[test]
fn rename_copies_the_file_under_a_name_made_from_its_content() {
    let lab = Lab::new();
    let source = lab.doc("scan0001.pdf", &pdfs::pdf_with_texts(&["INV-1001"]));
    let before = fs::read(&source.path).unwrap();
    let out = lab
        .run(
            ToolOptions::RenameByContent(RenameByContentOptions {
                region: Some(region(0.0, 0.0, 0.6, 0.15)),
                pattern: "{text} ({filename})".into(),
                ..RenameByContentOptions::default()
            }),
            source.clone(),
        )
        .unwrap();
    assert_eq!(name(&out[0]), "INV-1001 (scan0001).pdf");
    assert_eq!(
        fs::read(&out[0].path).unwrap(),
        before,
        "a byte-for-byte copy"
    );
    assert_eq!(
        fs::read(&source.path).unwrap(),
        before,
        "the original stays"
    );
    lab.tidy();
}

#[test]
fn rename_cleans_characters_a_file_name_cannot_hold() {
    let lab = Lab::new();
    let source = lab.doc("a.pdf", &pdfs::pdf_with_texts(&["No: 12/34"]));
    let out = lab
        .run(
            ToolOptions::RenameByContent(RenameByContentOptions {
                region: Some(region(0.0, 0.0, 0.7, 0.15)),
                ..RenameByContentOptions::default()
            }),
            source,
        )
        .unwrap();
    let file = name(&out[0]);
    assert!(file.starts_with("No") && file.ends_with(".pdf"), "{file}");
    assert!(!file.contains(['/', ':']), "{file}");
}

#[test]
fn rename_reports_an_empty_region_and_two_files_with_the_same_text_get_different_names() {
    let lab = Lab::new();
    let options = |y: f32| {
        ToolOptions::RenameByContent(RenameByContentOptions {
            region: Some(region(0.0, y, 0.6, 0.1)),
            ..RenameByContentOptions::default()
        })
    };
    let err = lab
        .run(
            options(0.7),
            lab.doc("a.pdf", &pdfs::pdf_with_texts(&["INV-7"])),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(err.message.contains("No text was found"), "{}", err.message);

    let first = lab
        .run(
            options(0.0),
            lab.doc("one.pdf", &pdfs::pdf_with_texts(&["INV-7"])),
        )
        .unwrap();
    let second = lab
        .run(
            options(0.0),
            lab.doc("two.pdf", &pdfs::pdf_with_texts(&["INV-7"])),
        )
        .unwrap();
    assert_eq!(name(&first[0]), "INV-7.pdf");
    assert_eq!(name(&second[0]), "INV-7 (2).pdf");
}

// ---- Attachments --------------------------------------------------------------------------

#[test]
fn attachments_can_be_added_listed_extracted_and_removed() {
    let lab = Lab::new();
    let notes = lab.put("notes.txt", b"remember the milk");
    let data = lab.put("data.csv", b"a,b\n1,2\n");
    let source = lab.doc("doc.pdf", &pdfs::pdf_with_texts(&["x"]));

    let added = lab
        .run(
            ToolOptions::Attachments(AttachmentsOptions {
                action: AttachmentAction::Add,
                files: vec![
                    notes.to_string_lossy().into_owned(),
                    data.to_string_lossy().into_owned(),
                ],
                ..AttachmentsOptions::default()
            }),
            source,
        )
        .unwrap();
    assert_eq!(name(&added[0]), "doc (attachments added).pdf");

    let listed = lab
        .run(
            ToolOptions::Attachments(AttachmentsOptions::default()),
            InputDoc::new(&added[0].path),
        )
        .unwrap();
    let listing = fs::read_to_string(&listed[0].path).unwrap();
    assert!(listing.contains("2 attachment(s)"), "{listing}");
    assert!(
        listing.contains("data.csv\t8 bytes") && listing.contains("notes.txt\t17 bytes"),
        "{listing}"
    );

    let extracted = lab
        .run(
            ToolOptions::Attachments(AttachmentsOptions {
                action: AttachmentAction::Extract,
                names: vec!["notes.txt".into()],
                ..AttachmentsOptions::default()
            }),
            InputDoc::new(&added[0].path),
        )
        .unwrap();
    assert_eq!(extracted.len(), 1);
    assert_eq!(name(&extracted[0]), "doc (attachments added) - notes.txt");
    assert_eq!(fs::read(&extracted[0].path).unwrap(), b"remember the milk");

    let removed = lab
        .run(
            ToolOptions::Attachments(AttachmentsOptions {
                action: AttachmentAction::Delete,
                names: vec!["notes.txt".into()],
                ..AttachmentsOptions::default()
            }),
            InputDoc::new(&added[0].path),
        )
        .unwrap();
    let after = lab
        .run(
            ToolOptions::Attachments(AttachmentsOptions::default()),
            InputDoc::new(&removed[0].path),
        )
        .unwrap();
    let listing = fs::read_to_string(&after[0].path).unwrap();
    assert!(
        listing.contains("1 attachment(s)") && listing.contains("data.csv"),
        "{listing}"
    );
    assert!(!listing.contains("notes.txt"));
}

#[test]
fn extracting_from_a_document_without_attachments_and_removing_an_unknown_name_fail_clearly() {
    let lab = Lab::new();
    let source = lab.doc("plain.pdf", &pdfs::pdf_with_texts(&["x"]));
    let err = lab
        .run(
            ToolOptions::Attachments(AttachmentsOptions {
                action: AttachmentAction::Extract,
                ..AttachmentsOptions::default()
            }),
            source.clone(),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    let err = lab
        .run(
            ToolOptions::Attachments(AttachmentsOptions {
                action: AttachmentAction::Delete,
                names: vec!["missing.txt".into()],
                ..AttachmentsOptions::default()
            }),
            source,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(err.message.contains("missing.txt"));
    let missing = lab
        .run(
            ToolOptions::Attachments(AttachmentsOptions {
                action: AttachmentAction::Add,
                files: vec!["Z:\\nowhere\\x.txt".into()],
                ..AttachmentsOptions::default()
            }),
            lab.doc("b.pdf", &pdfs::pdf_with_texts(&["y"])),
        )
        .unwrap_err();
    assert_eq!(missing.code, ErrorCode::PathDenied);
}

// ---- Page labels --------------------------------------------------------------------------

fn labels_json(path: &Path) -> String {
    let out = Command::new(qpdf_exe().unwrap())
        .args(["--json", "--json-key=pagelabels"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn page_labels_are_written_as_a_number_tree_viewers_understand() {
    let lab = Lab::new();
    let source = lab.doc("l.pdf", &pdfs::pdf_with_texts(&["a", "b", "c", "d", "e"]));
    let out = lab
        .run(
            ToolOptions::PageLabels(PageLabelsOptions {
                ranges: vec![
                    LabelRange {
                        from_page: 1,
                        style: NumberStyle::RomanLower,
                        ..LabelRange::default()
                    },
                    LabelRange {
                        from_page: 3,
                        style: NumberStyle::Arabic,
                        prefix: "A-".into(),
                        start: 1,
                    },
                ],
                remove_all: false,
            }),
            source,
        )
        .unwrap();
    assert_eq!(name(&out[0]), "l (page labels).pdf");
    let json = labels_json(&out[0].path);
    assert!(json.contains("\"/S\": \"/r\""), "{json}");
    assert!(json.contains("\"/P\": \"u:A-\""), "{json}");
    assert!(json.contains("\"/S\": \"/D\""), "{json}");

    let cleared = lab
        .run(
            ToolOptions::PageLabels(PageLabelsOptions {
                ranges: vec![],
                remove_all: true,
            }),
            InputDoc::new(&out[0].path),
        )
        .unwrap();
    assert!(!labels_json(&cleared[0].path).contains("\"/S\""));
}

#[test]
fn every_edit_tool_leaves_the_input_untouched_and_no_scratch_behind() {
    let lab = Lab::new();
    let bytes = pdfs::pdf_with_texts(&["alpha", "beta"]);
    let source = lab.doc("keep.pdf", &bytes);
    let tools = [
        ToolId::PageNumbers,
        ToolId::Watermark,
        ToolId::Resize,
        ToolId::Flip,
        ToolId::NUp,
        ToolId::Grayscale,
    ];
    for tool in tools {
        lab.run(ToolOptions::defaults_for(tool), source.clone())
            .unwrap_or_else(|e| panic!("{tool}: {e}"));
        assert_eq!(
            fs::read(&source.path).unwrap(),
            bytes,
            "{tool} changed the input"
        );
    }
    lab.tidy();
}

#[test]
fn a_cancelled_stamp_run_writes_nothing() {
    use application::{CancellationToken, NoProgress, UnitRequest};
    let lab = Lab::new();
    let token = CancellationToken::new();
    token.cancel();
    let request = UnitRequest {
        tool: ToolId::PageNumbers,
        options: ToolOptions::defaults_for(ToolId::PageNumbers),
        inputs: vec![lab.doc("a.pdf", &pdfs::pdf_with_texts(&["x"]))],
        output_dir: Some(lab.out()),
        naming_pattern: domain::naming::OutputNamingPattern::default(),
    };
    let err = lab
        .runner
        .run_unit(&request, &token, &NoProgress)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::JobCancelled);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}

#[test]
fn the_phase_4_tools_keep_a_protected_file_protected_too() {
    use domain::tools::document::{FlattenOptions, MetadataOptions};
    let lab = Lab::new();
    let plain = lab.put("a.pdf", &pdfs::pdf_with_texts(&["kept", "safe"]));
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "pw");
    let tools = [
        ToolOptions::defaults_for(ToolId::Sanitize),
        ToolOptions::EditMetadata(MetadataOptions {
            remove_all: true,
            ..MetadataOptions::default()
        }),
        ToolOptions::defaults_for(ToolId::RemoveAnnotations),
        ToolOptions::defaults_for(ToolId::Linearize),
        ToolOptions::Flatten(FlattenOptions::default()),
        ToolOptions::defaults_for(ToolId::Compress),
    ];
    for options in tools {
        let id = options.tool_id();
        let mut input = InputDoc::new(&locked);
        input.password = Some(Password::new("pw"));
        let out = lab
            .run(options, input)
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        let pw = Password::new("pw");
        assert!(
            lab.texts(&out[0].path, Some(&pw))[0].contains("kept"),
            "{id}"
        );
        assert_eq!(
            lab.engine.open_info(&out[0].path, None).unwrap_err().code,
            ErrorCode::PdfEncrypted,
            "{id} dropped the protection"
        );
    }
    lab.tidy();
}
