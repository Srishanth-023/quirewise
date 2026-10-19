//! Batch job queue: bounded concurrency, per-file status, retry failed only.

use application::batch::{BatchReport, FileReport, split_units};
use application::{
    AppError, CancellationToken, InputDoc, InputMode, ProgressEvent, ProgressSink, UnitRequest,
    UnitRunner,
};
use domain::naming::OutputNamingPattern;
use domain::{ToolId, ToolOptions};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// Everything needed to run a batch of files through one tool.
#[derive(Debug, Clone)]
pub struct BatchRequest {
    /// Tool to run.
    pub tool: ToolId,
    /// How files map onto runs (from the tool).
    pub mode: InputMode,
    /// Options, shared by every unit.
    pub options: ToolOptions,
    /// Input files with optional passwords.
    pub inputs: Vec<InputDoc>,
    /// Output folder, or `None` for next to each source.
    pub output_dir: Option<PathBuf>,
    /// Naming pattern.
    pub naming_pattern: OutputNamingPattern,
    /// True when the tool can run with no input file (it prints a web address): an empty file
    /// list then still makes one run.
    pub without_files: bool,
}

impl BatchRequest {
    /// The runs of this batch.
    fn units(&self) -> Vec<Vec<InputDoc>> {
        if self.inputs.is_empty() && self.without_files {
            return vec![Vec::new()];
        }
        split_units(self.mode, &self.inputs)
    }
}

/// Observes a running batch. Called from worker threads.
pub trait BatchObserver: Sync {
    /// A unit reported progress.
    fn on_progress(&self, unit: usize, event: ProgressEvent);
    /// A unit finished (done or failed).
    fn on_finished(&self, unit: usize, report: &FileReport);
}

struct UnitProgress<'a> {
    unit: usize,
    observer: &'a dyn BatchObserver,
}

impl ProgressSink for UnitProgress<'_> {
    fn report(&self, event: ProgressEvent) {
        self.observer.on_progress(self.unit, event);
    }
}

/// Runs batches through a [`UnitRunner`] with at most `slots` units at once.
pub struct JobQueue {
    runner: Arc<dyn UnitRunner>,
    slots: usize,
}

impl JobQueue {
    /// Creates a queue. `slots` is clamped to at least 1.
    pub fn new(runner: Arc<dyn UnitRunner>, slots: usize) -> Self {
        Self {
            runner,
            slots: slots.max(1),
        }
    }

    /// Runs every unit of the batch.
    pub fn run(
        &self,
        request: &BatchRequest,
        cancel: &CancellationToken,
        observer: &dyn BatchObserver,
    ) -> BatchReport {
        let units = request.units();
        let report = BatchReport {
            tool: request.tool,
            files: units
                .iter()
                .map(|u| FileReport::pending(u.iter().map(|d| d.path.clone()).collect()))
                .collect(),
        };
        let all: Vec<usize> = (0..units.len()).collect();
        self.run_units(request, units, report, &all, cancel, observer)
    }

    /// Runs again only the units that failed in `previous`. Cancelled and finished units keep
    /// their result. Update passwords in `request.inputs` before retrying wrong-password files.
    pub fn retry_failed(
        &self,
        request: &BatchRequest,
        previous: &BatchReport,
        cancel: &CancellationToken,
        observer: &dyn BatchObserver,
    ) -> BatchReport {
        let units = request.units();
        let retry = previous.retryable();
        self.run_units(request, units, previous.clone(), &retry, cancel, observer)
    }

    fn run_units(
        &self,
        request: &BatchRequest,
        units: Vec<Vec<InputDoc>>,
        report: BatchReport,
        indices: &[usize],
        cancel: &CancellationToken,
        observer: &dyn BatchObserver,
    ) -> BatchReport {
        let tool = report.tool;
        let shared = Mutex::new(report);
        let next = AtomicUsize::new(0);
        let workers = self.slots.min(indices.len().max(1));

        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    loop {
                        let slot = next.fetch_add(1, Ordering::SeqCst);
                        let Some(&unit) = indices.get(slot) else {
                            break;
                        };
                        let Some(inputs) = units.get(unit) else {
                            continue;
                        };
                        let result = if cancel.is_cancelled() {
                            Err(AppError::cancelled())
                        } else {
                            self.run_one(request, tool, inputs, unit, cancel, observer)
                        };
                        let finished = {
                            let mut guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
                            match guard.files.get_mut(unit) {
                                Some(file) => {
                                    file.finish(result);
                                    Some(file.clone())
                                }
                                None => None,
                            }
                        };
                        if let Some(file) = finished {
                            observer.on_finished(unit, &file);
                        }
                    }
                });
            }
        });
        shared.into_inner().unwrap_or_else(PoisonError::into_inner)
    }

    fn run_one(
        &self,
        request: &BatchRequest,
        tool: ToolId,
        inputs: &[InputDoc],
        unit: usize,
        cancel: &CancellationToken,
        observer: &dyn BatchObserver,
    ) -> Result<Vec<application::OutputArtifact>, AppError> {
        let unit_request = UnitRequest {
            tool,
            options: request.options.clone(),
            inputs: inputs.to_vec(),
            output_dir: request.output_dir.clone(),
            naming_pattern: request.naming_pattern.clone(),
        };
        let progress = UnitProgress { unit, observer };
        // A panicking engine must fail one file, not the whole batch.
        catch_unwind(AssertUnwindSafe(|| {
            self.runner.run_unit(&unit_request, cancel, &progress)
        }))
        .unwrap_or_else(|_| Err(AppError::internal("processing this file")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::OutputArtifact;
    use domain::ErrorCode;
    use domain::job::{FileStatus, JobStatus};
    use std::time::Duration;

    #[derive(Default)]
    struct Probe {
        current: AtomicUsize,
        peak: AtomicUsize,
        finished: AtomicUsize,
    }

    impl BatchObserver for Probe {
        fn on_progress(&self, _unit: usize, _event: ProgressEvent) {}
        fn on_finished(&self, _unit: usize, _report: &FileReport) {
            self.finished.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Fails files whose name contains `bad`, panics on `boom`, sleeps to measure concurrency.
    struct FakeRunner {
        probe: Arc<Probe>,
        calls: AtomicUsize,
    }

    impl UnitRunner for FakeRunner {
        fn run_unit(
            &self,
            request: &UnitRequest,
            cancel: &CancellationToken,
            progress: &dyn ProgressSink,
        ) -> Result<Vec<OutputArtifact>, AppError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let now = self.probe.current.fetch_add(1, Ordering::SeqCst) + 1;
            self.probe.peak.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(15));
            progress.report(ProgressEvent { done: 1, total: 1 });
            self.probe.current.fetch_sub(1, Ordering::SeqCst);
            cancel.check()?;
            let name = request.inputs[0].path.to_string_lossy().into_owned();
            if name.contains("boom") {
                panic!("engine bug");
            }
            if name.contains("bad") {
                return Err(AppError::new(ErrorCode::PdfDamaged, "damaged"));
            }
            Ok(vec![OutputArtifact {
                path: PathBuf::from(format!("{name}.out")),
                pages: Some(1),
                bytes: 1,
            }])
        }
    }

    fn request(names: &[&str]) -> BatchRequest {
        BatchRequest {
            without_files: false,
            tool: ToolId::Rotate,
            mode: InputMode::PerFile,
            options: ToolOptions::defaults_for(ToolId::Rotate),
            inputs: names.iter().map(|n| InputDoc::new(*n)).collect(),
            output_dir: None,
            naming_pattern: OutputNamingPattern::default(),
        }
    }

    fn queue(slots: usize) -> (JobQueue, Arc<Probe>, Arc<FakeRunner>) {
        let probe = Arc::new(Probe::default());
        let runner = Arc::new(FakeRunner {
            probe: probe.clone(),
            calls: AtomicUsize::new(0),
        });
        (JobQueue::new(runner.clone(), slots), probe, runner)
    }

    #[test]
    fn concurrency_is_bounded_by_the_slot_count() {
        let (q, probe, _) = queue(2);
        let names: Vec<String> = (0..8).map(|i| format!("f{i}.pdf")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let report = q.run(&request(&refs), &CancellationToken::new(), &*probe);
        assert_eq!(report.status(), JobStatus::Completed);
        assert!(probe.peak.load(Ordering::SeqCst) <= 2);
        assert!(probe.peak.load(Ordering::SeqCst) >= 1);
        assert_eq!(probe.finished.load(Ordering::SeqCst), 8);
    }

    #[test]
    fn per_file_status_and_retry_failed_only() {
        let (q, probe, runner) = queue(3);
        let req = request(&["ok.pdf", "bad.pdf", "boom.pdf"]);
        let report = q.run(&req, &CancellationToken::new(), &*probe);
        assert_eq!(report.files[0].status, FileStatus::Done);
        assert_eq!(report.files[1].status, FileStatus::Damaged);
        assert_eq!(report.files[2].status, FileStatus::Failed);
        assert_eq!(report.status(), JobStatus::CompletedWithErrors);

        let before = runner.calls.load(Ordering::SeqCst);
        let retried = q.retry_failed(&req, &report, &CancellationToken::new(), &*probe);
        assert_eq!(
            runner.calls.load(Ordering::SeqCst) - before,
            2,
            "done file not rerun"
        );
        assert_eq!(retried.files[0], report.files[0]);
    }

    #[test]
    fn cancelling_marks_unstarted_files_cancelled() {
        let (q, probe, runner) = queue(1);
        let token = CancellationToken::new();
        token.cancel();
        let report = q.run(&request(&["a.pdf", "b.pdf"]), &token, &*probe);
        assert_eq!(runner.calls.load(Ordering::SeqCst), 0);
        assert!(
            report
                .files
                .iter()
                .all(|f| f.status == FileStatus::Cancelled)
        );
        assert_eq!(report.status(), JobStatus::Cancelled);
        assert!(report.retryable().is_empty());
    }

    #[test]
    fn merge_style_tools_run_once_for_all_files() {
        let (q, probe, runner) = queue(4);
        let mut req = request(&["a.pdf", "b.pdf", "c.pdf"]);
        req.mode = InputMode::AllTogether;
        let report = q.run(&req, &CancellationToken::new(), &*probe);
        assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
        assert_eq!(report.files.len(), 1);
        assert_eq!(report.files[0].inputs.len(), 3);
    }

    #[test]
    fn a_batch_without_files_runs_once_when_the_tool_allows_it() {
        let (q, probe, runner) = queue(2);
        let mut req = request(&[]);
        req.without_files = true;
        let report = q.run(&req, &CancellationToken::new(), &*probe);
        assert_eq!(report.files.len(), 1);
        assert!(report.files[0].inputs.is_empty());
        assert_eq!(runner.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn empty_batches_complete_immediately() {
        let (q, probe, _) = queue(2);
        let report = q.run(&request(&[]), &CancellationToken::new(), &*probe);
        assert!(report.files.is_empty());
    }
}
