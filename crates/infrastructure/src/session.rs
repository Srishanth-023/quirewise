//! A long-lived worker process for read-only queries (thumbnails, inspect).
//!
//! Starting a worker per thumbnail would cost far too much, so one worker stays up and answers
//! queries by correlation id. If it dies (a parser crash, a safety limit), the next query starts a
//! fresh one; the app process itself never parses a PDF.

use application::ipc::{Request, Response, read_frame, write_frame};
use application::query::{DocumentQuery, QueryResult, QueryRunner};
use application::{AppError, CancellationToken};
use domain::ErrorCode;
use domain::limits::SafetyLimits;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const POLL_INTERVAL: Duration = Duration::from_millis(50);

type Pending = Arc<Mutex<HashMap<u64, Sender<Response>>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One running worker process and its reply dispatcher.
struct Live {
    child: Child,
    stdin: Mutex<ChildStdin>,
    pending: Pending,
    alive: Arc<AtomicBool>,
}

impl Live {
    fn spawn(worker_exe: &PathBuf, limits: &SafetyLimits) -> Result<Self, AppError> {
        let mut command = Command::new(worker_exe);
        command
            .arg("--max-memory-bytes")
            .arg(limits.max_memory.get().to_string())
            .arg("--max-cpu-seconds")
            .arg(limits.max_cpu_seconds.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        let mut child = command.spawn().map_err(|_| {
            AppError::new(
                ErrorCode::ComponentMissing,
                "The processing engine could not be started. Reinstall Quirewise or check that antivirus did not remove quirewise-worker.exe.",
            )
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppError::internal("starting the worker"))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::internal("starting the worker"))?;

        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let (reader_pending, reader_alive) = (Arc::clone(&pending), Arc::clone(&alive));
        std::thread::spawn(move || {
            while let Ok(Some(response)) = read_frame::<Response>(&mut stdout) {
                let id = match &response {
                    Response::Answer { id, .. } | Response::Failed { id, .. } => *id,
                    _ => continue,
                };
                if let Some(sender) = lock(&reader_pending).remove(&id) {
                    let _ = sender.send(response);
                }
            }
            // Worker gone: dropping the senders wakes every waiting query.
            reader_alive.store(false, Ordering::SeqCst);
            lock(&reader_pending).clear();
        });
        Ok(Self {
            child,
            stdin: Mutex::new(stdin),
            pending,
            alive,
        })
    }

    fn send(&self, request: &Request) -> Result<(), AppError> {
        write_frame(&mut *lock(&self.stdin), request)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.send(&Request::Shutdown);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// [`QueryRunner`] backed by a persistent worker process.
pub struct WorkerSession {
    worker_exe: PathBuf,
    limits: SafetyLimits,
    live: Mutex<Option<Arc<Live>>>,
    next_id: AtomicU64,
}

impl WorkerSession {
    /// Creates a session. The worker starts on the first query.
    pub fn new(worker_exe: PathBuf, limits: SafetyLimits) -> Self {
        Self {
            worker_exe,
            limits,
            live: Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    fn live(&self) -> Result<Arc<Live>, AppError> {
        let mut slot = lock(&self.live);
        if let Some(live) = slot.as_ref().filter(|l| l.alive.load(Ordering::SeqCst)) {
            return Ok(Arc::clone(live));
        }
        let fresh = Arc::new(Live::spawn(&self.worker_exe, &self.limits)?);
        *slot = Some(Arc::clone(&fresh));
        Ok(fresh)
    }
}

impl QueryRunner for WorkerSession {
    fn query(
        &self,
        query: &DocumentQuery,
        cancel: &CancellationToken,
    ) -> Result<QueryResult, AppError> {
        cancel.check()?;
        let live = self.live()?;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        lock(&live.pending).insert(id, sender);
        let sent = live.send(&Request::Query {
            id,
            query: Box::new(query.clone()),
        });
        if let Err(e) = sent {
            lock(&live.pending).remove(&id);
            return Err(e);
        }

        let timeout = Duration::from_secs(u64::from(self.limits.job_timeout_seconds));
        let started = Instant::now();
        let mut cancel_sent = false;
        loop {
            match receiver.recv_timeout(POLL_INTERVAL) {
                Ok(Response::Answer { result, .. }) => return Ok(*result),
                Ok(Response::Failed { error, .. }) => return Err(error),
                Ok(_) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(AppError::new(
                        ErrorCode::PdfLimitExceeded,
                        "The processing engine stopped unexpectedly, most likely because the file hit a memory or time safety limit. Try Repair first, or split the file.",
                    ));
                }
                Err(RecvTimeoutError::Timeout) => {
                    if cancel.is_cancelled() && !cancel_sent {
                        cancel_sent = live.send(&Request::Cancel { id }).is_ok();
                    }
                    if started.elapsed() > timeout {
                        lock(&live.pending).remove(&id);
                        return Err(AppError::new(
                            ErrorCode::EngineTimeout,
                            "Reading this file took longer than the time limit and was stopped.",
                        ));
                    }
                }
            }
        }
    }
}
