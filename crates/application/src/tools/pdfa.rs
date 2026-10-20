//! PDF to PDF/A (row 30): a copy of the document that follows an archive standard.
//!
//! The document is normalized (fonts embedded, a color profile and an XMP packet added, what the
//! standard forbids removed) and then checked with veraPDF. If the check fails and downgrading is
//! allowed, the next weaker level is tried. As a last resort, when the person agreed to it, the
//! pages are replaced by pictures, which always conforms and loses the text.

use domain::tools::ToolGroup;
use domain::tools::pdfa::{PdfToPdfaOptions, PdfaLevel};
use domain::{ErrorCode, ToolId, ToolOptions};

use super::common::{
    Progress, Scratch, ToolDeps, first_input, page_count, publish_pdf_checked, wrong_options,
};
use super::document::FlattenTool;
use crate::conformance::{ConformanceReport, Profile};
use crate::error::AppError;
use crate::ports::LogLevel;
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

/// Resolution of the pictures the last-resort mode makes.
const PICTURE_DPI: u32 = 200;

/// Converts a document to PDF/A.
pub struct PdfToPdfaTool {
    deps: ToolDeps,
}

impl PdfToPdfaTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn pdfa_options(opts: &ToolOptions) -> Result<&PdfToPdfaOptions, AppError> {
    match opts {
        ToolOptions::PdfToPdfa(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

/// What a failed attempt says, in a few words for the message.
fn summary(report: &ConformanceReport) -> String {
    let mut parts: Vec<String> = report
        .violations
        .iter()
        .take(3)
        .map(|v| format!("{} ({})", v.description, v.clause))
        .collect();
    if report.violations.len() > 3 {
        parts.push(format!("and {} more rules", report.violations.len() - 3));
    }
    parts.join("; ")
}

fn failure(level: PdfaLevel, last: Option<&ConformanceReport>, tried: &[PdfaLevel]) -> AppError {
    let levels = tried
        .iter()
        .map(|l| l.name())
        .collect::<Vec<_>>()
        .join(", ");
    let detail = last.map(summary).unwrap_or_default();
    AppError::new(
        ErrorCode::OutputVerifyFailed,
        format!(
            "The file could not be made to conform to {} (tried {levels}). The checker still finds: {detail}. Turn on \"Replace pages by pictures if needed\" to get a conforming file without selectable text, or fix the file in the program that made it.",
            level.name()
        ),
    )
}

impl PdfToPdfaTool {
    /// Normalizes `unlocked` for `level` and checks the result. Returns the file when it
    /// conforms, otherwise the report that says why not.
    fn attempt<'a>(
        &self,
        ctx: &'a JobContext<'a>,
        unlocked: &std::path::Path,
        level: PdfaLevel,
    ) -> Result<(Scratch<'a>, ConformanceReport), AppError> {
        // The scratch file lives as long as the caller keeps it.
        let candidate = Scratch::new(ctx, "pdf");
        let notes = self.deps.normalizer.normalize(
            unlocked,
            level,
            ctx.clock.unix_now(),
            candidate.path(),
        )?;
        if !notes.is_empty() {
            ctx.logger.log(LogLevel::Info, "pdfa.normalized");
        }
        ctx.cancel.check()?;
        let report =
            self.deps
                .conformance
                .validate(candidate.path(), Profile::PdfA(level), ctx.cancel)?;
        Ok((candidate, report))
    }
}

impl Tool for PdfToPdfaTool {
    fn id(&self) -> ToolId {
        ToolId::PdfToPdfa
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::ConvertFromPdf
    }
    fn label(&self) -> &'static str {
        "PDF-A"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(PdfToPdfaOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        pdfa_options(opts)?;
        Ok(Plan {
            tool: ToolId::PdfToPdfa,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            total_units: 4,
        })
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = pdfa_options(&plan.options)?;
        if !self.deps.conformance.is_available() {
            return Err(AppError::new(
                ErrorCode::ComponentMissing,
                "The PDF/A checker is not installed. Open Settings, then Components, to install veraPDF; every PDF/A file is checked before it is saved.",
            ));
        }
        let mut progress = Progress::start(ctx, plan.total_units);
        // PDF/A files are never encrypted, so work from a copy without the password.
        let unlocked = Scratch::new(ctx, "pdf");
        self.deps
            .security
            .unlock(&input.path, input.password.as_ref(), unlocked.path())?;
        progress.advance(1);

        let mut levels = vec![options.level];
        if options.allow_downgrade {
            levels.extend(options.level.fallbacks());
        }
        let mut last: Option<ConformanceReport> = None;
        let mut reached: Option<(Scratch<'_>, PdfaLevel)> = None;
        for level in &levels {
            ctx.cancel.check()?;
            let (candidate, report) = self.attempt(ctx, unlocked.path(), *level)?;
            if report.compliant {
                reached = Some((candidate, *level));
                break;
            }
            last = Some(report);
        }
        progress.advance(1);

        // The plugin is a second opinion on how to rewrite the file, before pictures.
        if reached.is_none()
            && let Some(fallback) = &self.deps.pdfa_fallback
        {
            for level in &levels {
                ctx.cancel.check()?;
                let candidate = Scratch::new(ctx, "pdf");
                if fallback
                    .normalize(
                        unlocked.path(),
                        *level,
                        ctx.clock.unix_now(),
                        candidate.path(),
                    )
                    .is_err()
                {
                    break;
                }
                let report = self.deps.conformance.validate(
                    candidate.path(),
                    Profile::PdfA(*level),
                    ctx.cancel,
                )?;
                if report.compliant {
                    reached = Some((candidate, *level));
                    break;
                }
                last = Some(report);
            }
        }

        if reached.is_none() && options.rasterize_if_needed {
            let target = options
                .level
                .fallbacks()
                .last()
                .copied()
                .unwrap_or(options.level);
            let target = if options.allow_downgrade {
                target
            } else {
                options.level
            };
            // Pictures carry no text, so level A cannot hold; the weakest of the part does.
            let target = if target.needs_tags() {
                PdfaLevel::A2b
            } else {
                target
            };
            let pictures = Scratch::new(ctx, "pdf");
            let mut sub = Progress::start(ctx, 1);
            FlattenTool::new(self.deps.clone()).flatten_everything(
                ctx,
                &InputDoc {
                    password: None,
                    ..input.clone()
                },
                PICTURE_DPI,
                &mut sub,
                pictures.path(),
            )?;
            let (candidate, report) = self.attempt(ctx, pictures.path(), target)?;
            if report.compliant {
                reached = Some((candidate, target));
            } else {
                last = Some(report);
            }
        }
        progress.advance(1);

        let Some((file, level)) = reached else {
            return Err(failure(options.level, last.as_ref(), &levels));
        };
        let label = format!("PDF-A {}", level.code());
        let out = publish_pdf_checked(
            ctx,
            &self.deps,
            input,
            &label,
            Some(page_count(input)),
            None,
            &mut |tmp| ctx.files.copy_file(file.path(), tmp),
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance::{Example, Violation};

    fn doc() -> InputDoc {
        InputDoc {
            page_count: Some(2),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    #[test]
    fn the_tool_takes_a_pdf_and_plans_four_steps() {
        let tool = PdfToPdfaTool::new(ToolDeps::unavailable());
        let plan = tool
            .validate(
                &[doc()],
                &ToolOptions::PdfToPdfa(PdfToPdfaOptions::default()),
            )
            .unwrap();
        assert_eq!(plan.total_units, 4);
        assert_eq!(tool.group(), ToolGroup::ConvertFromPdf);
        assert!(
            tool.validate(&[doc()], &ToolOptions::ValidatePdfa(Default::default()))
                .is_err()
        );
    }

    fn report(rules: &[(&str, &str)]) -> ConformanceReport {
        ConformanceReport {
            profile: "PDF/A-2b".into(),
            compliant: false,
            passed_rules: 100,
            failed_rules: rules.len() as u32,
            passed_checks: 100,
            failed_checks: rules.len() as u32,
            violations: rules
                .iter()
                .map(|(clause, description)| Violation {
                    specification: "ISO 19005-2:2011".into(),
                    clause: (*clause).into(),
                    test: 1,
                    description: (*description).into(),
                    count: 1,
                    examples: vec![Example {
                        place: "root".into(),
                        message: "x".into(),
                    }],
                })
                .collect(),
            validator: "veraPDF".into(),
        }
    }

    #[test]
    fn the_failure_names_the_rules_and_the_way_out() {
        let r = report(&[
            ("6.2.4.3", "DeviceCMYK"),
            ("6.2.11.4.1", "Fonts"),
            ("6.1.3", "ID"),
            ("6.6.2.1", "XMP"),
        ]);
        let err = failure(PdfaLevel::A2b, Some(&r), &[PdfaLevel::A2b]);
        assert_eq!(err.code, ErrorCode::OutputVerifyFailed);
        assert!(
            err.message.contains("DeviceCMYK (6.2.4.3)"),
            "{}",
            err.message
        );
        assert!(err.message.contains("and 1 more rules"), "{}", err.message);
        assert!(
            err.message.contains("Replace pages by pictures"),
            "{}",
            err.message
        );
        assert!(err.message.contains("PDF/A-2b"));
    }

    #[test]
    fn the_failure_lists_every_level_tried() {
        let err = failure(
            PdfaLevel::A3a,
            None,
            &[PdfaLevel::A3a, PdfaLevel::A3u, PdfaLevel::A3b],
        );
        assert!(
            err.message.contains("PDF/A-3a, PDF/A-3u, PDF/A-3b"),
            "{}",
            err.message
        );
    }
}
