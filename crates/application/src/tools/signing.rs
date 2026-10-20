//! The digital signature tools: Digital signature (row 61) and Verify signatures (row 62).
//!
//! Signing appends a revision to a copy of the file, so the original bytes and any earlier
//! signature stay as they were. The result is checked before it is published: it must open, pass
//! `qpdf --check`, and its new signature must validate.

use domain::tools::ToolGroup;
use domain::tools::signing::{DigitalSignOptions, KeySource, VerifySignaturesOptions};
use domain::{DomainError, ToolId, ToolOptions};

use super::common::{
    Progress, ToolDeps, first_input, page_count, publish_pdf_checked, wrong_options,
};
use super::reportkit::PAGE;
use super::signreport::{Subject, layout};
use crate::error::AppError;
use crate::ports::{AssemblyPlan, LogLevel, PdfEdit, PlannedPage};
use crate::signing::{KeyChoice, SignRequest, SignatureBox, Status, VerifyRequest, WebClient};
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};
use domain::ErrorCode;

fn plan_for(tool: ToolId, inputs: &[InputDoc], opts: &ToolOptions, units: u64) -> Plan {
    Plan {
        tool,
        inputs: inputs.to_vec(),
        options: opts.clone(),
        total_units: units,
    }
}

/// Signs a file with a certificate.
pub struct DigitalSignTool {
    deps: ToolDeps,
}

impl DigitalSignTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn sign_options(opts: &ToolOptions) -> Result<&DigitalSignOptions, AppError> {
    match opts {
        ToolOptions::DigitalSign(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

impl Tool for DigitalSignTool {
    fn id(&self) -> ToolId {
        ToolId::DigitalSign
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Security
    }
    fn label(&self) -> &'static str {
        "digitally signed"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(DigitalSignOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let input = first_input(inputs)?;
        let options = sign_options(opts)?;
        options.validate()?;
        let pages = page_count(input);
        if options.visible && options.page > pages {
            return Err(DomainError::RangeOutOfBounds {
                detail: format!("The signature is placed on page {}.", options.page),
                valid: format!("1-{pages}"),
            }
            .into());
        }
        Ok(plan_for(ToolId::DigitalSign, inputs, opts, 2))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = sign_options(&plan.options)?;
        let key = match options.source {
            KeySource::Pfx => {
                let path = options
                    .pfx_path
                    .as_deref()
                    .ok_or_else(|| AppError::internal("reading the certificate file name"))?;
                KeyChoice::Pfx {
                    path: ctx.files.canonicalize(path)?,
                    password: options.pfx_password.clone().unwrap_or_default(),
                }
            }
            KeySource::WindowsStore => KeyChoice::Store {
                thumbprint: options.thumbprint_hex()?,
            },
        };
        let request = SignRequest {
            key,
            field: options.field.clone(),
            visible: options.visible.then(|| SignatureBox {
                page: options.page.saturating_sub(1),
                left: options.left,
                top: options.top,
                width: options.width,
                height: options.height,
            }),
            signer_name: options.signer_name.clone(),
            reason: options.reason.clone(),
            location: options.location.clone(),
            contact: options.contact.clone(),
            level: options.level,
            timestamp_url: options.timestamp_url.clone(),
            certify: options.certify,
            signed_at: ctx.clock.unix_now(),
        };
        let web: Option<&dyn WebClient> = options.allow_network.then_some(self.deps.web.as_ref());
        if web.is_some() {
            // Only the kind of connection is logged, never the address or the file.
            ctx.logger.log(
                LogLevel::Info,
                if options.level.long_term() {
                    "net.timestamp-and-revocation"
                } else {
                    "net.timestamp"
                },
            );
        }
        let mut progress = Progress::start(ctx, plan.total_units);
        let out = publish_pdf_checked(
            ctx,
            &self.deps,
            input,
            self.label(),
            Some(page_count(input)),
            None,
            &mut |tmp| {
                let done = self
                    .deps
                    .signer
                    .sign(&input.path, tmp, &request, web, ctx.cancel)?;
                if !done.warnings.is_empty() {
                    ctx.logger
                        .log(LogLevel::Warn, "sign.validation-data-incomplete");
                }
                progress.advance(1);
                // The new signature must hold before the file is published.
                let reports = self.deps.signatures.check(
                    tmp,
                    &VerifyRequest {
                        check_revocation: false,
                        trusted_certificates: None,
                        now: request.signed_at,
                    },
                    None,
                    ctx.cancel,
                )?;
                let signed = reports.iter().find(|r| r.field == done.field);
                match signed {
                    Some(r) if r.signature.status == Status::Pass => Ok(()),
                    _ => Err(AppError::new(
                        ErrorCode::OutputVerifyFailed,
                        "The new signature did not validate, so no file was written. Try again; if it keeps happening, sign with another certificate.",
                    )),
                }
            },
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

/// Checks the signatures of a file and writes a report.
pub struct VerifySignaturesTool {
    deps: ToolDeps,
}

impl VerifySignaturesTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }
}

fn verify_options(opts: &ToolOptions) -> Result<&VerifySignaturesOptions, AppError> {
    match opts {
        ToolOptions::VerifySignatures(o) => Ok(o),
        _ => Err(wrong_options()),
    }
}

/// `YYYY-MM-DD HH:MM` of a local time.
fn local_time(now: domain::naming::DateTimeParts) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} (local time)",
        now.year, now.month, now.day, now.hour, now.minute
    )
}

impl Tool for VerifySignaturesTool {
    fn id(&self) -> ToolId {
        ToolId::VerifySignatures
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Security
    }
    fn label(&self) -> &'static str {
        "signature report"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(VerifySignaturesOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        first_input(inputs)?;
        verify_options(opts)?.validate()?;
        Ok(plan_for(ToolId::VerifySignatures, inputs, opts, 3))
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let input = first_input(&plan.inputs)?;
        let options = verify_options(&plan.options)?;
        let trusted = options
            .trusted_certificates
            .as_deref()
            .map(|p| ctx.files.canonicalize(p))
            .transpose()?;
        let web: Option<&dyn WebClient> =
            (options.check_revocation && options.allow_network).then_some(self.deps.web.as_ref());
        if web.is_some() {
            ctx.logger.log(LogLevel::Info, "net.revocation");
        }
        let mut progress = Progress::start(ctx, plan.total_units);
        let reports = self.deps.signatures.check(
            &input.path,
            &VerifyRequest {
                check_revocation: web.is_some(),
                trusted_certificates: trusted,
                now: ctx.clock.unix_now(),
            },
            web,
            ctx.cancel,
        )?;
        progress.advance(1);
        let name = input.path.file_name().map_or_else(
            || "document".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        let checked = local_time(ctx.clock.now());
        let report = layout(
            &Subject {
                file_name: &name,
                checked: &checked,
                asked_revocation: web.is_some(),
            },
            &reports,
        );
        progress.advance(1);
        let out = publish_pdf_checked(
            ctx,
            &self.deps,
            input,
            self.label(),
            Some(report.pages),
            None,
            &mut |tmp| {
                let blank = super::common::Scratch::new(ctx, "pdf");
                let sheets = AssemblyPlan {
                    sources: Vec::new(),
                    pages: (0..report.pages)
                        .map(|_| PlannedPage::Blank {
                            width_pt: PAGE.0,
                            height_pt: PAGE.1,
                        })
                        .collect(),
                };
                self.deps
                    .writer
                    .assemble(&sheets, ctx.temp_dir, blank.path())?;
                ctx.cancel.check()?;
                self.deps
                    .editor
                    .apply(blank.path(), &PdfEdit::Overlay(report.items.clone()), tmp)
            },
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::tools::signing::SignLevel;
    use std::path::PathBuf;

    fn doc(pages: u32) -> InputDoc {
        InputDoc {
            page_count: Some(pages),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    fn with_pfx() -> DigitalSignOptions {
        DigitalSignOptions {
            pfx_path: Some(PathBuf::from("C:\\me.pfx")),
            ..DigitalSignOptions::default()
        }
    }

    #[test]
    fn signing_needs_a_certificate() {
        let tool = DigitalSignTool::new(ToolDeps::unavailable());
        let none = ToolOptions::DigitalSign(DigitalSignOptions::default());
        assert_eq!(
            tool.validate(&[doc(2)], &none).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
        let ok = ToolOptions::DigitalSign(with_pfx());
        assert_eq!(tool.validate(&[doc(2)], &ok).unwrap().total_units, 2);
    }

    #[test]
    fn the_page_of_a_visible_signature_must_exist() {
        let tool = DigitalSignTool::new(ToolDeps::unavailable());
        let mut o = with_pfx();
        o.visible = true;
        o.page = 5;
        let err = tool
            .validate(&[doc(2)], &ToolOptions::DigitalSign(o))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::RangeOutOfBounds);
    }

    #[test]
    fn the_internet_needs_consent_before_anything_runs() {
        let tool = DigitalSignTool::new(ToolDeps::unavailable());
        let mut o = with_pfx();
        o.level = SignLevel::BT;
        o.timestamp_url = Some("https://tsa.example/ts".into());
        let err = tool
            .validate(&[doc(2)], &ToolOptions::DigitalSign(o.clone()))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NetworkConsentRequired);
        o.allow_network = true;
        assert!(
            tool.validate(&[doc(2)], &ToolOptions::DigitalSign(o))
                .is_ok()
        );
    }

    #[test]
    fn a_revocation_check_needs_consent_too() {
        let tool = VerifySignaturesTool::new(ToolDeps::unavailable());
        let mut o = VerifySignaturesOptions {
            check_revocation: true,
            ..VerifySignaturesOptions::default()
        };
        let err = tool
            .validate(&[doc(1)], &ToolOptions::VerifySignatures(o.clone()))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NetworkConsentRequired);
        o.allow_network = true;
        assert!(
            tool.validate(&[doc(1)], &ToolOptions::VerifySignatures(o))
                .is_ok()
        );
    }

    #[test]
    fn the_tools_are_in_the_security_group_with_distinct_labels() {
        let deps = ToolDeps::unavailable();
        let a = DigitalSignTool::new(deps.clone());
        let b = VerifySignaturesTool::new(deps);
        assert_eq!(a.group(), ToolGroup::Security);
        assert_eq!(b.group(), ToolGroup::Security);
        assert_ne!(a.label(), b.label());
    }
}
