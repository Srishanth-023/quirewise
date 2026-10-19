//! End-to-end tests for Compare: the real RunTool, worker engines and file store, on generated
//! pages. The report is a PDF, read back through its text and its rendered pages.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use application::{InputDoc, Password};
use common::{Lab, name};
use domain::tools::compare::{CompareMode, CompareOptions};
use domain::{ErrorCode, ToolOptions};
use std::fs;
use test_support::{encrypt_pdf, pdfs};

fn options(f: impl FnOnce(&mut CompareOptions)) -> ToolOptions {
    let mut o = CompareOptions::default();
    f(&mut o);
    ToolOptions::Compare(o)
}

fn page(lines: &[&str]) -> String {
    let mut content = String::new();
    for (i, line) in lines.iter().enumerate() {
        content.push_str(&format!(
            "BT /F1 12 Tf 100 {} Td ({line}) Tj ET\n",
            700 - 20 * i
        ));
    }
    content
}

fn pair(lab: &Lab, a: &[&[&str]], b: &[&[&str]]) -> (InputDoc, InputDoc) {
    let pages = |docs: &[&[&str]]| docs.iter().map(|p| page(p)).collect::<Vec<_>>();
    let (pa, pb) = (pages(a), pages(b));
    let ra: Vec<&str> = pa.iter().map(String::as_str).collect();
    let rb: Vec<&str> = pb.iter().map(String::as_str).collect();
    (
        lab.doc("old.pdf", &pdfs::pdf_from_pages(&ra, 0)),
        lab.doc("new.pdf", &pdfs::pdf_from_pages(&rb, 0)),
    )
}

#[test]
fn text_changes_are_listed_side_by_side() {
    let lab = Lab::new();
    let (a, b) = pair(
        &lab,
        &[&["Intro line", "Price is 100 dollars", "Closing line"]],
        &[&[
            "Intro line",
            "Price is 120 dollars",
            "Closing line",
            "New footnote",
        ]],
    );
    let before = fs::read(&a.path).unwrap();
    let out = lab
        .run_all(options(|o| o.mode = CompareMode::Text), vec![a.clone(), b])
        .unwrap();
    assert_eq!(name(&out[0]), "old (compared).pdf");
    assert_eq!(out[0].pages, Some(1));
    let text = lab.texts(&out[0].path, None).remove(0);
    for expected in [
        "Comparison report",
        "old.pdf",
        "new.pdf",
        "Text changes",
        "Price is 100 dollars",
        "Price is 120 dollars",
        "New footnote",
        "0 lines removed, 1 added, 1 rewritten",
    ] {
        assert!(text.contains(expected), "{expected:?} missing: {text}");
    }
    assert_eq!(fs::read(&a.path).unwrap(), before);
    lab.tidy();
}

#[test]
fn identical_files_have_no_differences() {
    let lab = Lab::new();
    let (a, b) = pair(&lab, &[&["Same", "Same too"]], &[&["Same", "Same too"]]);
    let out = lab.run_all(options(|_| {}), vec![a, b]).unwrap();
    assert_eq!(out[0].pages, Some(1));
    let text = lab.texts(&out[0].path, None).remove(0);
    assert!(text.contains("Text: no differences"), "{text}");
    assert!(text.contains("the pages look the same"), "{text}");
    lab.tidy();
}

#[test]
fn pages_that_look_different_get_a_picture_in_red_and_blue() {
    let lab = Lab::new();
    // The first page is the same; on the second, a block moves.
    let a = lab.doc(
        "old.pdf",
        &pdfs::pdf_from_pages(
            &[
                "BT /F1 12 Tf 100 700 Td (steady) Tj ET\n",
                "BT /F1 24 Tf 100 700 Td (BLOCK) Tj ET\n",
            ],
            0,
        ),
    );
    let b = lab.doc(
        "new.pdf",
        &pdfs::pdf_from_pages(
            &[
                "BT /F1 12 Tf 100 700 Td (steady) Tj ET\n",
                "BT /F1 24 Tf 100 300 Td (BLOCK) Tj ET\n",
            ],
            0,
        ),
    );
    let out = lab
        .run_all(options(|o| o.mode = CompareMode::Visual), vec![a, b])
        .unwrap();
    // The summary page and one picture page, for page 2 only.
    assert_eq!(out[0].pages, Some(2));
    let summary = lab.texts(&out[0].path, None).remove(0);
    assert!(summary.contains("1 pages differ"), "{summary}");
    assert!(summary.contains("Page 2:"), "{summary}");
    assert!(!summary.contains("Page 1:"), "{summary}");
    let look = lab.look(&out[0].path, 1, None);
    assert!(
        look.colored > 50,
        "the overlay should show red and blue: {look:?}"
    );
    lab.tidy();
}

#[test]
fn files_of_different_length_are_compared_page_by_page() {
    let lab = Lab::new();
    let (a, b) = pair(
        &lab,
        &[&["First page"]],
        &[&["First page"], &["Second page only in the new file"]],
    );
    let out = lab
        .run_all(options(|o| o.mode = CompareMode::Text), vec![a, b])
        .unwrap();
    let text = lab.texts(&out[0].path, None).remove(0);
    assert!(text.contains("Pages compared: 2"), "{text}");
    assert!(text.contains("Second page only in the new file"), "{text}");
    assert!(
        text.contains("(1 pages)") && text.contains("(2 pages)"),
        "{text}"
    );
    lab.tidy();
}

#[test]
fn protected_files_are_compared_with_their_own_passwords() {
    let lab = Lab::new();
    let plain_a = lab.put("a.pdf", &pdfs::pdf_from_content(&page(&["Alpha line"]), 0));
    let plain_b = lab.put("b.pdf", &pdfs::pdf_from_content(&page(&["Beta line"]), 0));
    let locked_a = lab.dir.path().join("la.pdf");
    let locked_b = lab.dir.path().join("lb.pdf");
    encrypt_pdf(&plain_a, &locked_a, "one");
    encrypt_pdf(&plain_b, &locked_b, "two");
    let mut a = InputDoc::new(&locked_a);
    a.password = Some(Password::new("one"));
    let mut b = InputDoc::new(&locked_b);
    b.password = Some(Password::new("two"));
    let out = lab
        .run_all(options(|o| o.mode = CompareMode::Text), vec![a, b])
        .unwrap();
    let text = lab.texts(&out[0].path, None).remove(0);
    assert!(
        text.contains("Alpha line") && text.contains("Beta line"),
        "{text}"
    );
    // The report is not protected: it holds text of both files, so it is the person's call.
    lab.tidy();
}

#[test]
fn exactly_two_files_are_needed() {
    let lab = Lab::new();
    let one = lab.doc("one.pdf", &pdfs::pdf_with_texts(&["x"]));
    let err = lab.run_all(options(|_| {}), vec![one.clone()]).unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(err.message.contains("two PDF files"), "{}", err.message);
    let three = lab.run_all(options(|_| {}), vec![one.clone(), one.clone(), one]);
    assert_eq!(three.unwrap_err().code, ErrorCode::OptionInvalid);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}

#[test]
fn bad_numbers_are_refused() {
    let lab = Lab::new();
    let (a, b) = pair(&lab, &[&["x"]], &[&["y"]]);
    let err = lab
        .run_all(options(|o| o.tolerance_percent = 90.0), vec![a, b])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    lab.tidy();
}
