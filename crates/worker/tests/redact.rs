//! End-to-end tests for Redact: the real RunTool, worker engines and file store, on generated
//! pages. Each result is read back three ways: the text PDFium finds, every stream and string in
//! the file, and the pixels of the rendered page.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use application::ports::Renderer;
use application::{InputDoc, Password};
use common::{Lab, name};
use domain::tools::common::Region;
use domain::tools::redact::{RedactArea, RedactOptions, RedactPreset};
use domain::{Dpi, ErrorCode, PageIndex, ToolOptions};
use lopdf::{Document, Object};
use std::fs;
use std::path::Path;
use test_support::{encrypt_pdf, pdfs};

fn region(x: f32, y: f32, width: f32, height: f32) -> Region {
    Region {
        x,
        y,
        width,
        height,
    }
}

fn options(f: impl FnOnce(&mut RedactOptions)) -> ToolOptions {
    let mut o = RedactOptions::default();
    f(&mut o);
    ToolOptions::Redact(o)
}

fn area(page: u32, r: Region) -> RedactArea {
    RedactArea { page, region: r }
}

/// Whether the text is in any stream or string of the file, however it is stored.
fn anywhere(path: &Path, needle: &str) -> bool {
    let doc = Document::load(path).unwrap();
    doc.objects.values().any(|object| match object {
        Object::Stream(s) => {
            let data = s
                .decompressed_content()
                .unwrap_or_else(|_| s.content.clone());
            String::from_utf8_lossy(&data).contains(needle)
        }
        Object::String(b, _) => String::from_utf8_lossy(b).contains(needle),
        _ => false,
    })
}

/// The average brightness, 0 to 255, of a region of a rendered page.
fn brightness(lab: &Lab, path: &Path, page: u32, r: Region) -> f32 {
    let image = lab
        .engine
        .render_page(path, None, PageIndex::new(page), Dpi::new(72).unwrap())
        .unwrap();
    let (w, h) = (image.width as usize, image.height as usize);
    let (x0, x1) = (
        (r.x * w as f32) as usize,
        ((r.x + r.width) * w as f32) as usize,
    );
    let (y0, y1) = (
        (r.y * h as f32) as usize,
        ((r.y + r.height) * h as f32) as usize,
    );
    let mut sum = 0f64;
    let mut n = 0usize;
    for y in y0..y1.min(h) {
        for x in x0..x1.min(w) {
            let px = &image.rgba[(y * w + x) * 4..(y * w + x) * 4 + 3];
            sum += f64::from(px[0]) + f64::from(px[1]) + f64::from(px[2]);
            n += 3;
        }
    }
    (sum / n.max(1) as f64) as f32
}

/// A Letter page's region from user space points, with y counted from the bottom.
fn at(x0: f32, y0: f32, x1: f32, y1: f32) -> Region {
    region(
        x0 / 612.0,
        1.0 - y1 / 792.0,
        (x1 - x0) / 612.0,
        (y1 - y0) / 792.0,
    )
}

#[test]
fn an_area_removes_the_text_under_it_and_covers_it() {
    let lab = Lab::new();
    let source = lab.doc(
        "a.pdf",
        &pdfs::pdf_with_positioned_text(&[
            (100.0, 700.0, "Public line one"),
            (100.0, 650.0, "Account 1234 TOPSECRET"),
            (100.0, 600.0, "Public line three"),
        ]),
    );
    let before = fs::read(&source.path).unwrap();
    let covered = at(95.0, 645.0, 400.0, 665.0);
    let out = lab
        .run(
            options(|o| o.areas = vec![area(1, covered)]),
            source.clone(),
        )
        .unwrap();
    assert_eq!(name(&out[0]), "a (redacted).pdf");
    let text = &lab.texts(&out[0].path, None)[0];
    assert!(
        text.contains("Public line one") && text.contains("Public line three"),
        "{text}"
    );
    assert!(
        !text.contains("TOPSECRET") && !text.contains("Account"),
        "{text}"
    );
    assert!(!anywhere(&out[0].path, "TOPSECRET"));
    assert!(brightness(&lab, &out[0].path, 0, covered) < 20.0);
    // The lines around it are as bright as before.
    assert!(brightness(&lab, &out[0].path, 0, at(95.0, 695.0, 300.0, 712.0)) > 200.0);
    assert_eq!(fs::read(&source.path).unwrap(), before);
    lab.tidy();
}

#[test]
fn words_are_found_on_every_page_and_only_those_characters_go() {
    let lab = Lab::new();
    let source = lab.doc(
        "w.pdf",
        &pdfs::pdf_from_pages(
            &[
                "BT /F1 12 Tf 100 700 Td (the Secret code and more) Tj ET\n",
                "BT /F1 12 Tf 100 700 Td (SECRET again, then nothing) Tj ET\nBT /F1 12 Tf 100 500 Td (unrelated) Tj ET\n",
            ],
            0,
        ),
    );
    let out = lab
        .run(options(|o| o.terms = vec!["secret".to_owned()]), source)
        .unwrap();
    let texts = lab.texts(&out[0].path, None);
    assert!(
        texts[0].contains("the") && texts[0].contains("code and more"),
        "{texts:?}"
    );
    assert!(
        texts[1].contains("again, then nothing") && texts[1].contains("unrelated"),
        "{texts:?}"
    );
    assert!(
        texts.iter().all(|t| !t.to_lowercase().contains("secret")),
        "{texts:?}"
    );
    assert!(!anywhere(&out[0].path, "Secret"));
    assert!(!anywhere(&out[0].path, "SECRET"));
    assert_eq!(out[0].pages, Some(2));
    lab.tidy();
}

#[test]
fn kinds_of_text_are_found_by_their_shape() {
    let lab = Lab::new();
    let source = lab.doc(
        "k.pdf",
        &pdfs::pdf_with_positioned_text(&[
            (100.0, 700.0, "Mail ada@example.org now"),
            (100.0, 670.0, "Card 4111 1111 1111 1111 ok"),
            (100.0, 640.0, "Pay GB82 WEST 1234 5698 7654 32 soon"),
            (100.0, 610.0, "Call +1 (555) 123-4567 today"),
            (100.0, 580.0, "Reference 4111 1111 1111 1112 stays"),
        ]),
    );
    let out = lab
        .run(
            options(|o| {
                o.presets = vec![
                    RedactPreset::Email,
                    RedactPreset::CardNumber,
                    RedactPreset::Iban,
                    RedactPreset::Phone,
                ];
            }),
            source,
        )
        .unwrap();
    let text = lab.texts(&out[0].path, None).remove(0);
    for gone in ["ada@example.org", "4111 1111 1111 1111", "GB82", "555"] {
        assert!(!text.contains(gone), "{gone} is still there: {text}");
    }
    // A number that fails the card check is not a card number.
    assert!(text.contains("4111 1111 1111 1112"), "{text}");
    assert!(
        text.contains("Mail") && text.contains("Card") && text.contains("today"),
        "{text}"
    );
    lab.tidy();
}

#[test]
fn custom_patterns_and_case_sensitivity_work() {
    let lab = Lab::new();
    let source = lab.doc(
        "p.pdf",
        &pdfs::pdf_with_positioned_text(&[(100.0, 700.0, "INV-1001 inv-1002 INV-77")]),
    );
    let out = lab
        .run(
            options(|o| {
                o.patterns = vec![r"INV-\d{4}".to_owned()];
                o.case_sensitive = true;
            }),
            source,
        )
        .unwrap();
    let text = lab.texts(&out[0].path, None).remove(0);
    assert!(!text.contains("INV-1001"), "{text}");
    assert!(
        text.contains("inv-1002") && text.contains("INV-77"),
        "{text}"
    );
    lab.tidy();
}

#[test]
fn a_page_whose_content_cannot_be_cleaned_becomes_a_picture_with_the_area_filled() {
    let lab = Lab::new();
    // The font has no widths and no known name, so the text cannot be placed character by
    // character.
    let content = "BT /F1 12 Tf 100 700 Td (Hello TOPSECRET) Tj ET\nBT /F1 12 Tf 100 300 Td (Elsewhere) Tj ET\n";
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Odd >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    let source = lab.doc("o.pdf", &pdfs::serialize(&objects, ""));
    let covered = at(95.0, 690.0, 300.0, 715.0);
    let out = lab
        .run(options(|o| o.areas = vec![area(1, covered)]), source)
        .unwrap();
    // The page is a picture now: no text at all, and the area is filled.
    assert_eq!(lab.texts(&out[0].path, None)[0], "");
    assert!(!anywhere(&out[0].path, "TOPSECRET"));
    assert!(!anywhere(&out[0].path, "Elsewhere"));
    assert!(brightness(&lab, &out[0].path, 0, covered) < 20.0);
    lab.tidy();
}

#[test]
fn a_picture_under_an_area_loses_its_pixels() {
    let lab = Lab::new();
    // The test picture is 2 by 2 pixels, red green blue white, stretched over 200 by 200 points.
    let source = lab.doc(
        "i.pdf",
        &pdfs::pdf_from_content("q 200 0 0 200 100 400 cm /Im1 Do Q\n", 0),
    );
    let top_left = at(105.0, 505.0, 195.0, 595.0);
    let before = brightness(&lab, &source.path, 0, top_left);
    assert!(
        before > 60.0,
        "the red pixel should be bright enough: {before}"
    );
    let out = lab
        .run(options(|o| o.areas = vec![area(1, top_left)]), source)
        .unwrap();
    assert!(brightness(&lab, &out[0].path, 0, top_left) < 20.0);
    // The other pixels are still colored.
    assert!(brightness(&lab, &out[0].path, 0, at(205.0, 505.0, 295.0, 595.0)) > 60.0);
    lab.tidy();
}

#[test]
fn a_protected_file_stays_protected_and_clean() {
    let lab = Lab::new();
    let plain = lab.put(
        "s.pdf",
        &pdfs::pdf_with_positioned_text(&[(100.0, 700.0, "Hidden TOPSECRET words")]),
    );
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "pw");
    let mut input = InputDoc::new(&locked);
    input.password = Some(Password::new("pw"));
    let out = lab
        .run(options(|o| o.terms = vec!["topsecret".to_owned()]), input)
        .unwrap();
    let password = Password::new("pw");
    let text = lab.texts(&out[0].path, Some(&password)).remove(0);
    assert!(text.contains("Hidden") && text.contains("words"), "{text}");
    assert!(!text.contains("TOPSECRET"), "{text}");
    use application::ports::PdfReader;
    assert!(lab.engine.open_info(&out[0].path, None).is_err());
    lab.tidy();
}

#[test]
fn document_information_is_cleared_unless_asked_to_stay() {
    let lab = Lab::new();
    let bytes = pdfs::pdf_with_metadata("Board minutes", "Ada Lovelace");
    let drawn = vec![area(1, at(30.0, 690.0, 200.0, 730.0))];
    let areas = drawn.clone();
    let out = lab
        .run(options(move |o| o.areas = areas), lab.doc("m.pdf", &bytes))
        .unwrap();
    assert!(!anywhere(&out[0].path, "Ada Lovelace"));
    assert!(!anywhere(&out[0].path, "Board minutes"));
    let kept = lab
        .run(
            options(move |o| {
                o.areas = drawn;
                o.strip_metadata = false;
            }),
            lab.doc("m2.pdf", &bytes),
        )
        .unwrap();
    assert!(anywhere(&kept[0].path, "Ada Lovelace"));
    lab.tidy();
}

#[test]
fn nothing_found_is_an_error_and_writes_nothing() {
    let lab = Lab::new();
    let source = lab.doc("n.pdf", &pdfs::pdf_with_texts(&["Nothing special here"]));
    let err = lab
        .run(
            options(|o| o.terms = vec!["absent".to_owned()]),
            source.clone(),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(err.message.contains("Nothing was found"), "{}", err.message);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    let nothing = lab.run(options(|_| {}), source).unwrap_err();
    assert_eq!(nothing.code, ErrorCode::OptionInvalid);
    lab.tidy();
}

#[test]
fn a_pattern_that_is_not_valid_is_refused_before_any_work() {
    let lab = Lab::new();
    let source = lab.doc("v.pdf", &pdfs::pdf_with_texts(&["text"]));
    let err = lab
        .run(options(|o| o.patterns = vec!["(oops".to_owned()]), source)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(err.message.contains("(oops"), "{}", err.message);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}

#[test]
fn rotated_pages_take_areas_as_they_look() {
    let lab = Lab::new();
    let source = lab.doc(
        "r.pdf",
        &pdfs::pdf_from_content(
            "BT /F1 12 Tf 100 700 Td (keep this) Tj ET\nBT /F1 12 Tf 100 400 Td (TOPSECRET) Tj ET\n",
            90,
        ),
    );
    // Turned a quarter: the text that sits at user (100, 400) is shown at the displayed place
    // found by rendering. Search for it instead of working it out by hand.
    let out = lab
        .run(options(|o| o.terms = vec!["topsecret".to_owned()]), source)
        .unwrap();
    let text = lab.texts(&out[0].path, None).remove(0);
    assert!(
        text.contains("keep this") && !text.contains("TOPSECRET"),
        "{text}"
    );
    assert!(!anywhere(&out[0].path, "TOPSECRET"));
    lab.tidy();
}
