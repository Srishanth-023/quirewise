//! Starting and stopping the headless browser.

use application::AppError;
use application::CancellationToken;
use domain::ErrorCode;
use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Keeps a console window from flashing up while a helper runs.
pub fn quiet(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// Stops a program and everything it started.
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

/// A running headless browser. It is stopped when this value is dropped.
pub struct Browser {
    child: Child,
    endpoint: String,
}

fn start_failed() -> AppError {
    AppError::new(
        ErrorCode::EngineTimeout,
        "The web page printer did not start. Close other copies of Microsoft Edge that are stuck, then try again.",
    )
}

impl Browser {
    /// Starts the browser with an empty profile in `profile` and waits until it says where it
    /// listens.
    pub fn start(
        program: &Path,
        profile: &Path,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<Self, AppError> {
        cancel.check()?;
        let mut command = Command::new(program);
        command
            .args([
                "--headless=new",
                "--disable-gpu",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-extensions",
                "--disable-sync",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-default-apps",
                "--disable-popup-blocking",
                "--mute-audio",
                "--hide-scrollbars",
                "--remote-debugging-port=0",
            ])
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("about:blank");
        quiet(&mut command);
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AppError::from_io("start Microsoft Edge", &e))?;
        let started = Instant::now();
        let port_file = profile.join("DevToolsActivePort");
        loop {
            if cancel.check().is_err() {
                kill_tree(&mut child);
                return Err(AppError::cancelled());
            }
            if let Ok(text) = fs::read_to_string(&port_file) {
                let mut lines = text.lines();
                if let (Some(port), Some(path)) = (lines.next(), lines.next())
                    && port.trim().parse::<u16>().is_ok()
                    && path.starts_with('/')
                {
                    return Ok(Self {
                        child,
                        endpoint: format!("ws://127.0.0.1:{}{}", port.trim(), path.trim()),
                    });
                }
            }
            if matches!(child.try_wait(), Ok(Some(_))) || started.elapsed() > timeout {
                kill_tree(&mut child);
                return Err(start_failed());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The address of the browser's DevTools socket.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        kill_tree(&mut self.child);
    }
}
