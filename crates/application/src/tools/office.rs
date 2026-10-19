//! The tools that hand a document to a program outside Quirewise: Word to PDF (row 19),
//! PowerPoint to PDF (row 20), Excel to PDF (row 21) and HTML to PDF (row 22).
//!
//! Office documents go to Microsoft Office when it is installed, otherwise to LibreOffice. Web
//! pages go to the web view that ships with Windows.

use std::path::PathBuf;

use domain::tools::ToolGroup;
use domain::tools::office::{
    ExcelToPdfOptions, HtmlToPdfOptions, OfficeEngine, PowerPointToPdfOptions, WordToPdfOptions,
};
use domain::{Duplicates, ErrorCode, PageRange, ToolId, ToolOptions};

use super::common::{Progress, Scratch, ToolDeps, first_input, publish_pdf_checked, wrong_options};
use crate::error::AppError;
use crate::ports::{HtmlSource, OfficeConverter, OfficeKind, OfficeRequest};
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

fn plan_for(tool: ToolId, inputs: &[InputDoc], opts: &ToolOptions, units: u64) -> Plan {
    Plan {
        tool,
        inputs: inputs.to_vec(),
        options: opts.clone(),
        total_units: units,
    }
}

/// The converter a run will use.
fn pick(
    choice: OfficeEngine,
    kind: OfficeKind,
    deps: &ToolDeps,
) -> Result<&dyn OfficeConverter, AppError> {
    let (ms, libre) = (&deps.office.microsoft, &deps.office.libre);
    let missing_libre = || {
        AppError::new(
            ErrorCode::ComponentMissing,
            "LibreOffice is not installed. Open Settings, then Components, to install it, or install Microsoft Office.",
        )
    };
    match choice {
        OfficeEngine::Auto => {
            if ms.is_available(kind) {
                Ok(ms.as_ref())
            } else if libre.is_available(kind) {
                Ok(libre.as_ref())
            } else {
                Err(missing_libre())
            }
        }
        OfficeEngine::MsOffice if ms.is_available(kind) => Ok(ms.as_ref()),
        OfficeEngine::MsOffice => Err(AppError::new(
            ErrorCode::ComponentMissing,
            "Microsoft Office was not found on this computer. Choose LibreOffice or Auto instead.",
        )),
        OfficeEngine::LibreOffice if libre.is_available(kind) => Ok(libre.as_ref()),
        OfficeEngine::LibreOffice => Err(missing_libre()),
    }
}

/// Converts one document and publishes the pages of the result that `pages` selects.
fn convert_office(
    ctx: &JobContext<'_>,
    deps: &ToolDeps,
    input: &InputDoc,
    choice: OfficeEngine,
    request: &OfficeRequest,
    pages: &PageRange,
    label: &str,
) -> Result<OutputArtifact, AppError> {
    let converter = pick(choice, request.kind, deps)?;
    ctx.cancel.check()?;
    let converted = Scratch::new(ctx, "pdf");
    converter.convert(
        &input.path,
        request,
        ctx.temp_dir,
        converted.path(),
        ctx.cancel,
    )?;
    let total = deps.reader.open_info(converted.path(), None)?.page_count;
    let selected = if *pages == PageRange::all() {
        None
    } else {
        let mut list: Vec<_> = pages.resolve(total, Duplicates::Reject)?.pages().to_vec();
        list.sort_by_key(|p| p.get());
        Some(list)
    };
    let expected = selected.as_ref().map_or(total, |s| s.len() as u32);
    publish_pdf_checked(
        ctx,
        deps,
        input,
        label,
        Some(expected),
        None,
        &mut |tmp| match &selected {
            None => ctx.files.copy_file(converted.path(), tmp),
            Some(list) => deps.writer.copy_pages(converted.path(), None, list, tmp),
        },
    )
}

macro_rules! office_tool {
    (
        $name:ident, $id:expr, $label:literal, $kind:expr, $kind_of:expr, $variant:ident, $options:ty,
        $request:expr, $pages:expr, $engine:expr
    ) => {
        #[doc = concat!("Converts documents with the tool id `", stringify!($id), "`.")]
        pub struct $name {
            deps: ToolDeps,
        }

        impl $name {
            /// Creates the tool over its ports.
            pub fn new(deps: ToolDeps) -> Self {
                Self { deps }
            }
        }

        impl Tool for $name {
            fn id(&self) -> ToolId {
                $id
            }
            fn group(&self) -> ToolGroup {
                ToolGroup::ConvertToPdf
            }
            fn label(&self) -> &'static str {
                $label
            }
            fn accepts(&self) -> &'static [InputKind] {
                &[$kind]
            }
            fn reads_pages_first(&self) -> bool {
                false
            }
            fn options_schema(&self) -> schemars::Schema {
                schemars::schema_for!($options)
            }

            fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
                let ToolOptions::$variant(options) = opts else {
                    return Err(wrong_options());
                };
                first_input(inputs)?;
                // A missing program is reported before anything is converted.
                pick($engine(options), $kind_of, &self.deps)?;
                Ok(plan_for($id, inputs, opts, 1))
            }

            fn execute(
                &self,
                ctx: &JobContext<'_>,
                plan: Plan,
            ) -> Result<Vec<OutputArtifact>, AppError> {
                let ToolOptions::$variant(options) = &plan.options else {
                    return Err(wrong_options());
                };
                let input = first_input(&plan.inputs)?;
                let mut progress = Progress::start(ctx, plan.total_units);
                let request: OfficeRequest = $request(options);
                let out = convert_office(
                    ctx,
                    &self.deps,
                    input,
                    $engine(options),
                    &request,
                    $pages(options),
                    self.label(),
                )?;
                progress.advance(1);
                Ok(vec![out])
            }
        }
    };
}

fn word_request(_: &WordToPdfOptions) -> OfficeRequest {
    OfficeRequest::plain(OfficeKind::Word)
}

fn slides_request(o: &PowerPointToPdfOptions) -> OfficeRequest {
    OfficeRequest {
        include_hidden_slides: o.include_hidden,
        ..OfficeRequest::plain(OfficeKind::Slides)
    }
}

fn sheet_request(o: &ExcelToPdfOptions) -> OfficeRequest {
    OfficeRequest {
        sheets: o.sheets.clone(),
        fit_to_page_width: o.fit_to_page_width,
        orientation: o.orientation,
        ..OfficeRequest::plain(OfficeKind::Sheet)
    }
}

office_tool!(
    WordToPdfTool,
    ToolId::WordToPdf,
    "word",
    InputKind::Word,
    OfficeKind::Word,
    WordToPdf,
    WordToPdfOptions,
    word_request,
    word_pages,
    word_engine
);

office_tool!(
    PowerPointToPdfTool,
    ToolId::PowerpointToPdf,
    "slides",
    InputKind::Slides,
    OfficeKind::Slides,
    PowerpointToPdf,
    PowerPointToPdfOptions,
    slides_request,
    slides_pages,
    slides_engine
);

fn word_pages(o: &WordToPdfOptions) -> &PageRange {
    &o.pages
}

fn slides_pages(o: &PowerPointToPdfOptions) -> &PageRange {
    &o.slides
}

/// Sheets are chosen by the converter, so the pages of the result are never cut again.
fn sheet_pages(_: &ExcelToPdfOptions) -> &'static PageRange {
    use std::sync::OnceLock;
    static ALL: OnceLock<PageRange> = OnceLock::new();
    ALL.get_or_init(PageRange::all)
}

fn word_engine(o: &WordToPdfOptions) -> OfficeEngine {
    o.engine
}

fn slides_engine(o: &PowerPointToPdfOptions) -> OfficeEngine {
    o.engine
}

fn sheet_engine(o: &ExcelToPdfOptions) -> OfficeEngine {
    o.engine
}

office_tool!(
    ExcelToPdfTool,
    ToolId::ExcelToPdf,
    "sheets",
    InputKind::Sheet,
    OfficeKind::Sheet,
    ExcelToPdf,
    ExcelToPdfOptions,
    sheet_request,
    sheet_pages,
    sheet_engine
);

/// Prints a web page or a saved HTML file.
pub struct HtmlToPdfTool {
    deps: ToolDeps,
}

impl HtmlToPdfTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

/// A name for the output of a web address: its host, so the file is easy to recognize.
fn name_for_address(url: &str) -> InputDoc {
    let host: String = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("page")
        // No user name and no port in the file name.
        .rsplit('@')
        .next()
        .unwrap_or("page")
        .split(':')
        .next()
        .unwrap_or("page")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let host = if host.is_empty() {
        "page".to_owned()
    } else {
        host
    };
    InputDoc::new(PathBuf::from(format!("{host}.html")))
}

impl Tool for HtmlToPdfTool {
    fn id(&self) -> ToolId {
        ToolId::HtmlToPdf
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::ConvertToPdf
    }
    fn label(&self) -> &'static str {
        "web page"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Html]
    }
    fn reads_pages_first(&self) -> bool {
        false
    }
    fn runs_without_files(&self, opts: &ToolOptions) -> bool {
        matches!(opts, ToolOptions::HtmlToPdf(o) if o.url.as_deref().is_some_and(|u| !u.trim().is_empty()))
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(HtmlToPdfOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::HtmlToPdf(options) = opts else {
            return Err(wrong_options());
        };
        options.validate()?;
        if options.address()?.is_none() {
            first_input(inputs)?;
        }
        if !self.deps.html.is_available() {
            return Err(AppError::new(
                ErrorCode::ComponentMissing,
                "The Microsoft Edge WebView2 Runtime is not installed, so web pages cannot be printed. Install it from Microsoft, then try again.",
            ));
        }
        Ok(plan_for(ToolId::HtmlToPdf, inputs, opts, 1))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::HtmlToPdf(options) = &plan.options else {
            return Err(wrong_options());
        };
        let mut progress = Progress::start(ctx, plan.total_units);
        let (source, naming) = match options.address()? {
            Some(url) => (HtmlSource::Url(url.to_owned()), name_for_address(url)),
            None => {
                let input = first_input(&plan.inputs)?;
                (HtmlSource::File(input.path.clone()), input.clone())
            }
        };
        let out = publish_pdf_checked(
            ctx,
            &self.deps,
            &naming,
            self.label(),
            None,
            None,
            &mut |tmp| self.deps.html.print(&source, options, tmp, ctx.cancel),
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::OfficeEngines;
    use crate::runtime::CancellationToken;
    use std::path::Path;
    use std::sync::Arc;

    struct Engine(bool, &'static str);

    impl OfficeConverter for Engine {
        fn name(&self) -> &'static str {
            self.1
        }
        fn is_available(&self, _: OfficeKind) -> bool {
            self.0
        }
        fn convert(
            &self,
            _: &Path,
            _: &OfficeRequest,
            _: &Path,
            _: &Path,
            _: &CancellationToken,
        ) -> Result<(), AppError> {
            Ok(())
        }
    }

    fn deps(ms: bool, libre: bool) -> ToolDeps {
        ToolDeps {
            office: OfficeEngines {
                microsoft: Arc::new(Engine(ms, "Microsoft Office")),
                libre: Arc::new(Engine(libre, "LibreOffice")),
            },
            ..ToolDeps::unavailable()
        }
    }

    fn doc() -> InputDoc {
        InputDoc::new("C:\\a.docx")
    }

    #[test]
    fn auto_prefers_microsoft_office_then_libreoffice_then_says_what_is_missing() {
        let name = |ms, libre| {
            pick(OfficeEngine::Auto, OfficeKind::Word, &deps(ms, libre)).map(|c| c.name())
        };
        assert_eq!(name(true, true), Ok("Microsoft Office"));
        assert_eq!(name(true, false), Ok("Microsoft Office"));
        assert_eq!(name(false, true), Ok("LibreOffice"));
        let err = name(false, false).unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentMissing);
        assert!(err.message.contains("Components"), "{}", err.message);
    }

    #[test]
    fn a_named_program_must_be_there() {
        let err = pick(OfficeEngine::MsOffice, OfficeKind::Word, &deps(false, true))
            .err()
            .unwrap();
        assert_eq!(err.code, ErrorCode::ComponentMissing);
        assert!(err.message.contains("Microsoft Office"));
        let err = pick(
            OfficeEngine::LibreOffice,
            OfficeKind::Word,
            &deps(true, false),
        )
        .err()
        .unwrap();
        assert!(err.message.contains("LibreOffice"));
        assert!(pick(OfficeEngine::MsOffice, OfficeKind::Word, &deps(true, false)).is_ok());
        assert!(
            pick(
                OfficeEngine::LibreOffice,
                OfficeKind::Word,
                &deps(false, true)
            )
            .is_ok()
        );
    }

    #[test]
    fn validation_reports_a_missing_program_before_any_work() {
        let tool = WordToPdfTool::new(deps(false, false));
        let options = ToolOptions::WordToPdf(WordToPdfOptions::default());
        assert_eq!(
            tool.validate(&[doc()], &options).unwrap_err().code,
            ErrorCode::ComponentMissing
        );
        let ready = WordToPdfTool::new(deps(true, false));
        assert_eq!(ready.validate(&[doc()], &options).unwrap().total_units, 1);
        assert!(ready.validate(&[], &options).is_err());
    }

    #[test]
    fn requests_carry_the_options_of_each_kind() {
        let slides = slides_request(&PowerPointToPdfOptions {
            include_hidden: true,
            ..PowerPointToPdfOptions::default()
        });
        assert!(slides.include_hidden_slides);
        assert_eq!(slides.kind, OfficeKind::Slides);
        let sheet = sheet_request(&ExcelToPdfOptions {
            sheets: PageRange::parse("1,3").unwrap(),
            ..ExcelToPdfOptions::default()
        });
        assert_eq!(sheet.kind, OfficeKind::Sheet);
        assert!(sheet.fit_to_page_width);
        assert_eq!(sheet.sheets, PageRange::parse("1,3").unwrap());
    }

    #[test]
    fn a_web_address_gets_a_file_name_from_its_host() {
        assert_eq!(
            name_for_address("https://www.example.com/a/b?c=d").stem(),
            "www.example.com"
        );
        assert_eq!(
            name_for_address("https://x.org").path,
            PathBuf::from("x.org.html")
        );
        assert_eq!(
            name_for_address("https://a b/").path,
            PathBuf::from("a_b.html")
        );
    }

    #[test]
    fn a_web_address_needs_no_file_but_a_file_run_does() {
        let tool = HtmlToPdfTool::new(ToolDeps::unavailable());
        let url = ToolOptions::HtmlToPdf(HtmlToPdfOptions {
            url: Some("https://example.com".to_owned()),
            allow_network: true,
            ..HtmlToPdfOptions::default()
        });
        assert!(tool.runs_without_files(&url));
        assert!(!tool.runs_without_files(&ToolOptions::HtmlToPdf(HtmlToPdfOptions::default())));
    }

    #[test]
    fn an_address_without_consent_is_refused_with_the_consent_code() {
        let tool = HtmlToPdfTool::new(ToolDeps::unavailable());
        let options = ToolOptions::HtmlToPdf(HtmlToPdfOptions {
            url: Some("https://example.com".to_owned()),
            allow_network: false,
            ..HtmlToPdfOptions::default()
        });
        assert_eq!(
            tool.validate(&[], &options).unwrap_err().code,
            ErrorCode::NetworkConsentRequired
        );
    }
}
