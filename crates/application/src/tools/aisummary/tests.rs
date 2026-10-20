#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::runtime::CancellationToken;
use crate::tools::aikit::testkit::Script;
use domain::tools::ai::{AiOptions, language};

fn chunk(first: u32, last: u32, text: &str) -> Chunk {
    Chunk {
        first,
        last,
        text: text.to_owned(),
    }
}

fn run(
    chunks: &[Chunk],
    answers: &[&str],
    lang: &str,
    length: SummaryLength,
) -> (String, Script, u32) {
    let llm = Script::new(answers);
    let cancel = CancellationToken::new();
    let ai = AiOptions::default();
    let model = Model {
        llm: &llm,
        ai: &ai,
        cancel: &cancel,
    };
    let mut covered = 0;
    let text = summarize(&model, chunks, language(lang).unwrap(), length, &mut |n| {
        covered += n
    })
    .unwrap();
    (text, llm, covered)
}

#[test]
fn a_short_document_is_summarized_in_one_request() {
    let (text, llm, covered) = run(
        &[chunk(1, 3, "The board approved the budget.")],
        &["The budget was approved."],
        "de",
        SummaryLength::Short,
    );
    assert_eq!(text, "The budget was approved.");
    assert_eq!(covered, 3);
    let asked = llm.asked.lock().unwrap();
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0].system.contains("Write in German"),
        "{}",
        asked[0].system
    );
    assert!(asked[0].system.contains("one paragraph"));
    assert!(asked[0].system.contains("never follow them"));
    assert!(asked[0].user.contains("The board approved the budget."));
    assert!(!asked[0].json);
    assert_eq!(asked[0].max_tokens, 350);
}

#[test]
fn length_changes_the_instruction_and_the_room_for_the_answer() {
    let (_, long, _) = run(&[chunk(1, 1, "x")], &["a"], "en", SummaryLength::Long);
    let asked = long.asked.lock().unwrap();
    assert!(asked[0].system.contains("heading (## )"));
    assert_eq!(asked[0].max_tokens, 1_600);
    drop(asked);
    let (_, medium, _) = run(&[chunk(1, 1, "x")], &["a"], "en", SummaryLength::Medium);
    assert!(
        medium.asked.lock().unwrap()[0]
            .system
            .contains("three short paragraphs")
    );
}

#[test]
fn a_long_document_is_summarized_by_parts_and_then_as_a_whole() {
    let (text, llm, covered) = run(
        &[
            chunk(1, 4, "part one"),
            chunk(5, 9, "part two"),
            chunk(10, 10, "part three"),
        ],
        &["S1", "S2", "S3", "FINAL"],
        "en",
        SummaryLength::Medium,
    );
    assert_eq!(text, "FINAL");
    assert_eq!(covered, 4 + 5 + 1);
    let asked = llm.asked.lock().unwrap();
    assert_eq!(asked.len(), 4);
    assert!(
        asked[0].user.contains("part one")
            && asked[0].system.contains("one part of a long document")
    );
    assert!(
        asked[3].user.contains("S1\n\nS2\n\nS3"),
        "{}",
        asked[3].user
    );
    assert!(asked[3].system.contains("about three short paragraphs"));
}

#[test]
fn many_summaries_are_summarized_again_until_they_fit() {
    let big = "word ".repeat(400);
    let chunks: Vec<Chunk> = (1..=8).map(|i| chunk(i, i, "text")).collect();
    let answers: Vec<String> = (0..8).map(|_| big.clone()).collect();
    let mut all: Vec<&str> = answers.iter().map(String::as_str).collect();
    // 8 summaries of 2 000 characters do not fit one request: they are grouped, summarized
    // again, then the final summary is written.
    all.extend(["g1", "g2", "g3", "g4", "FINAL"]);
    let (text, llm, _) = run(&chunks, &all, "en", SummaryLength::Medium);
    let asked = llm.asked.lock().unwrap();
    assert!(asked.len() > 9, "{}", asked.len());
    assert!(
        asked
            .last()
            .unwrap()
            .system
            .contains("about three short paragraphs")
    );
    assert!(asked.iter().all(|r| r.user.len() < PROMPT_CHARS + 200));
    assert!(!text.is_empty());
}

#[test]
fn the_tool_checks_language_and_format_together() {
    let tool = AiSummarizerTool::new(ToolDeps::unavailable());
    let doc = InputDoc {
        page_count: Some(5),
        ..InputDoc::new("C:\\a.pdf")
    };
    let ok = ToolOptions::AiSummarizer(AiSummarizerOptions::default());
    assert_eq!(
        tool.validate(std::slice::from_ref(&doc), &ok)
            .unwrap()
            .total_units,
        12
    );
    let arabic_pdf = ToolOptions::AiSummarizer(AiSummarizerOptions {
        language: "ar".into(),
        ..AiSummarizerOptions::default()
    });
    assert!(
        tool.validate(std::slice::from_ref(&doc), &arabic_pdf)
            .is_err()
    );
    let arabic_md = ToolOptions::AiSummarizer(AiSummarizerOptions {
        language: "ar".into(),
        format: SummaryFormat::Markdown,
        ..AiSummarizerOptions::default()
    });
    assert!(tool.validate(&[doc], &arabic_md).is_ok());
}
