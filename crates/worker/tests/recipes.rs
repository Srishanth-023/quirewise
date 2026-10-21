//! End-to-end test for recipes: a chain of real tools run through the real `RunTool` and the
//! worker engines on a generated PDF.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::fs;
use std::path::Path;

use application::recipe::{RecipeUnit, run_recipe_unit};
use application::{CancellationToken, NoProgress};
use common::{Lab, name};
use domain::naming::OutputNamingPattern;
use domain::recipe::{Recipe, RecipeStep};
use domain::{ErrorCode, ToolId, ToolOptions};
use infrastructure::AtomicFileStore;
use test_support::pdfs;

fn recipe(tools: &[ToolId]) -> Recipe {
    Recipe {
        id: String::new(),
        name: "Tidy".into(),
        steps: tools
            .iter()
            .map(|t| RecipeStep {
                options: ToolOptions::defaults_for(*t),
            })
            .collect(),
    }
}

fn files_below(dir: &Path) -> usize {
    fs::read_dir(dir).map_or(0, |entries| {
        entries
            .flatten()
            .map(|e| {
                if e.path().is_dir() {
                    files_below(&e.path())
                } else {
                    1
                }
            })
            .sum()
    })
}

#[test]
fn a_three_step_recipe_gives_one_clean_output_and_leaves_no_scratch_files() {
    let lab = Lab::new();
    let bytes = pdfs::pdf_with_texts(&["alpha", "beta", "gamma"]);
    let input = lab.doc("report.pdf", &bytes);
    let source = input.path.clone();
    let scratch = lab.dir.path().join("recipe-tmp");
    let r = recipe(&[ToolId::Rotate, ToolId::PageNumbers, ToolId::Compress]);

    let outputs = run_recipe_unit(
        lab.runner.as_ref(),
        &AtomicFileStore,
        &RecipeUnit {
            recipe: &r,
            input,
            output_dir: Some(lab.out()),
            naming_pattern: OutputNamingPattern::default(),
            temp_dir: &scratch,
        },
        &CancellationToken::new(),
        &NoProgress,
    )
    .unwrap();

    assert_eq!(outputs.len(), 1);
    assert_eq!(name(&outputs[0]), "report (compressed).pdf");
    assert_eq!(
        fs::read_dir(lab.out()).unwrap().count(),
        1,
        "only the result"
    );
    assert_eq!(files_below(&scratch), 0, "intermediate files are removed");
    assert_eq!(fs::read(&source).unwrap(), bytes, "the input is untouched");

    // The first step turned the pages, the second numbered them, the third kept both.
    let (w, h) = lab.sizes(&outputs[0].path)[0];
    assert!(w > h, "rotated to landscape: {w} x {h}");
    let texts = lab.texts(&outputs[0].path, None);
    assert_eq!(texts.len(), 3);
    assert!(
        texts[0].contains("alpha") && texts[0].contains('1'),
        "{texts:?}"
    );
    assert!(
        texts[2].contains("gamma") && texts[2].contains('3'),
        "{texts:?}"
    );
    lab.tidy();
}

#[test]
fn a_failing_step_names_itself_and_writes_nothing() {
    let lab = Lab::new();
    let input = lab.doc("broken.pdf", b"%PDF-1.4 not really a pdf");
    let scratch = lab.dir.path().join("recipe-tmp");
    let r = recipe(&[ToolId::Rotate, ToolId::Compress]);

    let err = run_recipe_unit(
        lab.runner.as_ref(),
        &AtomicFileStore,
        &RecipeUnit {
            recipe: &r,
            input,
            output_dir: Some(lab.out()),
            naming_pattern: OutputNamingPattern::default(),
            temp_dir: &scratch,
        },
        &CancellationToken::new(),
        &NoProgress,
    )
    .unwrap_err();

    assert_eq!(err.code, ErrorCode::PdfDamaged);
    assert!(err.message.contains("Step 1"), "{}", err.message);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    assert_eq!(files_below(&scratch), 0);
}
