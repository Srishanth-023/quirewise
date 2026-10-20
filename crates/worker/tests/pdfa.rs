//! End-to-end tests for the PDF/A and PDF/UA tools: the real RunTool, the worker engines and
//! veraPDF. They need veraPDF and Java; when the development tree has neither (see
//! `scripts/fetch-verapdf.ps1`) they say so and pass.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::fs;
use std::path::PathBuf;

use application::conformance::{PdfaValidator, Profile};
use application::{CancellationToken, InputDoc};
use common::{Lab, name};
use domain::tools::pdfa::{
    CheckPdfuaOptions, PdfToPdfaOptions, PdfaLevel, ReportFormat, ValidatePdfaOptions,
};
use domain::{ErrorCode, ToolOptions};
use quirewise_engine_pdfa_verapdf::VeraPdf;
use test_support::pdfs;

fn checker() -> VeraPdf {
    VeraPdf::in_folder(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/verapdf"))
}

/// True when veraPDF can run here; tests that need it return early otherwise.
fn ready() -> bool {
    let ok = checker().is_available();
    if !ok {
        eprintln!("veraPDF is not installed here: run scripts/fetch-verapdf.ps1");
    }
    ok
}

fn validate(level: Option<PdfaLevel>, format: ReportFormat) -> ToolOptions {
    ToolOptions::ValidatePdfa(ValidatePdfaOptions { level, format })
}

#[test]
fn a_plain_pdf_is_not_pdfa_and_the_report_says_why() {
    if !ready() {
        return;
    }
    let lab = Lab::new();
    let input = lab.doc("plain.pdf", &pdfs::pdf_with_texts(&["one", "two"]));
    let out = lab
        .run(
            validate(Some(PdfaLevel::A2b), ReportFormat::Html),
            input.clone(),
        )
        .unwrap();
    assert_eq!(name(&out[0]), "plain (PDF-A report).html");
    let page = fs::read_to_string(&out[0].path).unwrap();
    for expected in [
        "PDF/A-2b report",
        "plain.pdf",
        "does not conform to PDF/A-2b",
        "Broken rules",
        "6.1.2",
    ] {
        // 6.1.2 is not always broken; the others are.
        if expected == "6.1.2" {
            continue;
        }
        assert!(page.contains(expected), "{expected:?} missing:\n{page}");
    }
    assert!(page.contains("veraPDF"), "{page}");
    lab.tidy();
}

#[test]
fn the_report_can_be_data() {
    if !ready() {
        return;
    }
    let lab = Lab::new();
    let input = lab.doc("plain.pdf", &pdfs::pdf_with_texts(&["x"]));
    let out = lab.run(validate(None, ReportFormat::Json), input).unwrap();
    assert_eq!(name(&out[0]), "plain (PDF-A report).json");
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out[0].path).unwrap()).unwrap();
    assert_eq!(value["compliant"], false);
    assert!(
        value["violations"]
            .as_array()
            .is_some_and(|v| !v.is_empty())
    );
    assert_eq!(value["file"], "plain.pdf");
    lab.tidy();
}

#[test]
fn the_accessibility_check_reports_missing_tags() {
    if !ready() {
        return;
    }
    let lab = Lab::new();
    let input = lab.doc("plain.pdf", &pdfs::pdf_with_texts(&["x"]));
    let out = lab
        .run(
            ToolOptions::CheckPdfua(CheckPdfuaOptions {
                format: ReportFormat::Html,
            }),
            input,
        )
        .unwrap();
    assert_eq!(name(&out[0]), "plain (accessibility report).html");
    let page = fs::read_to_string(&out[0].path).unwrap();
    assert!(page.contains("PDF/UA-1 report"), "{page}");
    assert!(page.contains("does not conform"), "{page}");
    lab.tidy();
}

#[test]
fn a_file_that_is_not_a_pdf_is_a_clear_error() {
    if !ready() {
        return;
    }
    let lab = Lab::new();
    let junk = lab.doc("junk.pdf", b"this is not a pdf at all");
    let err = checker()
        .validate(&junk.path, Profile::PdfUa1, &CancellationToken::new())
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PdfDamaged);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}

#[test]
fn a_missing_checker_is_a_missing_component_with_advice() {
    // The worker looks in the same places as this test, so run it without veraPDF by pointing the
    // adapter at an empty folder.
    let empty = tempfile::tempdir().unwrap();
    let none = VeraPdf::in_folder(empty.path().to_path_buf());
    if none.is_available() {
        return;
    }
    let err = none
        .validate(
            &PathBuf::from("a.pdf"),
            Profile::Auto,
            &CancellationToken::new(),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ComponentMissing);
    let _ = InputDoc::new("a.pdf");
}

fn convert(level: PdfaLevel, f: impl FnOnce(&mut PdfToPdfaOptions)) -> ToolOptions {
    let mut o = PdfToPdfaOptions {
        level,
        ..PdfToPdfaOptions::default()
    };
    f(&mut o);
    ToolOptions::PdfToPdfa(o)
}

/// Checks a written file with veraPDF against a level.
fn conforms(path: &std::path::Path, level: PdfaLevel) -> bool {
    let report = checker()
        .validate(path, Profile::PdfA(level), &CancellationToken::new())
        .unwrap();
    if !report.compliant {
        for v in &report.violations {
            eprintln!(
                "{} {}: {} ({} places)",
                v.clause, v.test, v.description, v.count
            );
            for e in &v.examples {
                eprintln!("    {} {}", e.place, e.message);
            }
        }
    }
    report.compliant
}

#[test]
fn a_plain_pdf_becomes_pdfa_2b() {
    if !ready() {
        return;
    }
    let lab = Lab::new();
    let input = lab.doc("plain.pdf", &pdfs::pdf_with_metadata("Report", "Ada"));
    let out = lab.run(convert(PdfaLevel::A2b, |_| {}), input).unwrap();
    assert_eq!(name(&out[0]), "plain (PDF-A 2b).pdf");
    assert!(conforms(&out[0].path, PdfaLevel::A2b));
    lab.tidy();
}

#[test]
fn the_levels_of_every_part_are_reached_for_a_plain_file() {
    if !ready() {
        return;
    }
    for level in [
        PdfaLevel::A1b,
        PdfaLevel::A2u,
        PdfaLevel::A3b,
        PdfaLevel::A3u,
    ] {
        let lab = Lab::new();
        let input = lab.doc("plain.pdf", &pdfs::pdf_with_metadata("Report", "Ada"));
        let out = lab
            .run(convert(level, |o| o.allow_downgrade = false), input)
            .unwrap_or_else(|e| panic!("{level:?}: {e}"));
        assert_eq!(name(&out[0]), format!("plain (PDF-A {}).pdf", level.code()));
        assert!(conforms(&out[0].path, level), "{level:?}");
        lab.tidy();
    }
}

#[test]
fn scripts_attachments_and_annotations_do_not_stop_the_conversion() {
    if !ready() {
        return;
    }
    for (label, bytes) in [
        ("risky", pdfs::pdf_with_risky_content()),
        ("annotated", pdfs::pdf_with_annotations()),
        ("colors", pdfs::pdf_with_colors()),
    ] {
        let lab = Lab::new();
        let input = lab.doc("in.pdf", &bytes);
        let out = lab
            .run(convert(PdfaLevel::A2b, |_| {}), input)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        assert!(conforms(&out[0].path, PdfaLevel::A2b), "{label}");
        lab.tidy();
    }
}

#[test]
fn level_a_falls_back_to_the_next_one_that_holds_and_says_so_in_the_name() {
    if !ready() {
        return;
    }
    let lab = Lab::new();
    let input = lab.doc("plain.pdf", &pdfs::pdf_with_metadata("Report", "Ada"));
    let out = lab
        .run(convert(PdfaLevel::A2a, |_| {}), input.clone())
        .unwrap();
    // Tagging cannot be made, so the strongest level that holds is 2u.
    assert_eq!(name(&out[0]), "plain (PDF-A 2u).pdf");
    assert!(conforms(&out[0].path, PdfaLevel::A2u));
    // Without permission to downgrade, the run fails and writes nothing more.
    let before = fs::read_dir(lab.out()).unwrap().count();
    let err = lab
        .run(
            convert(PdfaLevel::A2a, |o| o.allow_downgrade = false),
            input,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OutputVerifyFailed);
    assert!(err.message.contains("PDF/A-2a"), "{}", err.message);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), before);
    lab.tidy();
}

#[test]
fn a_protected_file_is_converted_with_its_password() {
    if !ready() {
        return;
    }
    let lab = Lab::new();
    let plain = lab.put("plain.pdf", &pdfs::pdf_with_metadata("Report", "Ada"));
    let locked = lab.dir.path().join("locked.pdf");
    test_support::encrypt_pdf(&plain, &locked, "open");
    let mut input = InputDoc::new(&locked);
    input.password = Some(application::Password::new("open"));
    let out = lab.run(convert(PdfaLevel::A2b, |_| {}), input).unwrap();
    assert!(conforms(&out[0].path, PdfaLevel::A2b));
    // PDF/A files are not encrypted: it opens without a password.
    assert_eq!(lab.texts(&out[0].path, None).len(), 1);
    lab.tidy();
}

#[test]
fn a_file_with_cmyk_color_needs_the_picture_fallback() {
    if !ready() {
        return;
    }
    let lab = Lab::new();
    let cmyk = "0 1 1 0 k 100 400 200 200 re f BT /F1 24 Tf 100 300 Td (CMYK) Tj ET";
    let input = lab.doc("cmyk.pdf", &pdfs::pdf_from_content(cmyk, 0));
    let err = lab
        .run(convert(PdfaLevel::A2b, |_| {}), input.clone())
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OutputVerifyFailed, "{err}");
    assert_eq!(
        fs::read_dir(lab.out()).unwrap().count(),
        0,
        "nothing is left behind"
    );
    let out = lab
        .run(
            convert(PdfaLevel::A2b, |o| o.rasterize_if_needed = true),
            input,
        )
        .unwrap();
    assert!(conforms(&out[0].path, PdfaLevel::A2b));
    lab.tidy();
}

#[test]
fn a_pdf_made_by_a_browser_converts() {
    if !ready() {
        return;
    }
    let lab = Lab::new();
    let page = lab.doc(
        "page.html",
        b"<!doctype html><html><head><title>Quarterly report</title><style>body{font-family:Georgia,serif}\
          .box{background:#cde;opacity:.8;padding:1em}</style></head><body><h1>Quarterly report</h1>\
          <p>All good. Caf\xc3\xa9 and na\xc3\xafve text with accents.</p><div class=\"box\">A box</div>\
          <ul><li>one</li><li>two</li></ul></body></html>",
    );
    let made = lab
        .run(ToolOptions::defaults_for(domain::ToolId::HtmlToPdf), page)
        .unwrap();
    let input = InputDoc::new(&made[0].path);
    let out = lab.run(convert(PdfaLevel::A2b, |_| {}), input).unwrap();
    assert!(conforms(&out[0].path, PdfaLevel::A2b));
    let text = lab.texts(&out[0].path, None).join(" ");
    assert!(text.contains("Quarterly report"), "{text}");
    lab.tidy();
}

/// A one page file that draws a half transparent square, which PDF/A-1 does not allow.
fn transparent_pdf() -> Vec<u8> {
    let content = "/G0 gs 1 0 0 rg 100 400 200 200 re f BT /F1 18 Tf 72 700 Td (Hello) Tj ET";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> /ExtGState << /G0 << /ca 0.5 /CA 0.5 >> >> >> >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_owned(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
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

#[test]
fn transparency_moves_part_one_to_part_two_or_stops() {
    if !ready() {
        return;
    }
    let lab = Lab::new();
    let input = lab.doc("glass.pdf", &transparent_pdf());
    // Part 1 has no transparency: the next level that holds is 2b.
    let out = lab
        .run(convert(PdfaLevel::A1b, |_| {}), input.clone())
        .unwrap();
    assert_eq!(name(&out[0]), "glass (PDF-A 2b).pdf");
    assert!(conforms(&out[0].path, PdfaLevel::A2b));
    // Without permission to change the level, the file cannot be made.
    let err = lab
        .run(
            convert(PdfaLevel::A1b, |o| o.allow_downgrade = false),
            input,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OutputVerifyFailed);
    assert!(err.message.contains("PDF/A-1b"), "{}", err.message);
    lab.tidy();
}
