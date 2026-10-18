//! End-to-end: the persistent worker session answering real queries through the real worker.

use application::query::{DocumentQuery, QueryResult, QueryRunner};
use application::{CancellationToken, Password};
use domain::ErrorCode;
use domain::limits::SafetyLimits;
use infrastructure::WorkerSession;
use std::path::{Path, PathBuf};
use test_support::{encrypt_pdf, minimal_pdf};

fn session() -> WorkerSession {
    WorkerSession::new(
        PathBuf::from(env!("CARGO_BIN_EXE_quirewise-worker")),
        SafetyLimits::default(),
    )
}

fn thumb(path: &Path, page: u32) -> DocumentQuery {
    DocumentQuery::Thumbnail {
        path: path.to_path_buf(),
        password: None,
        page,
        max_edge: 120,
    }
}

#[test]
fn one_worker_serves_many_concurrent_thumbnail_queries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.pdf");
    std::fs::write(&path, minimal_pdf(6)).unwrap();
    let session = session();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..6)
            .map(|page| {
                let (session, path) = (&session, &path);
                scope.spawn(move || session.query(&thumb(path, page), &CancellationToken::new()))
            })
            .collect();
        for handle in handles {
            let QueryResult::Thumbnail(t) = handle.join().unwrap().unwrap() else {
                panic!("expected a thumbnail");
            };
            assert_eq!((t.page_count, t.height), (6, 120));
            assert_eq!(&t.png.0[1..4], b"PNG");
        }
    });
}

#[test]
fn errors_come_back_with_catalog_codes_and_the_session_keeps_working() {
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("a.pdf");
    std::fs::write(&plain, minimal_pdf(2)).unwrap();
    let locked = dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "open");
    let session = session();
    let token = CancellationToken::new();

    let err = session.query(&thumb(&locked, 0), &token).unwrap_err();
    assert_eq!(err.code, ErrorCode::PdfEncrypted);
    let out_of_range = session.query(&thumb(&plain, 9), &token).unwrap_err();
    assert_eq!(out_of_range.code, ErrorCode::RangeOutOfBounds);

    let unlocked = DocumentQuery::Inspect {
        path: locked,
        password: Some(Password::new("open")),
    };
    let QueryResult::Inspect(report) = session.query(&unlocked, &token).unwrap() else {
        panic!("expected a report");
    };
    assert_eq!(report.page_count, 2);
}

#[test]
fn a_missing_worker_is_reported_as_a_missing_component() {
    let session = WorkerSession::new(
        PathBuf::from(r"C:\definitely\missing\quirewise-worker.exe"),
        SafetyLimits::default(),
    );
    let err = session
        .query(
            &thumb(&PathBuf::from("a.pdf"), 0),
            &CancellationToken::new(),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ComponentMissing);
}
