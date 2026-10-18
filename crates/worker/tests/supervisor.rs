//! End-to-end: the real worker executable driven by the real supervisor over stdio.

use application::{CancellationToken, InputDoc, NoProgress, UnitRequest, UnitRunner};
use domain::limits::SafetyLimits;
use domain::naming::OutputNamingPattern;
use domain::{ErrorCode, ToolId, ToolOptions};
use infrastructure::WorkerSupervisor;
use std::path::PathBuf;

fn unit() -> UnitRequest {
    UnitRequest {
        tool: ToolId::Rotate,
        options: ToolOptions::defaults_for(ToolId::Rotate),
        inputs: vec![InputDoc::new("C:\\does\\not\\matter.pdf")],
        output_dir: None,
        naming_pattern: OutputNamingPattern::default(),
    }
}

#[test]
fn worker_round_trip_reports_unregistered_tools_as_a_failed_unit() {
    let supervisor = WorkerSupervisor::new(
        PathBuf::from(env!("CARGO_BIN_EXE_quirewise-worker")),
        SafetyLimits::default(),
    );
    let err = supervisor
        .run_unit(&unit(), &CancellationToken::new(), &NoProgress)
        .unwrap_err();
    // Phase 1 registers no tools, so the worker answers with a catalog error, not a crash.
    assert_eq!(err.code, ErrorCode::Internal);
}

#[test]
fn missing_worker_executable_is_reported_as_a_missing_component() {
    let supervisor = WorkerSupervisor::new(
        PathBuf::from("C:\\definitely\\missing\\quirewise-worker.exe"),
        SafetyLimits::default(),
    );
    let err = supervisor
        .run_unit(&unit(), &CancellationToken::new(), &NoProgress)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ComponentMissing);
}

#[test]
fn already_cancelled_jobs_never_start_a_worker() {
    let supervisor = WorkerSupervisor::new(
        PathBuf::from("C:\\definitely\\missing\\quirewise-worker.exe"),
        SafetyLimits::default(),
    );
    let token = CancellationToken::new();
    token.cancel();
    let err = supervisor
        .run_unit(&unit(), &token, &NoProgress)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::JobCancelled);
}

#[test]
fn worker_confines_itself_in_a_job_object_before_doing_anything_else() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_quirewise-worker"))
        .arg("--check-confinement")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "confined=true");
}
