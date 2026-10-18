//! The Optimize tools: Compress (row 14), Repair (row 15) and Linearize (row 64).

use domain::tools::ToolGroup;
use domain::tools::optimize::{CompressOptions, LinearizeOptions, RepairOptions};
use domain::tools::secure::SanitizeOptions;
use domain::{ErrorCode, ToolId, ToolOptions};

use super::common::{
    Progress, Scratch, ToolDeps, first_input, page_count, publish_pdf, publish_pdf_checked,
    wrong_options,
};
use crate::error::AppError;
use crate::ports::{LogLevel, PackageOptions, PdfEdit};
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

fn plan_for(tool: ToolId, inputs: &[InputDoc], opts: &ToolOptions, units: u64) -> Plan {
    Plan {
        tool,
        inputs: inputs.to_vec(),
        options: opts.clone(),
        total_units: units,
    }
}

/// Shrinks a file by downsampling and recompressing pictures and packing the structure.
pub struct CompressTool {
    deps: ToolDeps,
}

impl CompressTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn compress_options(opts: &ToolOptions) -> Result<&CompressOptions, AppError> {
    match opts {
        ToolOptions::Compress(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for CompressTool {
    fn id(&self) -> ToolId {
        ToolId::Compress
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Optimize
    }
    fn label(&self) -> &'static str {
        "compressed"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(CompressOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        compress_options(opts)?.settings()?;
        // Unlock, pictures, metadata and packing.
        Ok(plan_for(ToolId::Compress, inputs, opts, 4))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let opts = compress_options(&plan.options)?;
        let settings = opts.settings()?;
        let count = page_count(input);
        let mut progress = Progress::start(ctx, plan.total_units);

        let unlocked = Scratch::new(ctx, "pdf");
        self.deps
            .security
            .unlock(&input.path, input.password.as_ref(), unlocked.path())?;
        progress.advance(1);
        ctx.cancel.check()?;

        let optimized = Scratch::new(ctx, "pdf");
        let report =
            self.deps
                .optimizer
                .optimize_images(unlocked.path(), &settings, optimized.path())?;
        ctx.logger.log(
            LogLevel::Info,
            &format!(
                "compress.images found={} replaced={}",
                report.found, report.replaced
            ),
        );
        progress.advance(1);
        ctx.cancel.check()?;

        let stripped = Scratch::new(ctx, "pdf");
        let source = if opts.strip_metadata {
            let only_metadata = SanitizeOptions {
                metadata: true,
                xmp: true,
                javascript: false,
                embedded_files: false,
                hidden_layers: false,
                form_actions: false,
                thumbnails: false,
                private_data: false,
            };
            self.deps.editor.apply(
                optimized.path(),
                &PdfEdit::Sanitize(only_metadata),
                stripped.path(),
            )?;
            stripped.path()
        } else {
            optimized.path()
        };
        progress.advance(1);

        let packaging = PackageOptions {
            linearize: false,
            object_streams: opts.object_streams,
        };
        let out = publish_pdf(ctx, &self.deps, input, self.label(), count, &mut |tmp| {
            self.deps.packager.package(source, None, &packaging, tmp)
        })?;
        progress.advance(1);
        Ok(vec![out])
    }
}

/// Rewrites a damaged file into a sound one.
pub struct RepairTool {
    deps: ToolDeps,
}

impl RepairTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn repair_options(opts: &ToolOptions) -> Result<&RepairOptions, AppError> {
    match opts {
        ToolOptions::Repair(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

fn unrepairable() -> AppError {
    AppError::new(
        ErrorCode::PdfDamaged,
        "This file could not be repaired. Too little of it is left to rebuild a document.",
    )
}

impl Tool for RepairTool {
    fn id(&self) -> ToolId {
        ToolId::Repair
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Optimize
    }
    fn label(&self) -> &'static str {
        "repaired"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(RepairOptions)
    }

    /// The file may not open at all, so the runner must not try before the tool does.
    fn reads_pages_first(&self) -> bool {
        false
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        repair_options(opts)?;
        Ok(plan_for(ToolId::Repair, inputs, opts, 1))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let mut last_error = unrepairable();

        // Gentlest first. A result only counts when it opens again and has pages.
        let mut out = publish_pdf_checked(
            ctx,
            &self.deps,
            input,
            self.label(),
            None,
            None,
            &mut |tmp| {
                for (n, repairer) in self.deps.repairers.iter().enumerate() {
                    ctx.cancel.check()?;
                    let attempt = repairer
                        .repair(&input.path, input.password.as_ref(), tmp)
                        .and_then(|_| self.deps.verifier.verify(tmp, None, None))
                        .and_then(|()| self.deps.reader.open_info(tmp, None))
                        .and_then(|info| {
                            if info.page_count == 0 {
                                Err(unrepairable())
                            } else {
                                Ok(())
                            }
                        });
                    match attempt {
                        Ok(()) => {
                            let event = if n == 0 {
                                "repair.first"
                            } else {
                                "repair.fallback"
                            };
                            ctx.logger.log(LogLevel::Info, event);
                            return Ok(());
                        }
                        Err(error) => {
                            if error.code == ErrorCode::JobCancelled {
                                return Err(error);
                            }
                            ctx.logger.log(
                                LogLevel::Warn,
                                &format!("repair.{}.failed", repairer.name()),
                            );
                            ctx.files.remove_file(tmp);
                            last_error = if error.code == ErrorCode::PdfWrongPassword
                                || error.code == ErrorCode::PdfEncrypted
                            {
                                error
                            } else {
                                unrepairable()
                            };
                        }
                    }
                }
                Err(std::mem::replace(&mut last_error, unrepairable()))
            },
        )?;
        // The page count is only known now.
        out.pages = Some(self.deps.reader.open_info(&out.path, None)?.page_count);
        progress.advance(1);
        Ok(vec![out])
    }
}

/// Writes a file for fast web view, or an ordinary copy.
pub struct LinearizeTool {
    deps: ToolDeps,
}

impl LinearizeTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn linearize_options(opts: &ToolOptions) -> Result<&LinearizeOptions, AppError> {
    match opts {
        ToolOptions::Linearize(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for LinearizeTool {
    fn id(&self) -> ToolId {
        ToolId::Linearize
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Optimize
    }
    fn label(&self) -> &'static str {
        "fast web view"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(LinearizeOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        linearize_options(opts)?;
        Ok(plan_for(ToolId::Linearize, inputs, opts, 1))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let opts = linearize_options(&plan.options)?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let packaging = PackageOptions {
            linearize: opts.enabled,
            object_streams: true,
        };
        let out = publish_pdf(
            ctx,
            &self.deps,
            input,
            self.label(),
            page_count(input),
            &mut |tmp| {
                self.deps
                    .packager
                    .package(&input.path, input.password.as_ref(), &packaging, tmp)
            },
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::tools::optimize::CompressOptions;

    fn doc() -> InputDoc {
        InputDoc {
            page_count: Some(3),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    #[test]
    fn compress_rejects_a_bad_quality_before_any_work() {
        let tool = CompressTool::new(ToolDeps::unavailable());
        let bad = ToolOptions::Compress(CompressOptions {
            jpeg_quality: Some(0),
            ..CompressOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc()], &bad).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
        let ok = ToolOptions::defaults_for(ToolId::Compress);
        assert!(tool.validate(&[doc()], &ok).is_ok());
    }

    #[test]
    fn repair_is_the_tool_that_does_not_open_the_file_first() {
        let repair = RepairTool::new(ToolDeps::unavailable());
        assert!(!repair.reads_pages_first());
        assert!(CompressTool::new(ToolDeps::unavailable()).reads_pages_first());
        assert!(LinearizeTool::new(ToolDeps::unavailable()).reads_pages_first());
    }

    #[test]
    fn tools_describe_themselves() {
        let deps = ToolDeps::unavailable();
        for tool in [
            &CompressTool::new(deps.clone()) as &dyn Tool,
            &RepairTool::new(deps.clone()),
            &LinearizeTool::new(deps),
        ] {
            assert_eq!(tool.group(), ToolGroup::Optimize);
            assert_eq!(tool.accepts(), &[InputKind::Pdf]);
            assert!(tool.options_schema().as_value().is_object());
        }
    }
}
