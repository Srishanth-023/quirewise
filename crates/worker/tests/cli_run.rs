//! End-to-end: `qw run` drives the real worker, so a tool is usable without the UI.

use std::path::PathBuf;
use test_support::minimal_pdf;

fn worker() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_quirewise-worker"))
}

fn qw(args: &[&str]) -> (String, i32) {
    quirewise_cli::run_with(std::iter::once("qw").chain(args.iter().copied()), worker())
}

#[test]
fn run_splits_a_file_and_prints_each_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("book.pdf");
    std::fs::write(&input, minimal_pdf(4)).unwrap();
    // The output folder does not exist yet; the run creates it.
    let out = dir.path().join("out");

    let (text, code) = qw(&[
        "run",
        "split-every-n",
        input.to_str().unwrap(),
        "--options",
        r#"{"n":2}"#,
        "--out",
        out.to_str().unwrap(),
    ]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(text.lines().count(), 2, "{text}");
    assert!(text.contains("(2 pages,"), "{text}");
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 2);
}

#[test]
fn run_fails_with_exit_code_1_and_names_the_file_that_failed() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("broken.pdf");
    std::fs::write(&input, b"this is not a pdf").unwrap();

    let (text, code) = qw(&["run", "rotate", input.to_str().unwrap()]);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("broken.pdf"), "{text}");
}

#[test]
fn run_rejects_unknown_options_before_starting_a_worker() {
    let (text, code) = qw(&["run", "rotate", "a.pdf", "--options", r#"{"angel":"90"}"#]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("QW-OPTION"), "{text}");
}
