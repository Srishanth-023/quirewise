//! Runs the AI tools against the real llama.cpp server and a small model from the development
//! tree (`scripts/fetch-llama.ps1 -Model smollm2-360m-instruct`). When they are not there the
//! tests say so and pass, like the tests of the other on-demand components.
//!
//! A model this small does not write good summaries; these tests check that the whole path works
//! (the server starts with the right arguments, the stream is read, the output is a valid file),
//! not what the model says.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::fs;
use std::path::PathBuf;

use application::CancellationToken;
use application::query::{DocumentQuery, QueryResult};
use common::{Lab, name};
use domain::ToolOptions;
use domain::tools::ai::{
    AiOptions, AiSummarizerOptions, MODELS, SmartSplitOptions, SummaryFormat, SummaryLength,
};
use test_support::pdfs;

const MODEL: &str = "smollm2-360m-instruct";

fn vendor() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor")
}

/// True when the server and the small model are in the development tree.
fn ready() -> bool {
    let server = fs::read_dir(vendor().join("llama-cpp").join("x64"))
        .map(|d| d.flatten().any(|e| e.file_name() == "llama-server.exe"))
        .unwrap_or(false);
    let model = MODELS
        .iter()
        .find(|m| m.id == MODEL)
        .is_some_and(|m| vendor().join("models").join(MODEL).join(m.file).is_file());
    if !(server && model) {
        eprintln!(
            "llama.cpp or the small model is not installed here: run scripts/fetch-llama.ps1 -Model {MODEL}"
        );
    }
    server && model
}

fn small() -> AiOptions {
    AiOptions {
        model: Some(MODEL.into()),
        ..AiOptions::default()
    }
}

const REPORT: &[&str] = &[
    "The council met on Tuesday to discuss the new library. The budget is two million euros.",
    "Residents asked for longer opening hours. The council agreed to open on Saturdays.",
];

#[test]
fn the_real_server_starts_streams_and_stops_with_the_lab() {
    if !ready() {
        return;
    }
    let lab = Lab::new();
    let listed = lab
        .queries
        .query(
            &DocumentQuery::AiModels { ai: small() },
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(listed, QueryResult::AiModels(vec![MODEL.to_owned()]));

    let input = lab.doc("council.pdf", &pdfs::pdf_with_texts(REPORT));
    let out = lab
        .run(
            ToolOptions::AiSummarizer(AiSummarizerOptions {
                ai: small(),
                length: SummaryLength::Short,
                format: SummaryFormat::Markdown,
                ..AiSummarizerOptions::default()
            }),
            input,
        )
        .unwrap();
    assert_eq!(name(&out[0]), "council (summary).md");
    let md = fs::read_to_string(&out[0].path).unwrap();
    let body = md.strip_prefix("# council\n\n").expect("title");
    assert!(
        body.trim().chars().count() > 20,
        "the model wrote something: {md}"
    );
    eprintln!("summary by the real model: {body}");
}

#[test]
fn the_real_model_summary_can_be_a_pdf() {
    if !ready() {
        return;
    }
    let lab = Lab::new();
    let input = lab.doc("council.pdf", &pdfs::pdf_with_texts(REPORT));
    let out = lab
        .run(
            ToolOptions::AiSummarizer(AiSummarizerOptions {
                ai: small(),
                length: SummaryLength::Short,
                ..AiSummarizerOptions::default()
            }),
            input,
        )
        .unwrap();
    let text = lab.texts(&out[0].path, None).join("\n");
    assert!(text.contains("council"), "the title is on the page: {text}");
    assert!(text.chars().count() > 40, "{text}");
}

#[test]
fn smart_split_with_the_real_model_ends_in_valid_files_or_a_clear_message() {
    if !ready() {
        return;
    }
    let lab = Lab::new();
    let input = lab.doc(
        "mixed.pdf",
        &pdfs::pdf_with_texts(&[
            "INVOICE 1001 Acme Ltd total 120 euros",
            "Invoice 1001 page two: items and VAT",
            "INVOICE 1002 Beta GmbH total 80 euros",
            "Invoice 1002 page two: items and VAT",
        ]),
    );
    match lab.run(
        ToolOptions::SmartSplit(SmartSplitOptions {
            ai: small(),
            instruction: "A new file for each invoice".into(),
            parts: Vec::new(),
        }),
        input,
    ) {
        Ok(out) => {
            let pages: u32 = out.iter().map(|o| o.pages.unwrap()).sum();
            assert_eq!(pages, 4, "every page lands in a part: {out:?}");
        }
        Err(e) => {
            // A model this small may not manage the JSON; the message must say what to do.
            assert!(!e.message.is_empty());
            eprintln!("the small model did not split: {} {}", e.code, e.message);
        }
    }
}
