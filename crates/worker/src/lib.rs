//! Worker process library: the stdio serve loop and Job Object confinement.
//!
//! The worker is the only process that parses untrusted PDF bytes. It reads length-prefixed JSON
//! requests from stdin, runs each unit on its own thread so `Cancel` stays responsive, and
//! writes progress and results to stdout.
#![deny(unsafe_code)]

pub mod engine;
pub mod job_object;

use application::ipc::{Request, Response, read_frame, write_frame};
use application::query::QueryRunner;
use application::{AppError, CancellationToken, ProgressEvent, ProgressSink, UnitRunner};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, PoisonError};

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

struct FrameProgress<'a, W: Write + Send> {
    id: u64,
    out: &'a Mutex<W>,
}

impl<W: Write + Send> ProgressSink for FrameProgress<'_, W> {
    fn report(&self, event: ProgressEvent) {
        // Progress is advisory; a closed pipe ends the job through the main loop instead.
        let _ = write_frame(
            &mut *lock(self.out),
            &Response::Progress { id: self.id, event },
        );
    }
}

/// What the worker answers requests with.
#[derive(Clone)]
pub struct Services {
    /// Runs tool units.
    pub runner: Arc<dyn UnitRunner>,
    /// Answers read-only document queries.
    pub queries: Arc<dyn QueryRunner>,
}

/// Serves requests until `Shutdown` or end of input. Running units are cancelled on exit.
pub fn serve<R, W>(mut input: R, output: W, services: Services) -> Result<(), AppError>
where
    R: Read,
    W: Write + Send,
{
    let out = Mutex::new(output);
    let running: Mutex<HashMap<u64, CancellationToken>> = Mutex::new(HashMap::new());

    std::thread::scope(|scope| {
        let result = loop {
            match read_frame::<Request>(&mut input) {
                Ok(Some(Request::Ping)) => {
                    let pong = Response::Pong {
                        version: env!("CARGO_PKG_VERSION").to_owned(),
                    };
                    if let Err(e) = write_frame(&mut *lock(&out), &pong) {
                        break Err(e);
                    }
                }
                Ok(Some(Request::Run { id, unit })) => {
                    let token = CancellationToken::new();
                    lock(&running).insert(id, token.clone());
                    let (out, running, runner) = (&out, &running, Arc::clone(&services.runner));
                    scope.spawn(move || {
                        let progress = FrameProgress { id, out };
                        let reply = match runner.run_unit(&unit, &token, &progress) {
                            Ok(outputs) => Response::Finished { id, outputs },
                            Err(error) => Response::Failed { id, error },
                        };
                        lock(running).remove(&id);
                        let _ = write_frame(&mut *lock(out), &reply);
                    });
                }
                Ok(Some(Request::Query { id, query })) => {
                    let token = CancellationToken::new();
                    lock(&running).insert(id, token.clone());
                    let (out, running, queries) = (&out, &running, Arc::clone(&services.queries));
                    scope.spawn(move || {
                        let reply = match queries.query(&query, &token) {
                            Ok(result) => Response::Answer {
                                id,
                                result: Box::new(result),
                            },
                            Err(error) => Response::Failed { id, error },
                        };
                        lock(running).remove(&id);
                        let _ = write_frame(&mut *lock(out), &reply);
                    });
                }
                Ok(Some(Request::Cancel { id })) => {
                    if let Some(token) = lock(&running).get(&id) {
                        token.cancel();
                    }
                }
                Ok(Some(Request::Shutdown) | None) => break Ok(()),
                Err(e) => break Err(e),
            }
        };
        for token in lock(&running).values() {
            token.cancel();
        }
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::query::{Base64Bytes, DocumentQuery, QueryResult, Thumbnail};
    use application::{OutputArtifact, UnitRequest};
    use domain::naming::OutputNamingPattern;
    use domain::{ErrorCode, ToolId, ToolOptions};
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::time::Duration;

    struct FakeRunner;

    impl UnitRunner for FakeRunner {
        fn run_unit(
            &self,
            request: &UnitRequest,
            cancel: &CancellationToken,
            progress: &dyn ProgressSink,
        ) -> Result<Vec<OutputArtifact>, AppError> {
            let name = request.inputs[0].path.to_string_lossy().into_owned();
            if name.contains("wait") {
                while !cancel.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                return Err(AppError::cancelled());
            }
            progress.report(ProgressEvent { done: 1, total: 2 });
            progress.report(ProgressEvent { done: 2, total: 2 });
            Ok(vec![OutputArtifact {
                path: PathBuf::from("out.pdf"),
                pages: Some(2),
                bytes: 10,
            }])
        }
    }

    struct FakeQueries;

    impl QueryRunner for FakeQueries {
        fn query(
            &self,
            _query: &DocumentQuery,
            _cancel: &CancellationToken,
        ) -> Result<QueryResult, AppError> {
            Ok(QueryResult::Thumbnail(Thumbnail {
                width: 2,
                height: 3,
                page_count: 4,
                png: Base64Bytes(vec![1, 2, 3]),
            }))
        }
    }

    fn services() -> Services {
        Services {
            runner: Arc::new(FakeRunner),
            queries: Arc::new(FakeQueries),
        }
    }

    fn unit(path: &str) -> UnitRequest {
        UnitRequest {
            tool: ToolId::Rotate,
            options: ToolOptions::defaults_for(ToolId::Rotate),
            inputs: vec![application::InputDoc::new(path)],
            output_dir: None,
            naming_pattern: OutputNamingPattern::default(),
        }
    }

    fn frames(requests: &[Request]) -> Cursor<Vec<u8>> {
        let mut buf = Vec::new();
        for r in requests {
            write_frame(&mut buf, r).unwrap();
        }
        Cursor::new(buf)
    }

    fn responses(bytes: Vec<u8>) -> Vec<Response> {
        let mut cursor = Cursor::new(bytes);
        let mut all = Vec::new();
        while let Some(r) = read_frame::<Response>(&mut cursor).unwrap() {
            all.push(r);
        }
        all
    }

    /// Shares the output buffer so the test can read it after `serve` returns.
    #[derive(Clone, Default)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuf {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            lock(&self.0).extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn answers_ping_runs_units_and_stops_on_shutdown() {
        let input = frames(&[
            Request::Ping,
            Request::Run {
                id: 5,
                unit: Box::new(unit("a.pdf")),
            },
            Request::Shutdown,
        ]);
        let out = SharedBuf::default();
        serve(input, out.clone(), services()).unwrap();
        let replies = responses(lock(&out.0).clone());

        assert!(matches!(replies[0], Response::Pong { .. }));
        let finished = replies
            .iter()
            .any(|r| matches!(r, Response::Finished { id: 5, outputs } if outputs.len() == 1));
        assert!(
            finished,
            "unit result is delivered even when Shutdown follows immediately"
        );
        let progress = replies
            .iter()
            .filter(|r| matches!(r, Response::Progress { id: 5, .. }))
            .count();
        assert_eq!(progress, 2);
    }

    #[test]
    fn cancel_stops_a_running_unit_with_the_cancelled_code() {
        let input = frames(&[
            Request::Run {
                id: 9,
                unit: Box::new(unit("wait.pdf")),
            },
            Request::Cancel { id: 9 },
            Request::Shutdown,
        ]);
        let out = SharedBuf::default();
        serve(input, out.clone(), services()).unwrap();
        let replies = responses(lock(&out.0).clone());
        let cancelled = replies.iter().any(|r| {
            matches!(r, Response::Failed { id: 9, error } if error.code == ErrorCode::JobCancelled)
        });
        assert!(cancelled);
    }

    #[test]
    fn end_of_input_cancels_running_units_and_exits_cleanly() {
        let input = frames(&[Request::Run {
            id: 1,
            unit: Box::new(unit("wait.pdf")),
        }]);
        let out = SharedBuf::default();
        assert!(serve(input, out.clone(), services()).is_ok());
        let replies = responses(lock(&out.0).clone());
        assert!(
            replies
                .iter()
                .any(|r| matches!(r, Response::Failed { id: 1, .. }))
        );
    }

    #[test]
    fn answers_queries_by_correlation_id() {
        let query = DocumentQuery::Thumbnail {
            path: PathBuf::from("a.pdf"),
            password: None,
            page: 0,
            max_edge: 64,
        };
        let input = frames(&[Request::Query { id: 11, query }, Request::Shutdown]);
        let out = SharedBuf::default();
        serve(input, out.clone(), services()).unwrap();
        let replies = responses(lock(&out.0).clone());
        assert!(replies.iter().any(|r| matches!(
            r,
            Response::Answer { id: 11, result } if matches!(**result, QueryResult::Thumbnail(_))
        )));
    }

    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        let garbage = Cursor::new(vec![3, 0, 0, 0, b'{', b'x', b'}']);
        assert!(serve(garbage, SharedBuf::default(), services()).is_err());
    }
}
