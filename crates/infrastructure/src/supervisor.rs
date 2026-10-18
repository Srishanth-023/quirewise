//! Runs units of work in a separate `quirewise-worker` process over stdio IPC.
//!
//! One worker process per unit keeps a crash or runaway job from touching anything else. The
//! worker puts itself into a Windows Job Object (memory and CPU caps); the supervisor adds the
//! wall-clock timeout and forwards cancellation.

use application::ipc::{Request, Response, read_frame, write_frame};
use application::{
    AppError, CancellationToken, OutputArtifact, ProgressSink, UnitRequest, UnitRunner,
};
use domain::ErrorCode;
use domain::limits::SafetyLimits;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Windows process creation flag that stops a console window from flashing.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const UNIT_ID: u64 = 1;

/// [`UnitRunner`] that delegates to a worker process.
#[derive(Debug, Clone)]
pub struct WorkerSupervisor {
    worker_exe: PathBuf,
    limits: SafetyLimits,
}

impl WorkerSupervisor {
    /// Creates a supervisor for the worker executable at `worker_exe`.
    pub fn new(worker_exe: PathBuf, limits: SafetyLimits) -> Self {
        Self { worker_exe, limits }
    }

    fn spawn(&self) -> Result<Child, AppError> {
        let mut command = Command::new(&self.worker_exe);
        command
            .arg("--max-memory-bytes")
            .arg(self.limits.max_memory.get().to_string())
            .arg("--max-cpu-seconds")
            .arg(self.limits.max_cpu_seconds.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        command.spawn().map_err(|_| {
            AppError::new(
                ErrorCode::ComponentMissing,
                "The processing engine could not be started. Reinstall Quirewise or check that antivirus did not remove quirewise-worker.exe.",
            )
        })
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl UnitRunner for WorkerSupervisor {
    fn run_unit(
        &self,
        request: &UnitRequest,
        cancel: &CancellationToken,
        progress: &dyn ProgressSink,
    ) -> Result<Vec<OutputArtifact>, AppError> {
        cancel.check()?;
        let mut child = self.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::internal("starting the worker"))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::internal("starting the worker"))?;
        let stdin = Mutex::new(Some(stdin));
        let child = Mutex::new(child);

        let send = |req: &Request| -> Result<(), AppError> {
            match lock(&stdin).as_mut() {
                Some(pipe) => write_frame(pipe, req),
                None => Ok(()),
            }
        };
        send(&Request::Run {
            id: UNIT_ID,
            unit: Box::new(request.clone()),
        })?;

        let finished = AtomicBool::new(false);
        let timed_out = AtomicBool::new(false);
        let timeout = Duration::from_secs(u64::from(self.limits.job_timeout_seconds));
        let started = Instant::now();

        let outcome = std::thread::scope(|scope| {
            scope.spawn(|| {
                let mut cancel_sent = false;
                while !finished.load(Ordering::SeqCst) {
                    if cancel.is_cancelled() && !cancel_sent {
                        cancel_sent = send(&Request::Cancel { id: UNIT_ID }).is_ok();
                    }
                    if started.elapsed() > timeout {
                        timed_out.store(true, Ordering::SeqCst);
                        let _ = lock(&child).kill();
                        break;
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
            });

            let result = read_outcome(&mut stdout, progress);
            finished.store(true, Ordering::SeqCst);
            result
        });

        // Tell the worker to exit, close the pipe, then make sure the process is gone.
        let _ = send(&Request::Shutdown);
        drop(lock(&stdin).take());
        reap(&child);

        match outcome {
            Some(result) => result,
            None if timed_out.load(Ordering::SeqCst) => Err(AppError::new(
                ErrorCode::EngineTimeout,
                "Processing took longer than the time limit and was stopped. Try a smaller file or raise the limit in Settings.",
            )),
            None => Err(AppError::new(
                ErrorCode::PdfLimitExceeded,
                "The processing engine stopped unexpectedly, most likely because the file hit a memory or time safety limit. Try Repair first, or split the file.",
            )),
        }
    }
}

/// Reads responses until the unit finishes. Returns `None` when the worker closed the pipe
/// without answering.
fn read_outcome(
    stdout: &mut impl std::io::Read,
    progress: &dyn ProgressSink,
) -> Option<Result<Vec<OutputArtifact>, AppError>> {
    loop {
        match read_frame::<Response>(stdout) {
            Ok(Some(Response::Progress { event, .. })) => progress.report(event),
            Ok(Some(Response::Finished { outputs, .. })) => return Some(Ok(outputs)),
            Ok(Some(Response::Failed { error, .. })) => return Some(Err(error)),
            Ok(Some(Response::Pong { .. } | Response::Answer { .. })) => {}
            Ok(None) | Err(_) => return None,
        }
    }
}

/// Waits briefly for a clean exit, then kills the process.
fn reap(child: &Mutex<Child>) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let mut guard = lock(child);
        match guard.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) if Instant::now() >= deadline => {
                let _ = guard.kill();
                let _ = guard.wait();
                return;
            }
            Ok(None) => {}
        }
        drop(guard);
        std::thread::sleep(Duration::from_millis(20));
    }
}
