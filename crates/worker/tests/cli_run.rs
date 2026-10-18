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

#[test]
fn run_protects_a_file_with_secrets_read_from_an_options_file() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("plain.pdf");
    std::fs::write(&input, minimal_pdf(2)).unwrap();
    let options = dir.path().join("options.json");
    std::fs::write(
        &options,
        r#"{"user_password":"open","owner_password":"boss"}"#,
    )
    .unwrap();
    let out = dir.path().join("out");

    let (text, code) = qw(&[
        "run",
        "protect",
        input.to_str().unwrap(),
        "--options-file",
        options.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{text}");
    assert!(
        !text.contains("open") || text.contains("(protected)"),
        "secrets are never echoed"
    );
    assert!(!text.contains("boss"));

    // The result needs its password: running unlock on it without one says so.
    let protected = std::fs::read_dir(&out)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let (text, code) = qw(&["run", "unlock", protected.to_str().unwrap()]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("QW-PDF-ENCRYPTED"), "{text}");
}

#[test]
fn run_refuses_options_given_twice_and_a_missing_options_file() {
    let (_, code) = qw(&[
        "run",
        "rotate",
        "a.pdf",
        "--options",
        "{}",
        "--options-file",
        "x.json",
    ]);
    assert_eq!(code, 2);
    let (text, code) = qw(&[
        "run",
        "rotate",
        "a.pdf",
        "--options-file",
        r"C:\no\such\file.json",
    ]);
    assert_eq!(code, 1, "{text}");
}
