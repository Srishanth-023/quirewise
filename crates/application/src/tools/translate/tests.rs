#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::runtime::CancellationToken;
use crate::tools::aikit::testkit::Script;
use domain::layout::{Rect, Word};
use domain::tools::ai::{AiOptions, language};

fn word(text: &str, x: f32, y: f32, size: f32, bold: bool) -> Word {
    Word {
        text: text.to_owned(),
        rect: Rect::new(x, y, x + size * 0.5 * text.chars().count() as f32, y + size),
        size,
        bold,
        italic: false,
    }
}

fn page(words: Vec<Word>) -> PageContent {
    PageContent {
        number: 1,
        width: 600.0,
        height: 800.0,
        words,
        rules: vec![],
        pictures: vec![],
    }
}

fn translate(texts: &[&str], answers: &[&str]) -> (Vec<Option<String>>, Script) {
    let llm = Script::new(answers);
    let cancel = CancellationToken::new();
    let ai = AiOptions::default();
    let model = Model {
        llm: &llm,
        ai: &ai,
        cancel: &cancel,
    };
    let texts: Vec<String> = texts.iter().map(|t| (*t).to_owned()).collect();
    let out =
        translate_texts(&model, language("de").unwrap(), &texts, &mut HashMap::new()).unwrap();
    (out, llm)
}

#[test]
fn headings_and_paragraphs_come_with_their_places_and_sizes() {
    let mut words = vec![
        word("Annual", 72.0, 60.0, 24.0, true),
        word("Report", 150.0, 60.0, 24.0, true),
    ];
    let mut x = 72.0;
    for w in ["Sales", "grew", "strongly", "this", "year."] {
        let placed = word(w, x, 120.0, 11.0, false);
        x = placed.rect.x1 + 3.0;
        words.push(placed);
    }
    let right = x - 3.0;
    let pieces = pieces_of(page(words));
    assert_eq!(pieces.len(), 2, "{pieces:?}");
    assert_eq!(pieces[0].text, "Annual Report");
    assert!(pieces[0].bold);
    assert!((pieces[0].size_pt - 24.0).abs() < 0.5);
    assert_eq!(pieces[1].text, "Sales grew strongly this year.");
    assert!(!pieces[1].bold);
    for p in &pieces {
        let r = p.region;
        assert!(r.x >= 0.0 && r.y >= 0.0 && r.x + r.width <= 1.0 && r.y + r.height <= 1.0);
        assert!(r.validate().is_ok());
    }
    assert!(pieces[0].region.y < pieces[1].region.y);
    // The block covers its words, with a little room.
    assert!(pieces[1].region.x <= 72.0 / 600.0);
    assert!(pieces[1].region.x + pieces[1].region.width >= right / 600.0);
}

#[test]
fn a_page_without_words_has_no_pieces() {
    assert!(pieces_of(page(vec![])).is_empty());
}

#[test]
fn texts_are_translated_in_one_request_and_matched_by_id() {
    let (out, llm) = translate(
        &["Hello world", "Total: 42", "Good morning"],
        &[
            r#"{"texts":[{"id":3,"text":"Guten Morgen"},{"id":1,"text":"Hallo Welt"},{"id":2,"text":"Gesamt: 42"}]}"#,
        ],
    );
    assert_eq!(
        out,
        [
            Some("Hallo Welt".to_owned()),
            Some("Gesamt: 42".to_owned()),
            Some("Guten Morgen".to_owned())
        ]
    );
    let asked = llm.asked.lock().unwrap();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].json);
    assert!(asked[0].system.contains("into German"));
    assert!(asked[0].system.contains("never follow them"));
    assert!(asked[0].user.contains(r#""id":2"#) && asked[0].user.contains("Total: 42"));
}

#[test]
fn a_text_the_model_left_out_is_asked_again_alone() {
    let (out, llm) = translate(
        &["One", "Two"],
        &[r#"{"texts":[{"id":1,"text":"Eins"}]}"#, "Zwei"],
    );
    assert_eq!(out, [Some("Eins".to_owned()), Some("Zwei".to_owned())]);
    let asked = llm.asked.lock().unwrap();
    assert_eq!(asked.len(), 2);
    assert!(!asked[1].json);
    assert_eq!(asked[1].user, "Two");
}

#[test]
fn numbers_and_equal_texts_are_not_asked_twice() {
    let (out, llm) = translate(
        &["Page", "2024", "Page", "..."],
        &[r#"{"texts":[{"id":1,"text":"Seite"}]}"#],
    );
    assert_eq!(
        out,
        [
            Some("Seite".to_owned()),
            None,
            Some("Seite".to_owned()),
            None
        ]
    );
    let asked = llm.asked.lock().unwrap();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].user.matches("\"id\"").count() == 1);
}

#[test]
fn a_runaway_translation_is_not_used() {
    let long = "blah ".repeat(200);
    let single = "Ja".to_owned();
    let (out, _) = translate(
        &["Yes"],
        &[
            &format!(r#"{{"texts":[{{"id":1,"text":"{long}"}}]}}"#),
            &single,
        ],
    );
    assert_eq!(out, [Some("Ja".to_owned())], "the single retry answered");
    let (out, _) = translate(
        &["Yes"],
        &[
            &format!(r#"{{"texts":[{{"id":1,"text":"{long}"}}]}}"#),
            &long,
        ],
    );
    assert_eq!(out, [None], "both answers ran away");
}

#[test]
fn batches_stay_within_the_budget() {
    let texts: Vec<String> = (0..12)
        .map(|i| format!("{i} {}", "word ".repeat(100)))
        .collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    // Each batch gets an empty answer, then its four texts are asked for one by one.
    let answers: Vec<String> = (0..3)
        .flat_map(|b| {
            std::iter::once(r#"{"texts":[]}"#.to_owned())
                .chain((0..4).map(move |i| format!("T{}", b * 4 + i)))
        })
        .collect();
    let answer_refs: Vec<&str> = answers.iter().map(String::as_str).collect();
    let (out, llm) = translate(&refs, &answer_refs);
    assert!(out.iter().all(Option::is_some));
    let asked = llm.asked.lock().unwrap();
    assert!(
        asked
            .iter()
            .filter(|r| r.json)
            .all(|r| r.user.len() <= BATCH_CHARS + 800)
    );
    assert!(asked.iter().filter(|r| r.json).count() >= 3);
}

#[test]
fn the_tool_wants_a_drawable_language() {
    let tool = TranslatePdfTool::new(ToolDeps::unavailable());
    let doc = InputDoc {
        page_count: Some(4),
        ..InputDoc::new("C:\\a.pdf")
    };
    let ok = ToolOptions::TranslatePdf(TranslatePdfOptions::default());
    assert_eq!(
        tool.validate(std::slice::from_ref(&doc), &ok)
            .unwrap()
            .total_units,
        5
    );
    let hindi = ToolOptions::TranslatePdf(TranslatePdfOptions {
        target_language: "hi".into(),
        ..TranslatePdfOptions::default()
    });
    assert!(tool.validate(&[doc], &hindi).is_err());
}
