//! Worker protocol: length-prefixed JSON frames over the worker's stdio pipes.
//!
//! Frame = 4-byte little-endian length, then that many bytes of JSON. The length is checked
//! against [`MAX_FRAME_BYTES`] before any allocation so a hostile peer cannot exhaust memory.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

use crate::error::AppError;
use crate::query::{DocumentQuery, QueryResult};
use crate::run_tool::UnitRequest;
use crate::runtime::ProgressEvent;
use crate::tool::OutputArtifact;

/// Largest accepted frame, 16 MiB.
pub const MAX_FRAME_BYTES: u32 = 16 * 1024 * 1024;

/// Messages from the app to the worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Request {
    /// Liveness check.
    Ping,
    /// Run one unit of work.
    Run {
        /// Correlation id chosen by the app.
        id: u64,
        /// The work. Boxed: it is far larger than the other messages.
        unit: Box<UnitRequest>,
    },
    /// Answer a read-only document query (inspect, thumbnail).
    Query {
        /// Correlation id chosen by the app.
        id: u64,
        /// The question.
        query: DocumentQuery,
    },
    /// Cancel a running unit or query.
    Cancel {
        /// Id of the unit or query to cancel.
        id: u64,
    },
    /// Exit cleanly.
    Shutdown,
}

/// Messages from the worker to the app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Response {
    /// Reply to `Ping`.
    Pong {
        /// Worker version.
        version: String,
    },
    /// Progress for a running unit.
    Progress {
        /// Unit id.
        id: u64,
        /// Progress.
        event: ProgressEvent,
    },
    /// The unit finished and its outputs are verified.
    Finished {
        /// Unit id.
        id: u64,
        /// Outputs.
        outputs: Vec<OutputArtifact>,
    },
    /// A query was answered.
    Answer {
        /// Query id.
        id: u64,
        /// The result.
        result: Box<QueryResult>,
    },
    /// The unit or query failed.
    Failed {
        /// Unit id.
        id: u64,
        /// Why.
        error: AppError,
    },
}

fn protocol_error(what: &str) -> AppError {
    AppError::internal(&format!("talking to the worker ({what})"))
}

/// Writes one frame and flushes.
pub fn write_frame<T: Serialize>(writer: &mut impl Write, message: &T) -> Result<(), AppError> {
    let body = serde_json::to_vec(message).map_err(|_| protocol_error("encoding"))?;
    let len = u32::try_from(body.len()).map_err(|_| protocol_error("frame too large"))?;
    if len > MAX_FRAME_BYTES {
        return Err(protocol_error("frame too large"));
    }
    writer
        .write_all(&len.to_le_bytes())
        .and_then(|()| writer.write_all(&body))
        .and_then(|()| writer.flush())
        .map_err(|_| protocol_error("write"))
}

/// Reads one frame. Returns `Ok(None)` on a clean end of stream before a frame starts.
pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<Option<T>, AppError> {
    let mut len_bytes = [0u8; 4];
    match reader.read(&mut len_bytes[..1]) {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::Interrupted => return read_frame(reader),
        Err(_) => return Err(protocol_error("read")),
    }
    reader
        .read_exact(&mut len_bytes[1..])
        .map_err(|_| protocol_error("truncated header"))?;
    let len = u32::from_le_bytes(len_bytes);
    if len > MAX_FRAME_BYTES {
        return Err(protocol_error("frame too large"));
    }
    // Read through `take` so a lying length prefix never allocates more than was sent.
    let mut body = Vec::new();
    reader
        .take(u64::from(len))
        .read_to_end(&mut body)
        .map_err(|_| protocol_error("read"))?;
    if body.len() != len as usize {
        return Err(protocol_error("truncated body"));
    }
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|_| protocol_error("decoding"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Password;
    use crate::tool::InputDoc;
    use domain::naming::OutputNamingPattern;
    use domain::{ToolId, ToolOptions};
    use proptest::prelude::*;

    fn run_request() -> Request {
        let mut doc = InputDoc::new("C:\\in\\a.pdf");
        doc.password = Some(Password::new("pw"));
        Request::Run {
            id: 7,
            unit: Box::new(UnitRequest {
                tool: ToolId::Rotate,
                options: ToolOptions::defaults_for(ToolId::Rotate),
                inputs: vec![doc],
                output_dir: None,
                naming_pattern: OutputNamingPattern::default(),
            }),
        }
    }

    #[test]
    fn regression_list_answers_survive_the_wire() {
        use crate::query::QueryResult;
        use crate::signing::StoredCertificate;
        // An internal tag cannot be written next to a list: each of these failed to encode, and
        // the worker never answered.
        let answers = vec![
            QueryResult::OcrLanguages(vec!["eng".into(), "deu".into()]),
            QueryResult::EditorFonts(vec!["Arial".into()]),
            QueryResult::FormFields(Vec::new()),
            QueryResult::Suggestions(Vec::new()),
            QueryResult::TextBlocks(Vec::new()),
            QueryResult::RedactMatches(Vec::new()),
            QueryResult::Certificates(vec![StoredCertificate {
                thumbprint: "AB".repeat(20),
                subject: "Ada".into(),
                issuer: "Root".into(),
                valid_until: "2030-01-01".into(),
            }]),
        ];
        for result in answers {
            let reply = Response::Answer {
                id: 3,
                result: Box::new(result),
            };
            let mut buf = Vec::new();
            write_frame(&mut buf, &reply).expect("the answer can be written");
            let back = read_frame::<Response>(&mut io::Cursor::new(buf))
                .unwrap()
                .unwrap();
            assert_eq!(back, reply);
        }
    }

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &Request::Ping).unwrap();
        write_frame(&mut buf, &run_request()).unwrap();
        let mut cursor = io::Cursor::new(buf);
        assert_eq!(
            read_frame::<Request>(&mut cursor).unwrap(),
            Some(Request::Ping)
        );
        assert_eq!(
            read_frame::<Request>(&mut cursor).unwrap(),
            Some(run_request())
        );
        assert_eq!(read_frame::<Request>(&mut cursor).unwrap(), None);
    }

    #[test]
    fn responses_round_trip() {
        let msg = Response::Failed {
            id: 1,
            error: AppError::cancelled(),
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg).unwrap();
        let back: Response = read_frame(&mut io::Cursor::new(buf)).unwrap().unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn oversized_and_truncated_frames_are_rejected() {
        let huge = (MAX_FRAME_BYTES + 1).to_le_bytes().to_vec();
        assert!(read_frame::<Request>(&mut io::Cursor::new(huge)).is_err());

        let mut truncated = Vec::new();
        write_frame(&mut truncated, &Request::Ping).unwrap();
        truncated.pop();
        assert!(read_frame::<Request>(&mut io::Cursor::new(truncated)).is_err());

        assert!(read_frame::<Request>(&mut io::Cursor::new(vec![1u8, 0])).is_err());
    }

    #[test]
    fn garbage_bodies_are_rejected_not_panicking() {
        let mut frame = 3u32.to_le_bytes().to_vec();
        frame.extend_from_slice(b"{x}");
        assert!(read_frame::<Request>(&mut io::Cursor::new(frame)).is_err());
    }

    proptest! {
        #[test]
        fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
            let _ = read_frame::<Request>(&mut io::Cursor::new(bytes));
        }
    }
}
