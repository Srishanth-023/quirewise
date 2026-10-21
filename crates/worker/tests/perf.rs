//! Speed budgets of docs/performance-budgets.md that a test can measure: the first thumbnail and
//! the merge of 100 small PDFs. The budgets are for release builds; a debug build is far slower, so
//! it only checks the loose limit that catches an accidental hang or quadratic loop.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::{Duration, Instant};

use application::query::{DocumentQuery, QueryResult};
use application::{CancellationToken, InputDoc};
use common::Lab;
use domain::ToolOptions;
use domain::tools::MergeOptions;
use test_support::pdfs;

/// The budget in release builds, ten times that in debug builds.
fn limit(release: Duration) -> Duration {
    if cfg!(debug_assertions) {
        release * 10
    } else {
        release
    }
}

#[test]
fn merging_100_small_pdfs_stays_inside_the_budget() {
    let lab = Lab::new();
    let inputs: Vec<InputDoc> = (0..100)
        .map(|i| {
            let page = format!("file {i}");
            lab.doc(
                &format!("f{i:03}.pdf"),
                &pdfs::pdf_with_texts(&[page.as_str()]),
            )
        })
        .collect();

    let started = Instant::now();
    let out = lab
        .run_all(ToolOptions::Merge(MergeOptions::default()), inputs)
        .unwrap();
    let took = started.elapsed();

    assert_eq!(out[0].pages, Some(100));
    eprintln!("merge of 100 small PDFs: {took:?}");
    assert!(
        took < limit(Duration::from_secs(3)),
        "merge of 100 small PDFs took {took:?}"
    );
}

#[test]
fn the_first_thumbnail_appears_inside_the_budget() {
    let lab = Lab::new();
    let doc = lab.put("one.pdf", &pdfs::pdf_with_texts(&["Hello", "World"]));

    let started = Instant::now();
    let result = lab
        .queries
        .query(
            &DocumentQuery::Thumbnail {
                path: doc,
                password: None,
                page: 0,
                max_edge: 256,
            },
            &CancellationToken::new(),
        )
        .unwrap();
    let took = started.elapsed();

    assert!(matches!(result, QueryResult::Thumbnail(_)));
    eprintln!("first thumbnail: {took:?}");
    assert!(
        took < limit(Duration::from_millis(300)),
        "the first thumbnail took {took:?}"
    );
}
