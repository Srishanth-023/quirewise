//! End-to-end tests for Word, PowerPoint, Excel and HTML to PDF: the real RunTool, the worker
//! engines and the file store. They run Microsoft Office and Edge themselves, so each test does
//! nothing on a computer that lacks the program it needs.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use application::ports::{HtmlConverter, OfficeConverter, OfficeKind};
use application::{InputDoc, UnitRequest, UnitRunner};
use common::{Lab, name};
use domain::ErrorCode;
use domain::PageRange;
use domain::naming::OutputNamingPattern;
use domain::tools::office::{ExcelToPdfOptions, HtmlToPdfOptions, OfficeEngine, WordToPdfOptions};
use domain::{ToolId, ToolOptions};
use quirewise_engine_html_webview2::EdgePrinter;
use quirewise_engine_office_msoffice::MsOffice;
use std::fs;
use test_support::office::{docx_with_pages, xlsx_with_sheets};

fn have(kind: OfficeKind) -> bool {
    MsOffice::new().is_available(kind)
}

fn word(f: impl FnOnce(&mut WordToPdfOptions)) -> ToolOptions {
    let mut options = WordToPdfOptions::default();
    f(&mut options);
    ToolOptions::WordToPdf(options)
}

#[test]
fn a_word_document_becomes_a_pdf_with_its_pages_and_the_original_stays() {
    if !have(OfficeKind::Word) {
        return;
    }
    let lab = Lab::new();
    let source = lab.doc("letter.docx", &docx_with_pages(&["one", "two", "three"]));
    let before = fs::read(&source.path).unwrap();
    let out = lab.run(word(|_| {}), source.clone()).unwrap();
    assert_eq!(name(&out[0]), "letter (word).pdf");
    assert_eq!(out[0].pages, Some(3));
    let texts = lab.texts(&out[0].path, None);
    assert!(
        texts[0].contains("one") && texts[2].contains("three"),
        "{texts:?}"
    );
    assert_eq!(fs::read(&source.path).unwrap(), before);
    lab.tidy();
}

#[test]
fn only_the_pages_asked_for_are_kept() {
    if !have(OfficeKind::Word) {
        return;
    }
    let lab = Lab::new();
    let source = lab.doc("letter.docx", &docx_with_pages(&["one", "two", "three"]));
    let out = lab
        .run(
            word(|o| {
                o.pages = PageRange::parse("2-3").unwrap();
                o.engine = OfficeEngine::MsOffice;
            }),
            source.clone(),
        )
        .unwrap();
    assert_eq!(out[0].pages, Some(2));
    let texts = lab.texts(&out[0].path, None);
    assert!(texts[0].contains("two"), "{texts:?}");

    let err = lab
        .run(word(|o| o.pages = PageRange::parse("9").unwrap()), source)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::RangeOutOfBounds);
    lab.tidy();
}

#[test]
fn a_damaged_document_fails_that_file_with_a_message_and_leaves_nothing() {
    if !have(OfficeKind::Word) {
        return;
    }
    let lab = Lab::new();
    let bad = lab.doc("bad.docx", b"PK\x03\x04 this is not a document");
    let err = lab.run(word(|_| {}), bad).unwrap_err();
    assert_eq!(err.code, ErrorCode::OutputVerifyFailed);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}

#[test]
fn a_file_that_is_not_an_office_document_is_refused_by_kind() {
    let lab = Lab::new();
    let pdf = lab.doc("a.pdf", &test_support::pdfs::pdf_with_texts(&["x"]));
    let err = lab.run(word(|_| {}), pdf).unwrap_err();
    assert_eq!(err.code, ErrorCode::InputNotPdf);
    let renamed = lab.doc("a.docx", b"plain text pretending to be a document");
    let err = lab.run(word(|_| {}), renamed).unwrap_err();
    assert_eq!(err.code, ErrorCode::InputNotPdf);
}

#[test]
fn a_workbook_prints_the_sheets_chosen() {
    if !have(OfficeKind::Sheet) {
        return;
    }
    let lab = Lab::new();
    let book = xlsx_with_sheets(&[
        ("First", vec![vec!["alpha", "beta"]]),
        ("Second", vec![vec!["gamma"]]),
        ("Third", vec![vec!["delta"]]),
    ]);
    let source = lab.doc("book.xlsx", &book);
    let all = lab
        .run(
            ToolOptions::defaults_for(ToolId::ExcelToPdf),
            source.clone(),
        )
        .unwrap();
    assert_eq!(all[0].pages, Some(3));
    let some = lab
        .run(
            ToolOptions::ExcelToPdf(ExcelToPdfOptions {
                sheets: PageRange::parse("2-3").unwrap(),
                ..ExcelToPdfOptions::default()
            }),
            source,
        )
        .unwrap();
    assert_eq!(some[0].pages, Some(2));
    let texts = lab.texts(&some[0].path, None);
    assert!(
        texts[0].contains("gamma") && texts[1].contains("delta"),
        "{texts:?}"
    );
    lab.tidy();
}

// ---- HTML ---------------------------------------------------------------------------------

fn html(f: impl FnOnce(&mut HtmlToPdfOptions)) -> ToolOptions {
    let mut options = HtmlToPdfOptions::default();
    f(&mut options);
    ToolOptions::HtmlToPdf(options)
}

fn have_edge() -> bool {
    EdgePrinter::new().is_available()
}

#[test]
fn a_saved_web_page_prints_to_pdf() {
    if !have_edge() {
        return;
    }
    let lab = Lab::new();
    let source = lab.doc(
        "page.html",
        b"<!doctype html><html><body><h1>Quarterly report</h1><p>All good.</p></body></html>",
    );
    let before = fs::read(&source.path).unwrap();
    let out = lab.run(html(|_| {}), source.clone()).unwrap();
    assert_eq!(name(&out[0]), "page (web page).pdf");
    let texts = lab.texts(&out[0].path, None);
    assert!(texts[0].contains("Quarterly report"), "{texts:?}");
    let sizes = lab.sizes(&out[0].path);
    assert!((sizes[0].0 - 595.3).abs() < 1.0, "{sizes:?}");
    assert_eq!(fs::read(&source.path).unwrap(), before);
    lab.tidy();
}

#[test]
fn a_web_address_needs_consent_and_then_needs_no_file() {
    if !have_edge() {
        return;
    }
    let lab = Lab::new();
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        for stream in server.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buffer = [0u8; 2048];
            let _ = stream.read(&mut buffer);
            let body = "<html><body><h1>From the web</h1></body></html>";
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    let url = format!("http://127.0.0.1:{port}/index.html");
    let request = |allow: bool| UnitRequest {
        tool: ToolId::HtmlToPdf,
        options: html(|o| {
            o.url = Some(url.clone());
            o.allow_network = allow;
        }),
        inputs: Vec::<InputDoc>::new(),
        output_dir: Some(lab.out()),
        naming_pattern: OutputNamingPattern::default(),
    };
    let run = |allow| {
        lab.runner.run_unit(
            &request(allow),
            &application::CancellationToken::new(),
            &application::NoProgress,
        )
    };
    let refused = run(false).unwrap_err();
    assert_eq!(refused.code, ErrorCode::NetworkConsentRequired);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);

    let out = run(true).unwrap();
    assert_eq!(name(&out[0]), "127.0.0.1 (web page).pdf");
    assert!(lab.texts(&out[0].path, None)[0].contains("From the web"));
    lab.tidy();
}

#[test]
fn a_run_without_files_or_an_address_is_refused() {
    let lab = Lab::new();
    let err = lab
        .runner
        .run_unit(
            &UnitRequest {
                tool: ToolId::HtmlToPdf,
                options: ToolOptions::defaults_for(ToolId::HtmlToPdf),
                inputs: Vec::new(),
                output_dir: Some(lab.out()),
                naming_pattern: OutputNamingPattern::default(),
            },
            &application::CancellationToken::new(),
            &application::NoProgress,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
}

// ---- Inside the confined worker process --------------------------------------------------

fn confined(
    request: UnitRequest,
) -> Result<Vec<application::OutputArtifact>, application::AppError> {
    infrastructure::WorkerSupervisor::new(
        std::path::PathBuf::from(env!("CARGO_BIN_EXE_quirewise-worker")),
        domain::limits::SafetyLimits::default(),
    )
    .run_unit(
        &request,
        &application::CancellationToken::new(),
        &application::NoProgress,
    )
}

#[test]
fn the_programs_also_work_from_the_confined_worker_process() {
    let lab = Lab::new();
    if have_edge() {
        let page = lab.put("page.html", b"<html><body><h1>Confined</h1></body></html>");
        let out = confined(UnitRequest {
            tool: ToolId::HtmlToPdf,
            options: html(|_| {}),
            inputs: vec![InputDoc::new(&page)],
            output_dir: Some(lab.out()),
            naming_pattern: OutputNamingPattern::default(),
        })
        .unwrap();
        assert!(lab.texts(&out[0].path, None)[0].contains("Confined"));
    }
    if have(OfficeKind::Word) {
        let docx = lab.put("memo.docx", &docx_with_pages(&["only page"]));
        let out = confined(UnitRequest {
            tool: ToolId::WordToPdf,
            options: word(|_| {}),
            inputs: vec![InputDoc::new(&docx)],
            output_dir: Some(lab.out()),
            naming_pattern: OutputNamingPattern::default(),
        })
        .unwrap();
        assert_eq!(out[0].pages, Some(1));
    }
}
