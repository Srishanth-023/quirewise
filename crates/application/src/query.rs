//! Read-only document queries (inspect, thumbnails). They parse untrusted PDFs, so in production
//! they run inside the worker like tool runs do.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use domain::limits::SafetyLimits;
use domain::{DomainError, PageIndex};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::AppError;
use crate::input::validate_input;
use crate::inspect::{DocumentReport, InspectDocument};
use crate::ports::{FileStore, PdfReader, Renderer};
use crate::runtime::{CancellationToken, Password};
use crate::tool::InputKind;

/// Smallest and largest thumbnail edge accepted.
pub const THUMBNAIL_EDGE_RANGE: (u32, u32) = (16, 2048);

/// Bytes that travel as a base64 string in JSON.
#[derive(Clone, PartialEq, Eq)]
pub struct Base64Bytes(pub Vec<u8>);

impl core::fmt::Debug for Base64Bytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Base64Bytes({} bytes)", self.0.len())
    }
}

impl Serialize for Base64Bytes {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Base64Bytes {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        STANDARD
            .decode(text.as_bytes())
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Base64Bytes {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Base64Bytes".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({ "type": "string", "contentEncoding": "base64" })
    }
}

/// A question about one document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum DocumentQuery {
    /// Facts for the Inspect panel.
    Inspect {
        /// File to inspect.
        path: PathBuf,
        /// Password, when the user entered one this session.
        password: Option<Password>,
    },
    /// A rendered page image.
    Thumbnail {
        /// File to render.
        path: PathBuf,
        /// Password, when the user entered one this session.
        password: Option<Password>,
        /// Zero-based page index.
        page: u32,
        /// Longest edge in pixels, 16 to 2048.
        max_edge: u32,
    },
}

/// A rendered page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Thumbnail {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Page count of the document, so the viewer knows how many pages to ask for.
    pub page_count: u32,
    /// PNG bytes.
    pub png: Base64Bytes,
}

/// Answer to a [`DocumentQuery`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum QueryResult {
    /// Inspect result.
    Inspect(Box<DocumentReport>),
    /// Thumbnail result.
    Thumbnail(Thumbnail),
}

/// Runs document queries. Implemented in-process by [`QueryService`] and over IPC by the
/// worker session.
pub trait QueryRunner: Send + Sync {
    /// Answers one query.
    fn query(
        &self,
        query: &DocumentQuery,
        cancel: &CancellationToken,
    ) -> Result<QueryResult, AppError>;
}

/// In-process implementation of [`QueryRunner`].
pub struct QueryService {
    files: Arc<dyn FileStore>,
    reader: Arc<dyn PdfReader>,
    renderer: Arc<dyn Renderer>,
    inspect: InspectDocument,
    limits: SafetyLimits,
}

impl QueryService {
    /// Wires the service to its ports.
    pub fn new(
        files: Arc<dyn FileStore>,
        reader: Arc<dyn PdfReader>,
        renderer: Arc<dyn Renderer>,
        inspect: InspectDocument,
        limits: SafetyLimits,
    ) -> Self {
        Self {
            files,
            reader,
            renderer,
            inspect,
            limits,
        }
    }
}

impl QueryRunner for QueryService {
    fn query(
        &self,
        query: &DocumentQuery,
        cancel: &CancellationToken,
    ) -> Result<QueryResult, AppError> {
        cancel.check()?;
        match query {
            DocumentQuery::Inspect { path, password } => self
                .inspect
                .run(path, password.as_ref())
                .map(|r| QueryResult::Inspect(Box::new(r))),
            DocumentQuery::Thumbnail {
                path,
                password,
                page,
                max_edge,
            } => {
                let (min, max) = THUMBNAIL_EDGE_RANGE;
                if !(min..=max).contains(max_edge) {
                    return Err(DomainError::option(
                        "max_edge",
                        format!("must be between {min} and {max}"),
                    )
                    .into());
                }
                let (doc, _) =
                    validate_input(self.files.as_ref(), path, &[InputKind::Pdf], &self.limits)?;
                let info = self.reader.open_info(&doc.path, password.as_ref())?;
                if *page >= info.page_count {
                    return Err(DomainError::RangeOutOfBounds {
                        detail: format!("Page {} is outside this document.", page + 1),
                        valid: format!("1-{}", info.page_count),
                    }
                    .into());
                }
                let image = self.renderer.render_png(
                    &doc.path,
                    password.as_ref(),
                    PageIndex::new(*page),
                    *max_edge,
                )?;
                Ok(QueryResult::Thumbnail(Thumbnail {
                    width: image.width,
                    height: image.height,
                    page_count: info.page_count,
                    png: Base64Bytes(image.png),
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::{PdfiumFacts, StructureReport};
    use crate::ports::{PdfFacts, PngImage, RenderedImage, StructureFacts};
    use crate::testing::{MemoryFiles, StubReader};
    use domain::{Dpi, ErrorCode};
    use std::path::Path;

    struct FakeRenderer;
    impl Renderer for FakeRenderer {
        fn render_page(
            &self,
            _: &Path,
            _: Option<&Password>,
            _: PageIndex,
            _: Dpi,
        ) -> Result<RenderedImage, AppError> {
            Err(AppError::internal("unused in this test"))
        }
        fn render_png(
            &self,
            _: &Path,
            _: Option<&Password>,
            page: PageIndex,
            max_edge: u32,
        ) -> Result<PngImage, AppError> {
            Ok(PngImage {
                width: max_edge,
                height: max_edge / 2,
                png: vec![0x89, b'P', b'N', b'G', page.get() as u8],
            })
        }
    }
    struct NoFacts;
    impl PdfFacts for NoFacts {
        fn facts(&self, _: &Path, _: Option<&Password>) -> Result<PdfiumFacts, AppError> {
            Ok(PdfiumFacts::default())
        }
    }
    struct NoStructure;
    impl StructureFacts for NoStructure {
        fn structure(&self, _: &Path, _: Option<&Password>) -> Result<StructureReport, AppError> {
            Ok(StructureReport::default())
        }
    }

    fn service() -> QueryService {
        let files = Arc::new(MemoryFiles::default());
        files.put("C:\\a.pdf", b"%PDF-1.7 x");
        let inspect = InspectDocument::new(
            files.clone(),
            Arc::new(NoFacts),
            Arc::new(NoStructure),
            SafetyLimits::default(),
        );
        QueryService::new(
            files,
            Arc::new(StubReader::default().with("C:\\a.pdf", 3)),
            Arc::new(FakeRenderer),
            inspect,
            SafetyLimits::default(),
        )
    }

    fn thumb(page: u32, edge: u32) -> DocumentQuery {
        DocumentQuery::Thumbnail {
            path: PathBuf::from("C:\\a.pdf"),
            password: None,
            page,
            max_edge: edge,
        }
    }

    #[test]
    fn thumbnails_report_page_count_and_png_bytes() {
        let QueryResult::Thumbnail(t) = service()
            .query(&thumb(2, 200), &CancellationToken::new())
            .unwrap()
        else {
            panic!("expected a thumbnail");
        };
        assert_eq!((t.width, t.height, t.page_count), (200, 100, 3));
        assert_eq!(t.png.0[1..4], *b"PNG");
    }

    #[test]
    fn bad_pages_and_edges_are_rejected() {
        let s = service();
        let token = CancellationToken::new();
        assert_eq!(
            s.query(&thumb(3, 200), &token).unwrap_err().code,
            ErrorCode::RangeOutOfBounds
        );
        assert_eq!(
            s.query(&thumb(0, 8), &token).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
        assert_eq!(
            s.query(&thumb(0, 5000), &token).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
    }

    #[test]
    fn cancelled_queries_do_not_run() {
        let token = CancellationToken::new();
        token.cancel();
        assert_eq!(
            service().query(&thumb(0, 200), &token).unwrap_err().code,
            ErrorCode::JobCancelled
        );
    }

    #[test]
    fn inspect_query_returns_a_report() {
        let q = DocumentQuery::Inspect {
            path: PathBuf::from("C:\\a.pdf"),
            password: None,
        };
        assert!(matches!(
            service().query(&q, &CancellationToken::new()),
            Ok(QueryResult::Inspect(_))
        ));
    }

    #[test]
    fn bytes_travel_as_base64_json() {
        let json = serde_json::to_string(&Base64Bytes(vec![1, 2, 3, 250])).unwrap();
        assert_eq!(json, "\"AQID+g==\"");
        let back: Base64Bytes = serde_json::from_str(&json).unwrap();
        assert_eq!(back.0, vec![1, 2, 3, 250]);
        assert!(serde_json::from_str::<Base64Bytes>("\"!!!\"").is_err());
    }
}
