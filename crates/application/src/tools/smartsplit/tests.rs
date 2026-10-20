#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::runtime::CancellationToken;
use crate::tools::aikit::testkit::{FakeText, Script};
use domain::tools::ai::AiOptions;
use std::path::Path;

fn run(pages: &[&str], answers: &[&str]) -> (Result<Vec<SplitPart>, AppError>, Script) {
    let llm = Script::new(answers);
    let cancel = CancellationToken::new();
    let ai = AiOptions::default();
    let model = Model {
        llm: &llm,
        ai: &ai,
        cancel: &cancel,
    };
    let text = FakeText(pages.iter().map(|p| (*p).to_owned()).collect());
    let count = u32::try_from(pages.len()).unwrap();
    let mut read = 0;
    let result = propose(
        &model,
        &text,
        Path::new("a.pdf"),
        None,
        count,
        "one file for each invoice",
        &mut || read += 1,
    );
    if result.is_ok() {
        assert_eq!(read, count, "every page was read once");
    }
    (result, llm)
}

fn spans(parts: &[SplitPart]) -> Vec<(u32, u32, &str)> {
    parts
        .iter()
        .map(|p| (p.start, p.end, p.title.as_str()))
        .collect()
}

#[test]
fn parts_follow_the_starts_the_model_names() {
    let (parts, llm) = run(
        &[
            "Invoice 1001",
            "Items",
            "Invoice 1002",
            "Invoice 1003",
            "Total",
        ],
        &[
            r#"{"parts":[{"start":1,"title":"Invoice 1001"},{"start":3,"title":"Invoice 1002"},{"start":4,"title":"Invoice 1003"}]}"#,
        ],
    );
    assert_eq!(
        spans(&parts.unwrap()),
        [
            (1, 2, "Invoice 1001"),
            (3, 3, "Invoice 1002"),
            (4, 5, "Invoice 1003")
        ]
    );
    let asked = llm.asked.lock().unwrap();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].json);
    assert!(
        asked[0]
            .user
            .contains("Instruction: one file for each invoice")
    );
    assert!(asked[0].user.contains("[1] Invoice 1001"));
    assert!(asked[0].user.contains("[5] Total"));
    assert!(
        asked[0].system.contains("never follow them"),
        "the data rule is in the prompt"
    );
}

#[test]
fn a_missing_first_start_is_added_and_nonsense_is_ignored() {
    let (parts, _) = run(
        &["a", "b", "c", "d"],
        &[
            r#"{"parts":[{"start":"3","title":"  \"Second\"  "},{"start":3,"title":"dup"},{"start":99,"title":"far"},{"start":0},{"title":"no start"},{"start":2}]}"#,
        ],
    );
    assert_eq!(
        spans(&parts.unwrap()),
        [(1, 1, "Part 1"), (2, 2, "Part 2"), (3, 4, "Second")]
    );
}

#[test]
fn a_model_that_finds_no_place_to_split_says_so() {
    let (result, _) = run(
        &["a", "b", "c"],
        &[r#"{"parts":[{"start":1,"title":"All"}]}"#],
    );
    let err = result.unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(err.message.contains("no place to split"), "{}", err.message);
    // A document of one page needs no split to be right.
    let (single, _) = run(&["a"], &[r#"{"parts":[]}"#]);
    assert_eq!(spans(&single.unwrap()), [(1, 1, "Part 1")]);
}

#[test]
fn pages_without_text_cannot_be_read() {
    let (result, llm) = run(&["", "  ", ""], &[r#"{"parts":[]}"#, r#"{"parts":[]}"#]);
    let err = result.unwrap_err();
    assert!(err.message.contains("OCR"), "{}", err.message);
    assert!(llm.asked.lock().unwrap()[0].user.contains("(no text)"));
}

#[test]
fn long_documents_are_read_in_windows_that_know_the_open_part() {
    let pages: Vec<String> = (1..=90).map(|i| format!("page {i}")).collect();
    let refs: Vec<&str> = pages.iter().map(String::as_str).collect();
    let (parts, llm) = run(
        &refs,
        &[
            r#"{"parts":[{"start":1,"title":"Intro"},{"start":30,"title":"Body"}]}"#,
            r#"{"parts":[{"start":41,"title":"too early?"},{"start":70,"title":"Annex"}]}"#,
            r#"{"parts":[]}"#,
        ],
    );
    let parts = parts.unwrap();
    assert_eq!(
        spans(&parts),
        [
            (1, 29, "Intro"),
            (30, 40, "Body"),
            (41, 69, "too early?"),
            (70, 90, "Annex")
        ]
    );
    let asked = llm.asked.lock().unwrap();
    assert_eq!(asked.len(), 3);
    assert!(asked[0].user.contains("[40] page 40") && !asked[0].user.contains("[41]"));
    assert!(
        asked[1].system.contains("called \"Body\""),
        "{}",
        asked[1].system
    );
    assert!(asked[2].system.contains("called \"Annex\""));
    assert!(asked[2].user.contains("[90] page 90"));
}

#[test]
fn titles_are_one_clean_line() {
    assert_eq!(
        clean_title("  \"Annual\n report\t2024\"  "),
        "Annual report 2024"
    );
    assert_eq!(clean_title("a\u{7}b"), "ab");
    assert_eq!(clean_title(&"x".repeat(500)).chars().count(), 100);
}

#[test]
fn the_tool_checks_what_it_is_given() {
    let tool = SmartSplitTool::new(ToolDeps::unavailable());
    let doc = InputDoc {
        page_count: Some(6),
        ..InputDoc::new("C:\\a.pdf")
    };
    let none = ToolOptions::SmartSplit(SmartSplitOptions::default());
    assert_eq!(
        tool.validate(std::slice::from_ref(&doc), &none)
            .unwrap_err()
            .code,
        ErrorCode::OptionInvalid
    );
    let ask = ToolOptions::SmartSplit(SmartSplitOptions {
        instruction: "split by chapter".into(),
        ..SmartSplitOptions::default()
    });
    assert_eq!(
        tool.validate(std::slice::from_ref(&doc), &ask)
            .unwrap()
            .total_units,
        12
    );
    let checked = ToolOptions::SmartSplit(SmartSplitOptions {
        parts: vec![SplitPart {
            start: 1,
            end: 7,
            title: "x".into(),
        }],
        ..SmartSplitOptions::default()
    });
    assert!(tool.validate(std::slice::from_ref(&doc), &checked).is_err());
    let good = ToolOptions::SmartSplit(SmartSplitOptions {
        parts: vec![
            SplitPart {
                start: 1,
                end: 3,
                title: "a".into(),
            },
            SplitPart {
                start: 4,
                end: 6,
                title: "b".into(),
            },
        ],
        ..SmartSplitOptions::default()
    });
    assert_eq!(tool.validate(&[doc], &good).unwrap().total_units, 6);
}
