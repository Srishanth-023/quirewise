//! The checks of standards: Validate PDF/A (row 31) and the PDF/UA accessibility check (row 70).
//!
//! The file is never changed. The result is a report, as a web page or as data. The checker is
//! veraPDF, an on-demand component.

use domain::tools::ToolGroup;
use domain::tools::pdfa::{CheckPdfuaOptions, ReportFormat, ValidatePdfaOptions};
use domain::{ErrorCode, ToolId, ToolOptions};

use super::common::{Progress, ToolDeps, first_input, publish_file, wrong_options};
use super::conformreport::{Subject, render};
use crate::conformance::Profile;
use crate::error::AppError;
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

fn plan_for(tool: ToolId, inputs: &[InputDoc], opts: &ToolOptions) -> Plan {
    Plan {
        tool,
        inputs: inputs.to_vec(),
        options: opts.clone(),
        total_units: 2,
    }
}

/// Checks the file and writes the report.
fn check(
    ctx: &JobContext<'_>,
    deps: &ToolDeps,
    plan: &Plan,
    label: &str,
    profile: Profile,
    format: ReportFormat,
    purpose: &str,
) -> Result<Vec<OutputArtifact>, AppError> {
    let input = first_input(&plan.inputs)?;
    if !deps.conformance.is_available() {
        return Err(AppError::new(
            ErrorCode::ComponentMissing,
            "The PDF/A checker is not installed. Open Settings, then Components, to install veraPDF.",
        ));
    }
    let mut progress = Progress::start(ctx, plan.total_units);
    let report = deps
        .conformance
        .validate(&input.path, profile, ctx.cancel)?;
    progress.advance(1);
    let name = input.path.file_name().map_or_else(
        || "document".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    let now = ctx.clock.now();
    let checked = format!(
        "{:04}-{:02}-{:02} {:02}:{:02} (local time)",
        now.year, now.month, now.day, now.hour, now.minute
    );
    let text = render(
        format,
        &Subject {
            file_name: &name,
            checked: &checked,
            purpose,
        },
        &report,
    );
    let dest = ctx.output_path(input, label, format.extension(), |_| {})?;
    let out = publish_file(
        ctx,
        &dest,
        &mut |tmp| {
            std::fs::write(tmp, text.as_bytes())
                .map_err(|e| AppError::from_io("write the report", &e))
        },
        &|tmp| {
            let len = ctx.files.len(tmp)?;
            if len == 0 {
                Err(AppError::new(
                    ErrorCode::OutputVerifyFailed,
                    "The report came out empty, so it was not saved.",
                ))
            } else {
                Ok(())
            }
        },
    )?;
    progress.advance(1);
    Ok(vec![out])
}

/// Checks a file against PDF/A.
pub struct ValidatePdfaTool {
    deps: ToolDeps,
}

impl ValidatePdfaTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn pdfa_options(opts: &ToolOptions) -> Result<&ValidatePdfaOptions, AppError> {
    match opts {
        ToolOptions::ValidatePdfa(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for ValidatePdfaTool {
    fn id(&self) -> ToolId {
        ToolId::ValidatePdfa
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Validation
    }
    fn label(&self) -> &'static str {
        "PDF-A report"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(ValidatePdfaOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        pdfa_options(opts)?;
        Ok(plan_for(ToolId::ValidatePdfa, inputs, opts))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let options = pdfa_options(&plan.options)?;
        let profile = options.level.map_or(Profile::Auto, Profile::PdfA);
        check(
            ctx,
            &self.deps,
            &plan,
            self.label(),
            profile,
            options.format,
            "long-term archiving",
        )
    }
}

/// Checks a file against PDF/UA, the accessibility standard.
pub struct CheckPdfuaTool {
    deps: ToolDeps,
}

impl CheckPdfuaTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn ua_options(opts: &ToolOptions) -> Result<&CheckPdfuaOptions, AppError> {
    match opts {
        ToolOptions::CheckPdfua(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for CheckPdfuaTool {
    fn id(&self) -> ToolId {
        ToolId::CheckPdfua
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Validation
    }
    fn label(&self) -> &'static str {
        "accessibility report"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(CheckPdfuaOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        ua_options(opts)?;
        Ok(plan_for(ToolId::CheckPdfua, inputs, opts))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let options = ua_options(&plan.options)?;
        check(
            ctx,
            &self.deps,
            &plan,
            self.label(),
            Profile::PdfUa1,
            options.format,
            "accessibility for people who use screen readers",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> InputDoc {
        InputDoc {
            page_count: Some(1),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    #[test]
    fn both_tools_are_validation_tools_with_their_own_labels() {
        let deps = ToolDeps::unavailable();
        let a = ValidatePdfaTool::new(deps.clone());
        let b = CheckPdfuaTool::new(deps);
        assert_eq!(a.group(), ToolGroup::Validation);
        assert_eq!(b.group(), ToolGroup::Validation);
        assert_ne!(a.label(), b.label());
    }

    #[test]
    fn validation_plans_two_units_for_each_file() {
        let a = ValidatePdfaTool::new(ToolDeps::unavailable());
        let plan = a
            .validate(
                &[doc()],
                &ToolOptions::ValidatePdfa(ValidatePdfaOptions::default()),
            )
            .unwrap();
        assert_eq!(plan.total_units, 2);
        let wrong = ToolOptions::CheckPdfua(CheckPdfuaOptions::default());
        assert!(a.validate(&[doc()], &wrong).is_err());
    }
}
