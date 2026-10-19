//! The Security tools: Protect (row 51), Unlock (row 52) and Sanitize (row 63).

use domain::tools::ToolGroup;
use domain::tools::secure::{ProtectOptions, SanitizeOptions, UnlockOptions};
use domain::{ToolId, ToolOptions};

use super::common::{
    Progress, ToolDeps, first_input, page_count, publish_edit, publish_pdf, publish_pdf_checked,
    wrong_options,
};
use crate::error::AppError;
use crate::ports::PdfEdit;
use crate::runtime::Password;
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

fn plan_for(tool: ToolId, inputs: &[InputDoc], opts: &ToolOptions) -> Plan {
    Plan {
        tool,
        inputs: inputs.to_vec(),
        options: opts.clone(),
        total_units: 1,
    }
}

/// Encrypts a file with an open password, an owner password and permissions.
pub struct ProtectTool {
    deps: ToolDeps,
}

impl ProtectTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn protect_options(opts: &ToolOptions) -> Result<&ProtectOptions, AppError> {
    match opts {
        ToolOptions::Protect(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for ProtectTool {
    fn id(&self) -> ToolId {
        ToolId::Protect
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Security
    }
    fn label(&self) -> &'static str {
        "protected"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(ProtectOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        protect_options(opts)?.plan()?;
        Ok(plan_for(ToolId::Protect, inputs, opts))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let protection = protect_options(&plan.options)?.plan()?;
        let mut progress = Progress::start(ctx, plan.total_units);
        // The result is checked with its own password, so the check proves the password works.
        let open_with =
            (!protection.user.is_empty()).then(|| Password::new(protection.user.expose()));
        let out = publish_pdf_checked(
            ctx,
            &self.deps,
            input,
            self.label(),
            Some(page_count(input)),
            open_with.as_ref(),
            &mut |tmp| {
                self.deps
                    .security
                    .protect(&input.path, input.password.as_ref(), &protection, tmp)
            },
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

/// Writes an unencrypted copy of a protected file.
pub struct UnlockTool {
    deps: ToolDeps,
}

impl UnlockTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn unlock_options(opts: &ToolOptions) -> Result<&UnlockOptions, AppError> {
    match opts {
        ToolOptions::Unlock(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for UnlockTool {
    fn id(&self) -> ToolId {
        ToolId::Unlock
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Security
    }
    fn label(&self) -> &'static str {
        "unlocked"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(UnlockOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        unlock_options(opts)?;
        Ok(plan_for(ToolId::Unlock, inputs, opts))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_pdf(
            ctx,
            &self.deps,
            input,
            self.label(),
            page_count(input),
            &mut |tmp| {
                self.deps
                    .security
                    .unlock(&input.path, input.password.as_ref(), tmp)
            },
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

/// Removes metadata, scripts, attachments, hidden layers and other hidden content.
pub struct SanitizeTool {
    deps: ToolDeps,
}

impl SanitizeTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn sanitize_options(opts: &ToolOptions) -> Result<&SanitizeOptions, AppError> {
    match opts {
        ToolOptions::Sanitize(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for SanitizeTool {
    fn id(&self) -> ToolId {
        ToolId::Sanitize
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Security
    }
    fn label(&self) -> &'static str {
        "sanitized"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(SanitizeOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        sanitize_options(opts)?.validate()?;
        Ok(plan_for(ToolId::Sanitize, inputs, opts))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = *sanitize_options(&plan.options)?;
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_edit(
            ctx,
            &self.deps,
            input,
            self.label(),
            page_count(input),
            &PdfEdit::Sanitize(options),
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::ErrorCode;
    use domain::secret::Secret;
    use domain::tools::secure::Permissions;

    fn doc() -> InputDoc {
        InputDoc {
            page_count: Some(2),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    #[test]
    fn protect_checks_the_passwords_before_any_work() {
        let tool = ProtectTool::new(ToolDeps::unavailable());
        let none = ToolOptions::defaults_for(ToolId::Protect);
        assert_eq!(
            tool.validate(&[doc()], &none).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
        let ok = ToolOptions::Protect(ProtectOptions {
            user_password: Some(Secret::new("open")),
            ..ProtectOptions::default()
        });
        assert!(tool.validate(&[doc()], &ok).is_ok());
        let restricted = ToolOptions::Protect(ProtectOptions {
            user_password: Some(Secret::new("open")),
            permissions: Permissions {
                print: false,
                ..Permissions::default()
            },
            ..ProtectOptions::default()
        });
        assert_eq!(
            tool.validate(&[doc()], &restricted).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
    }

    #[test]
    fn sanitize_needs_at_least_one_removal() {
        let tool = SanitizeTool::new(ToolDeps::unavailable());
        let nothing = ToolOptions::Sanitize(SanitizeOptions {
            metadata: false,
            xmp: false,
            javascript: false,
            embedded_files: false,
            hidden_layers: false,
            form_actions: false,
            thumbnails: false,
            private_data: false,
        });
        assert!(tool.validate(&[doc()], &nothing).is_err());
        assert!(
            tool.validate(&[doc()], &ToolOptions::defaults_for(ToolId::Sanitize))
                .is_ok()
        );
    }

    #[test]
    fn tools_describe_themselves() {
        let deps = ToolDeps::unavailable();
        for tool in [
            &ProtectTool::new(deps.clone()) as &dyn Tool,
            &UnlockTool::new(deps.clone()),
            &SanitizeTool::new(deps),
        ] {
            assert_eq!(tool.group(), ToolGroup::Security);
            assert!(tool.options_schema().as_value().is_object());
        }
    }
}
