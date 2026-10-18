//! Job manager: runs batches in the background, streams events, supports cancel and retry.
//!
//! It wraps [`JobQueue`] so the Tauri shell only forwards commands and events.

use application::batch::{BatchReport, FileReport};
use application::dto::BatchEvent;
use application::{AppError, CancellationToken, ProgressEvent, UnitRunner};
use domain::job::JobStatus;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::queue::{BatchObserver, BatchRequest, JobQueue};

/// Receives events from running jobs. Called from worker threads.
pub trait EventSink: Send + Sync {
    /// Delivers one event to the UI.
    fn emit(&self, event: BatchEvent);
}

struct JobEntry {
    request: BatchRequest,
    cancel: CancellationToken,
    report: Option<BatchReport>,
    running: bool,
    outputs: HashSet<PathBuf>,
}

type Jobs = Arc<Mutex<HashMap<u64, JobEntry>>>;

fn lock(jobs: &Jobs) -> std::sync::MutexGuard<'_, HashMap<u64, JobEntry>> {
    jobs.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Observer {
    job: u64,
    sink: Arc<dyn EventSink>,
}

impl BatchObserver for Observer {
    fn on_progress(&self, unit: usize, event: ProgressEvent) {
        self.sink.emit(BatchEvent::Progress {
            job: self.job,
            unit,
            done: event.done,
            total: event.total,
        });
    }

    fn on_finished(&self, unit: usize, report: &FileReport) {
        self.sink.emit(BatchEvent::UnitFinished {
            job: self.job,
            unit,
            status: report.status,
            error: report.error.clone(),
            outputs: report.outputs.clone(),
        });
    }
}

/// Runs and tracks batch jobs.
pub struct JobManager {
    runner: Arc<dyn UnitRunner>,
    slots: usize,
    sink: Arc<dyn EventSink>,
    jobs: Jobs,
    next_id: AtomicU64,
}

impl JobManager {
    /// Creates a manager that runs units with `slots` at a time.
    pub fn new(runner: Arc<dyn UnitRunner>, slots: usize, sink: Arc<dyn EventSink>) -> Self {
        Self {
            runner,
            slots,
            sink,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
        }
    }

    /// Starts a batch in the background and returns its job id.
    pub fn start(&self, request: BatchRequest) -> u64 {
        let job = self.next_id.fetch_add(1, Ordering::SeqCst);
        let cancel = CancellationToken::new();
        lock(&self.jobs).insert(
            job,
            JobEntry {
                request: request.clone(),
                cancel: cancel.clone(),
                report: None,
                running: true,
                outputs: HashSet::new(),
            },
        );
        self.spawn(job, request, cancel, None);
        job
    }

    /// Runs again only the units that failed. Fails when the job is unknown, still running, or
    /// has nothing to retry. The job keeps its id; results arrive as events again.
    pub fn retry_failed(&self, job: u64) -> Result<(), AppError> {
        let (request, previous, cancel) = {
            let mut jobs = lock(&self.jobs);
            let entry = jobs
                .get_mut(&job)
                .ok_or_else(|| AppError::internal("finding that job"))?;
            let nothing =
                || domain::DomainError::option("retry", "there are no failed files to retry");
            let previous = match (&entry.report, entry.running) {
                (Some(report), false) if !report.retryable().is_empty() => report.clone(),
                _ => return Err(nothing().into()),
            };
            entry.running = true;
            entry.cancel = CancellationToken::new();
            (entry.request.clone(), previous, entry.cancel.clone())
        };
        self.spawn(job, request, cancel, Some(previous));
        Ok(())
    }

    /// Asks a running job to stop. Returns false when the job is unknown.
    pub fn cancel(&self, job: u64) -> bool {
        match lock(&self.jobs).get(&job) {
            Some(entry) => {
                entry.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// True when `path` was produced by some job. Used to restrict "open" and "show in folder".
    pub fn is_known_output(&self, path: &Path) -> bool {
        lock(&self.jobs).values().any(|e| e.outputs.contains(path))
    }

    /// Overall status of a job, if it exists.
    pub fn status(&self, job: u64) -> Option<JobStatus> {
        let jobs = lock(&self.jobs);
        let entry = jobs.get(&job)?;
        Some(if entry.running {
            JobStatus::Running
        } else {
            entry
                .report
                .as_ref()
                .map_or(JobStatus::Queued, BatchReport::status)
        })
    }

    fn spawn(
        &self,
        job: u64,
        request: BatchRequest,
        cancel: CancellationToken,
        previous: Option<BatchReport>,
    ) {
        let queue = JobQueue::new(Arc::clone(&self.runner), self.slots);
        let sink = Arc::clone(&self.sink);
        let jobs = Arc::clone(&self.jobs);
        std::thread::spawn(move || {
            let observer = Observer {
                job,
                sink: Arc::clone(&sink),
            };
            let report = match previous {
                Some(previous) => queue.retry_failed(&request, &previous, &cancel, &observer),
                None => queue.run(&request, &cancel, &observer),
            };
            let status = report.status();
            {
                let mut guard = lock(&jobs);
                if let Some(entry) = guard.get_mut(&job) {
                    entry.outputs.extend(
                        report
                            .files
                            .iter()
                            .flat_map(|f| f.outputs.iter().map(|o| o.path.clone())),
                    );
                    entry.report = Some(report);
                    entry.running = false;
                }
            }
            sink.emit(BatchEvent::JobFinished { job, status });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::{InputDoc, InputMode, OutputArtifact, ProgressSink, UnitRequest};
    use domain::job::FileStatus;
    use domain::naming::OutputNamingPattern;
    use domain::{ErrorCode, ToolId, ToolOptions};
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::time::Duration;

    struct Collect(Mutex<Sender<BatchEvent>>);
    impl EventSink for Collect {
        fn emit(&self, event: BatchEvent) {
            let _ = self.0.lock().unwrap().send(event);
        }
    }

    /// Fails files named `bad*` the first time only, so a retry can succeed.
    struct Runner {
        attempts: Mutex<HashMap<String, u32>>,
    }
    impl UnitRunner for Runner {
        fn run_unit(
            &self,
            request: &UnitRequest,
            cancel: &CancellationToken,
            progress: &dyn ProgressSink,
        ) -> Result<Vec<OutputArtifact>, AppError> {
            cancel.check()?;
            progress.report(ProgressEvent { done: 1, total: 1 });
            let name = request.inputs[0].path.to_string_lossy().into_owned();
            let n = {
                let mut a = self.attempts.lock().unwrap();
                let n = a.entry(name.clone()).or_insert(0);
                *n += 1;
                *n
            };
            if name.contains("bad") && n == 1 {
                return Err(AppError::new(ErrorCode::PdfDamaged, "damaged"));
            }
            Ok(vec![OutputArtifact {
                path: PathBuf::from(format!("{name}.out")),
                pages: Some(1),
                bytes: 1,
            }])
        }
    }

    fn manager() -> (JobManager, Receiver<BatchEvent>) {
        let (tx, rx) = channel();
        let runner = Arc::new(Runner {
            attempts: Mutex::new(HashMap::new()),
        });
        (
            JobManager::new(runner, 2, Arc::new(Collect(Mutex::new(tx)))),
            rx,
        )
    }

    fn request(names: &[&str]) -> BatchRequest {
        BatchRequest {
            tool: ToolId::Rotate,
            mode: InputMode::PerFile,
            options: ToolOptions::defaults_for(ToolId::Rotate),
            inputs: names.iter().map(|n| InputDoc::new(*n)).collect(),
            output_dir: None,
            naming_pattern: OutputNamingPattern::default(),
        }
    }

    fn until_finished(rx: &Receiver<BatchEvent>) -> (Vec<BatchEvent>, JobStatus) {
        let mut seen = Vec::new();
        loop {
            let event = rx
                .recv_timeout(Duration::from_secs(10))
                .expect("job events arrive");
            if let BatchEvent::JobFinished { status, .. } = event {
                return (seen, status);
            }
            seen.push(event);
        }
    }

    #[test]
    fn streams_progress_and_results_then_finishes() {
        let (m, rx) = manager();
        let job = m.start(request(&["a.pdf", "b.pdf"]));
        let (events, status) = until_finished(&rx);
        assert_eq!(status, JobStatus::Completed);
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, BatchEvent::Progress { .. }))
                .count(),
            2
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(
                    e,
                    BatchEvent::UnitFinished {
                        status: FileStatus::Done,
                        ..
                    }
                ))
                .count(),
            2
        );
        assert_eq!(m.status(job), Some(JobStatus::Completed));
        assert!(m.is_known_output(Path::new("a.pdf.out")));
        assert!(!m.is_known_output(Path::new("C:\\Windows\\System32\\cmd.exe")));
    }

    #[test]
    fn retry_runs_only_the_failed_files() {
        let (m, rx) = manager();
        let job = m.start(request(&["ok.pdf", "bad.pdf"]));
        let (_, first) = until_finished(&rx);
        assert_eq!(first, JobStatus::CompletedWithErrors);

        m.retry_failed(job).unwrap();
        let (events, second) = until_finished(&rx);
        assert_eq!(second, JobStatus::Completed);
        let finished: Vec<usize> = events
            .iter()
            .filter_map(|e| match e {
                BatchEvent::UnitFinished { unit, .. } => Some(*unit),
                _ => None,
            })
            .collect();
        assert_eq!(finished, vec![1], "only the failed unit ran again");
        assert!(m.retry_failed(job).is_err(), "nothing left to retry");
    }

    #[test]
    fn cancel_and_unknown_jobs() {
        let (m, _rx) = manager();
        assert!(!m.cancel(999));
        assert!(m.retry_failed(999).is_err());
        assert_eq!(m.status(999), None);
    }
}
