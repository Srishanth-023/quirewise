//! Running Tesseract with a time limit and a way to stop it.

use application::AppError;
use application::CancellationToken;
use domain::ErrorCode;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Most bytes of the error text that are kept.
const MAX_MESSAGE: u64 = 4096;

/// How a run ended.
#[derive(Debug)]
pub struct Outcome {
    /// True when the program exited with success.
    pub success: bool,
    /// What the program wrote to its error stream.
    pub message: String,
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

/// Runs `command` until it ends, `timeout` passes, or `cancel` is set.
pub fn run(
    mut command: Command,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Result<Outcome, AppError> {
    quiet(&mut command);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::from_io("start the text recognizer", &e))?;
    let stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(stream) = stderr {
            let _ = stream.take(MAX_MESSAGE).read_to_string(&mut text);
        }
        text
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => return Err(AppError::from_io("wait for the text recognizer", &e)),
        }
        if cancel.check().is_err() {
            kill_tree(&mut child);
            let _ = reader.join();
            return Err(AppError::cancelled());
        }
        if started.elapsed() > timeout {
            kill_tree(&mut child);
            let _ = reader.join();
            return Err(AppError::new(
                ErrorCode::EngineTimeout,
                "Reading a page took too long. Try a lower resolution.",
            ));
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    Ok(Outcome {
        success: status.success(),
        message: reader.join().unwrap_or_default(),
    })
}
