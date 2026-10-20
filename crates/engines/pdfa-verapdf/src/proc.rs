//! Running veraPDF with a time limit and a way to stop it.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use application::AppError;
use application::CancellationToken;
use domain::ErrorCode;

/// Most bytes of the report that are kept.
const MAX_REPORT: u64 = 64 * 1024 * 1024;
/// Most bytes of the error text that are kept.
const MAX_MESSAGE: u64 = 8 * 1024;

/// What a run wrote.
#[derive(Debug)]
pub struct Output {
    /// What the program wrote to its output stream.
    pub stdout: Vec<u8>,
    /// What it wrote to its error stream.
    pub stderr: String,
}

fn quiet(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

fn kill_tree(child: &mut Child) {
    let mut taskkill = Command::new("taskkill.exe");
    taskkill.args(["/PID", &child.id().to_string(), "/T", "/F"]);
    quiet(&mut taskkill);
    let _ = taskkill
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs `command` until it ends, `timeout` passes, or `cancel` is set. The exit status is not
/// returned: veraPDF exits with failure when the file does not conform, and the report says more.
pub fn run(
    mut command: Command,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<Output, AppError> {
    quiet(&mut command);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::from_io("start the PDF/A checker", &e))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(stream) = stdout {
            let _ = stream.take(MAX_REPORT).read_to_end(&mut bytes);
        }
        bytes
    });
    let err_reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(stream) = stderr {
            let _ = stream.take(MAX_MESSAGE).read_to_string(&mut text);
        }
        text
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(e) => return Err(AppError::from_io("wait for the PDF/A checker", &e)),
        }
        if cancel.check().is_err() {
            kill_tree(&mut child);
            let _ = out_reader.join();
            let _ = err_reader.join();
            return Err(AppError::cancelled());
        }
        if started.elapsed() > timeout {
            kill_tree(&mut child);
            let _ = out_reader.join();
            let _ = err_reader.join();
            return Err(AppError::new(
                ErrorCode::EngineTimeout,
                "Checking the file took too long. Try a smaller file.",
            ));
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    Ok(Output {
        stdout: out_reader.join().unwrap_or_default(),
        stderr: err_reader.join().unwrap_or_default(),
    })
}
