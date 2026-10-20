//! End-to-end tests for the tools that use the language model: the real RunTool, the worker
//! engines and a stand-in model that answers over the loopback address like Ollama does.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use application::query::{DocumentQuery, QueryResult};
use application::{CancellationToken, InputDoc};
use common::{Lab, name};
use domain::tools::ai::{
    AiBackend, AiOptions, AiSummarizerOptions, SmartSplitOptions, SplitPart, SummaryFormat,
    SummaryLength, TranslatePdfOptions,
};
use domain::tools::export::PdfToMarkdownOptions;
use domain::tools::redact::RedactOptions;
use domain::{ErrorCode, ToolOptions};
use serde_json::{Value, json};
use test_support::pdfs;

/// A model that answers by what the instructions say, and remembers every request.
struct FakeModel {
    port: u16,
    seen: Arc<Mutex<Vec<(String, String)>>>,
}

fn field(text: &str, key: &str) -> Option<String> {
    let after = text.split(key).nth(1)?;
    Some(after.trim().to_owned())
}

/// What the stand-in answers to one request.
fn answer(system: &str, user: &str) -> String {
    if system.contains("split a document into parts") {
        // A new part starts on every page whose line says Invoice.
        let mut parts = Vec::new();
        for line in user.lines() {
            let Some(rest) = line.strip_prefix('[') else {
                continue;
            };
            let Some((page, text)) = rest.split_once("] ") else {
                continue;
            };
            if text.contains("Invoice") {
                parts
                    .push(json!({"start": page.parse::<u32>().unwrap(), "title": text.to_owned()}));
            }
        }
        return json!({ "parts": parts }).to_string();
    }
    if system.contains("You summarize one part") || system.contains("You write a summary") {
        let lang = if system.contains("Write in German") {
            "Polish-style letters: Zażółć gęślą jaźń."
        } else {
            "The report covers sales."
        };
        return format!("## Overview\n\n{lang}\n\n- First point\n- Second point");
    }
    if system.contains("You translate texts into") {
        let request: Value = serde_json::from_str(user).unwrap();
        let texts: Vec<Value> = request["texts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                json!({"id": t["id"], "text": format!("Zażółć: {}", t["text"].as_str().unwrap().to_uppercase())})
            })
            .collect();
        return json!({ "texts": texts }).to_string();
    }
    if system.contains("find personal and confidential details") {
        let items: Vec<&str> = ["Maria Gonzales", "Peter Invented"]
            .into_iter()
            .filter(|n| {
                field(user, "Text:").is_some() && user.contains(n) || n.contains("Invented")
            })
            .collect();
        return json!({ "items": items }).to_string();
    }
    if system.contains("You tidy Markdown text") {
        return user.replace("-\n", "").replace('\n', " ");
    }
    "I do not know.".to_owned()
}

impl FakeModel {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let log = Arc::clone(&log);
                std::thread::spawn(move || serve(stream, &log));
            }
        });
        Self { port, seen }
    }

    fn ai(&self) -> AiOptions {
        AiOptions {
            backend: AiBackend::Ollama,
            model: Some("fake:1b".into()),
            ollama_url: format!("http://127.0.0.1:{}", self.port),
            allow_ollama: true,
        }
    }

    fn requests(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    fn systems(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|(s, _)| s.clone())
            .collect()
    }
}

fn serve(stream: TcpStream, log: &Mutex<Vec<(String, String)>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    let path = first.split_whitespace().nth(1).unwrap_or("").to_owned();
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().unwrap();
        }
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).unwrap();
    let mut out = stream;
    if path == "/v1/models" {
        let json = r#"{"data":[{"id":"fake:1b"}]}"#;
        let _ = write!(
            out,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
            json.len()
        );
        return;
    }
    let request: Value = serde_json::from_slice(&body).unwrap();
    let system = request["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    let user = request["messages"][1]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    let reply = answer(&system, &user);
    log.lock().unwrap().push((system, user));
    let _ = write!(
        out,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
    );
    // The answer arrives in pieces, as a real model streams it.
    let chars: Vec<char> = reply.chars().collect();
    for piece in chars.chunks(40) {
        let text: String = piece.iter().collect();
        let data = json!({"choices":[{"delta":{"content": text}}]});
        let _ = write!(out, "data: {data}\n\n");
    }
    let _ = write!(out, "data: [DONE]\n\n");
}

fn sized(pages: &[Vec<(&str, f32)>]) -> Vec<u8> {
    pdfs::pdf_with_sized_lines(pages)
}

#[test]
fn smart_split_follows_what_the_model_says_and_names_the_parts() {
    let model = FakeModel::start();
    let lab = Lab::new();
    let input = lab.doc(
        "scans.pdf",
        &pdfs::pdf_with_texts(&[
            "Invoice 1001",
            "Items one",
            "Invoice 1002",
            "Invoice 1003",
            "Total",
        ]),
    );
    let out = lab
        .run(
            ToolOptions::SmartSplit(SmartSplitOptions {
                ai: model.ai(),
                instruction: "one file for each invoice".into(),
                parts: Vec::new(),
            }),
            input,
        )
        .unwrap();
    let pages: Vec<u32> = out.iter().map(|o| o.pages.unwrap()).collect();
    assert_eq!(pages, [2, 1, 2]);
    assert!(name(&out[0]).contains("Invoice 1001"), "{}", name(&out[0]));
    assert!(name(&out[2]).contains("Invoice 1003"));
    assert_eq!(lab.texts(&out[0].path, None), ["Invoice 1001", "Items one"]);
    assert_eq!(lab.texts(&out[2].path, None), ["Invoice 1003", "Total"]);
    assert_eq!(model.requests(), 1);
    lab.tidy();
}

#[test]
fn checked_parts_are_made_without_asking_the_model_again() {
    let model = FakeModel::start();
    let lab = Lab::new();
    let input = lab.doc("a.pdf", &pdfs::pdf_with_texts(&["a", "b", "c", "d"]));
    let out = lab
        .run(
            ToolOptions::SmartSplit(SmartSplitOptions {
                ai: model.ai(),
                instruction: String::new(),
                parts: vec![
                    SplitPart {
                        start: 1,
                        end: 1,
                        title: "First".into(),
                    },
                    SplitPart {
                        start: 3,
                        end: 4,
                        title: "Rest".into(),
                    },
                ],
            }),
            input,
        )
        .unwrap();
    assert_eq!(
        out.iter().map(|o| o.pages.unwrap()).collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(lab.texts(&out[1].path, None), ["c", "d"]);
    assert_eq!(model.requests(), 0);
}

#[test]
fn the_preview_query_proposes_the_same_parts() {
    let model = FakeModel::start();
    let lab = Lab::new();
    let input = lab.put(
        "p.pdf",
        &pdfs::pdf_with_texts(&["Invoice A", "x", "Invoice B"]),
    );
    let answer = lab
        .queries
        .query(
            &DocumentQuery::SmartSplit {
                path: input,
                password: None,
                options: SmartSplitOptions {
                    ai: model.ai(),
                    instruction: "per invoice".into(),
                    parts: Vec::new(),
                },
            },
            &CancellationToken::new(),
        )
        .unwrap();
    let QueryResult::SmartSplit(parts) = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(
        parts.iter().map(|p| (p.start, p.end)).collect::<Vec<_>>(),
        [(1, 2), (3, 3)]
    );
    let listed = lab
        .queries
        .query(
            &DocumentQuery::AiModels { ai: model.ai() },
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(listed, QueryResult::AiModels(vec!["fake:1b".into()]));
}

#[test]
fn the_summary_is_a_pdf_in_the_chosen_language_that_can_be_read_back() {
    let model = FakeModel::start();
    let lab = Lab::new();
    let input = lab.doc(
        "report.pdf",
        &pdfs::pdf_with_texts(&["Sales rose in the north.", "Costs fell in the south."]),
    );
    let out = lab
        .run(
            ToolOptions::AiSummarizer(AiSummarizerOptions {
                ai: model.ai(),
                language: "de".into(),
                length: SummaryLength::Short,
                format: SummaryFormat::Pdf,
                ..AiSummarizerOptions::default()
            }),
            input,
        )
        .unwrap();
    assert_eq!(name(&out[0]), "report (summary).pdf");
    let text = lab.texts(&out[0].path, None).join("\n");
    // The letters outside WinAnsi are drawn with an installed font and read back right.
    assert!(text.contains("Zażółć gęślą jaźń"), "{text}");
    assert!(
        text.contains("First point") && text.contains("Overview"),
        "{text}"
    );
    assert!(text.contains("report"), "the title: {text}");
    assert!(model.systems()[0].contains("Write in German"));
    lab.tidy();
}

#[test]
fn the_summary_can_be_markdown_too() {
    let model = FakeModel::start();
    let lab = Lab::new();
    let input = lab.doc("notes.pdf", &pdfs::pdf_with_texts(&["Some notes."]));
    let out = lab
        .run(
            ToolOptions::AiSummarizer(AiSummarizerOptions {
                ai: model.ai(),
                format: SummaryFormat::Markdown,
                ..AiSummarizerOptions::default()
            }),
            input,
        )
        .unwrap();
    assert_eq!(name(&out[0]), "notes (summary).md");
    let md = fs::read_to_string(&out[0].path).unwrap();
    assert!(md.starts_with("# notes\n\n## Overview"), "{md}");
}

#[test]
fn a_document_without_text_asks_for_ocr_and_asks_the_model_nothing() {
    let model = FakeModel::start();
    let lab = Lab::new();
    let input = lab.doc("blank.pdf", &pdfs::pdf_with_texts(&["", ""]));
    let err = lab
        .run(
            ToolOptions::AiSummarizer(AiSummarizerOptions {
                ai: model.ai(),
                ..AiSummarizerOptions::default()
            }),
            input,
        )
        .unwrap_err();
    assert!(err.message.contains("OCR"), "{}", err.message);
    assert_eq!(model.requests(), 0);
}

#[test]
fn translation_keeps_the_pages_and_removes_the_old_text() {
    let model = FakeModel::start();
    let lab = Lab::new();
    let input = lab.doc(
        "letter.pdf",
        &sized(&[
            vec![
                ("Annual Report", 24.0),
                ("Sales grew strongly this year.", 11.0),
            ],
            vec![("Outlook", 24.0), ("We expect more growth.", 11.0)],
        ]),
    );
    let before = lab.sizes(&input.path);
    let out = lab
        .run(
            ToolOptions::TranslatePdf(TranslatePdfOptions {
                ai: model.ai(),
                target_language: "pl".into(),
                ..TranslatePdfOptions::default()
            }),
            input,
        )
        .unwrap();
    assert_eq!(name(&out[0]), "letter (translated pl).pdf");
    assert_eq!(out[0].pages, Some(2));
    assert_eq!(lab.sizes(&out[0].path), before);
    let texts = lab.texts(&out[0].path, None);
    assert!(texts[0].contains("Zażółć: ANNUAL REPORT"), "{}", texts[0]);
    assert!(
        texts[0].contains("Zażółć: SALES GREW STRONGLY THIS YEAR."),
        "{}",
        texts[0]
    );
    assert!(texts[1].contains("OUTLOOK"));
    for page in &texts {
        assert!(
            !page.contains("Annual Report") && !page.contains("Sales grew"),
            "the old text is gone: {page}"
        );
    }
    assert!(model.systems()[0].contains("into Polish"));
    lab.tidy();
}

#[test]
fn translation_without_the_layout_is_a_new_flowing_document() {
    let model = FakeModel::start();
    let lab = Lab::new();
    let input = lab.doc(
        "flow.pdf",
        &sized(&[vec![("Title", 24.0), ("Some body text here.", 11.0)]]),
    );
    let out = lab
        .run(
            ToolOptions::TranslatePdf(TranslatePdfOptions {
                ai: model.ai(),
                target_language: "de".into(),
                preserve_layout: false,
                ..TranslatePdfOptions::default()
            }),
            input,
        )
        .unwrap();
    let text = lab.texts(&out[0].path, None).join("\n");
    assert!(
        text.contains("Zażółć: TITLE") && text.contains("SOME BODY TEXT HERE."),
        "{text}"
    );
}

#[test]
fn redact_removes_what_the_model_found_and_only_what_is_really_there() {
    let model = FakeModel::start();
    let lab = Lab::new();
    let input = lab.doc(
        "form.pdf",
        &pdfs::pdf_with_positioned_text(&[
            (72.0, 700.0, "Applicant: Maria Gonzales"),
            (72.0, 660.0, "Reference: 4711"),
        ]),
    );
    let out = lab
        .run(
            ToolOptions::Redact(RedactOptions {
                ai_suggestions: true,
                ai: model.ai(),
                ..RedactOptions::default()
            }),
            input,
        )
        .unwrap();
    let text = lab.texts(&out[0].path, None).join("\n");
    assert!(
        !text.contains("Maria"),
        "the name is removed from the page: {text}"
    );
    assert!(text.contains("Reference: 4711"), "the rest stays: {text}");
    lab.tidy();
}

#[test]
fn markdown_is_tidied_by_the_model_when_asked() {
    let model = FakeModel::start();
    let lab = Lab::new();
    let doc = || {
        InputDoc::new(lab.put(
            "m.pdf",
            &sized(&[vec![
                ("Heading of the page", 24.0),
                ("The first paragraph of the page runs on", 11.0),
                ("over two lines of text here.", 11.0),
            ]]),
        ))
    };
    let plain = lab
        .run(
            ToolOptions::PdfToMarkdown(PdfToMarkdownOptions::default()),
            doc(),
        )
        .unwrap();
    assert_eq!(model.requests(), 0, "off by default");
    let tidied = lab
        .run(
            ToolOptions::PdfToMarkdown(PdfToMarkdownOptions {
                ai_cleanup: true,
                ai: model.ai(),
                images: false,
                ..PdfToMarkdownOptions::default()
            }),
            doc(),
        )
        .unwrap();
    assert!(model.requests() >= 1);
    let a = fs::read_to_string(&plain[0].path).unwrap();
    let b = fs::read_to_string(&tidied[0].path).unwrap();
    assert!(a.contains("Heading of the page") && b.contains("Heading of the page"));
    assert!(b.contains("runs on over two lines of text here."), "{b}");
}

#[test]
fn ollama_needs_agreement_and_a_missing_model_is_explained() {
    let lab = Lab::new();
    let input = lab.doc("x.pdf", &pdfs::pdf_with_texts(&["text"]));
    let mut ai = AiOptions {
        backend: AiBackend::Ollama,
        ..AiOptions::default()
    };
    let err = lab
        .run(
            ToolOptions::AiSummarizer(AiSummarizerOptions {
                ai: ai.clone(),
                ..AiSummarizerOptions::default()
            }),
            input.clone(),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NetworkConsentRequired);
    ai.backend = AiBackend::Bundled;
    let err = lab
        .run(
            ToolOptions::AiSummarizer(AiSummarizerOptions {
                ai,
                ..AiSummarizerOptions::default()
            }),
            input,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ComponentMissing);
    assert!(err.message.contains("Components"), "{}", err.message);
}
