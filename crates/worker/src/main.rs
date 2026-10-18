//! `quirewise-worker.exe`: parses untrusted PDFs inside a Job Object and serves the app over stdio.
#![forbid(unsafe_code)]

use domain::limits::SafetyLimits;
use infrastructure::{AtomicFileStore, NativePaths, SystemClock, TracingLogger};
use quirewise_worker::engine::{Engines, build_services};
use quirewise_worker::job_object::{JobLimits, confine_current_process, is_confined};
use std::process::ExitCode;
use std::sync::Arc;

/// Reads `--name value` as a number, falling back to `default` when absent or malformed.
fn arg_number(name: &str, default: u64) -> u64 {
    let mut args = std::env::args();
    while let Some(arg) = args.next() {
        if arg == name {
            return args.next().and_then(|v| v.parse().ok()).unwrap_or(default);
        }
    }
    default
}

fn main() -> ExitCode {
    let defaults = SafetyLimits::default();
    let limits = JobLimits {
        memory_bytes: arg_number("--max-memory-bytes", defaults.max_memory.get()),
        cpu_seconds: arg_number("--max-cpu-seconds", u64::from(defaults.max_cpu_seconds)),
    };
    // Refuse to parse untrusted input unconfined.
    let _job = match confine_current_process(limits) {
        Ok(guard) => guard,
        Err(_) => {
            eprintln!("quirewise-worker: could not apply safety limits");
            return ExitCode::from(2);
        }
    };

    // Self-test used by the test suite: report whether confinement is really in effect.
    if std::env::args().any(|a| a == "--check-confinement") {
        println!("confined={}", is_confined());
        return ExitCode::SUCCESS;
    }

    let engines = match NativePaths::from_environment().and_then(|paths| Engines::load(&paths)) {
        Ok(engines) => engines,
        Err(error) => {
            eprintln!("quirewise-worker: {error}");
            return ExitCode::from(3);
        }
    };

    let services = match build_services(
        &engines,
        Arc::new(AtomicFileStore),
        Arc::new(SystemClock),
        Arc::new(TracingLogger),
        defaults,
        std::env::temp_dir().join("Quirewise"),
    ) {
        Ok(services) => services,
        Err(error) => {
            eprintln!("quirewise-worker: {error}");
            return ExitCode::from(1);
        }
    };

    match quirewise_worker::serve(std::io::stdin().lock(), std::io::stdout(), services) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(1),
    }
}
