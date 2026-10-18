//! Runs a sidecar process with a time limit, feeding standard input and capturing output.

use application::AppError;
use domain::ErrorCode;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Captured result of a finished process.
#[derive(Debug)]
pub struct Output {
    /// Exit code, `None` when the process was killed by a signal.
    pub code: Option<i32>,
    /// Standard error as lossy UTF-8. Matched against, never shown to users.
    pub stderr: String,
}

fn spawn_failed() -> AppError {
    AppError::new(
        ErrorCode::ComponentMissing,
        "The PDF tool qpdf could not be started. Reinstall Quirewise or check that antivirus did not remove it.",
    )
}

/// Runs `exe args`, writes `stdin` to it, and waits at most `timeout`.
///
/// On timeout the process is killed and `QW-ENGINE-TIMEOUT` is returned.
pub fn run(
    exe: &Path,
    args: &[OsString],
    stdin: Option<&[u8]>,
    timeout: Duration,
) -> Result<Output, AppError> {
    let mut command = Command::new(exe);
    command
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    if let Some(dir) = exe.parent().filter(|d| !d.as_os_str().is_empty()) {
        // qpdf finds its runtime DLLs next to the executable.
        command.current_dir(dir);
    }
    let mut child = command.spawn().map_err(|_| spawn_failed())?;

    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // A process that exits early closes the pipe; that failure surfaces through its exit code.
        let _ = pipe.write_all(bytes);
    }
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    // Drain both pipes on threads so a chatty process cannot block on a full pipe.
    let drain_out = thread::spawn(move || {
        let mut sink = Vec::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_end(&mut sink);
        }
    });
    let drain_err = thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_end(&mut text);
        }
        String::from_utf8_lossy(&text).into_owned()
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = drain_out.join();
                let _ = drain_err.join();
                return Err(AppError::new(
                    ErrorCode::EngineTimeout,
                    "The PDF tool took longer than the time limit and was stopped. Try a smaller file.",
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => return Err(AppError::internal("running the PDF tool")),
        }
    };
    let _ = drain_out.join();
    let stderr = drain_err.join().unwrap_or_default();
    Ok(Output {
        code: status.code(),
        stderr,
    })
}
