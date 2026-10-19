//! The tools behind the editor: Edit PDF (row 42), Annotate (row 43), Fill and sign (row 45),
//! Create forms (row 46) and Sign PDF (row 53). Each one is a single edit of the document, built
//! from a list the editor drew on a preview of the page.

use domain::tools::ToolGroup;
use domain::tools::editor::{SignatureItem, SignatureSource};
use domain::{ToolId, ToolOptions};

use super::common::{Progress, ToolDeps, first_input, page_count, publish_edit, wrong_options};
use crate::error::AppError;
use crate::ports::PdfEdit;
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

fn plan_for(tool: ToolId, inputs: &[InputDoc], opts: &ToolOptions) -> Plan {
    Plan {
        tool,
        inputs: inputs.to_vec(),
        options: opts.clone(),
        total_units: 1,
    }
}

/// Writes out the dates of the signatures as typed text, in today's date.
fn with_dates(ctx: &JobContext<'_>, items: &[SignatureItem]) -> Vec<SignatureItem> {
    let now = ctx.clock.now();
    items
        .iter()
        .map(|item| match &item.source {
            SignatureSource::Date { format, style } => SignatureItem {
                source: SignatureSource::Typed {
                    text: format.write(
                        i32::from(now.year),
                        u32::from(now.month),
                        u32::from(now.day),
                    ),
                    style: style.clone(),
                },
                ..item.clone()
            },
            _ => item.clone(),
        })
        .collect()
}

/// Runs one edit on the first input and publishes the result.
fn run(
    ctx: &JobContext<'_>,
    deps: &ToolDeps,
    plan: &Plan,
    label: &str,
    edit: &PdfEdit,
) -> Result<Vec<OutputArtifact>, AppError> {
    let input = first_input(&plan.inputs)?;
    let mut progress = Progress::start(ctx, plan.total_units);
    let out = publish_edit(ctx, deps, input, label, page_count(input), edit)?;
    progress.advance(1);
    Ok(vec![out])
}

macro_rules! editor_tool {
    ($(#[$doc:meta])* $name:ident, $id:ident, $label:literal, $group:ident, $options:ident, $schema:ty) => {
        $(#[$doc])*
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
                ToolId::$id
            }
            fn group(&self) -> ToolGroup {
                ToolGroup::$group
            }
            fn label(&self) -> &'static str {
                $label
            }
            fn accepts(&self) -> &'static [InputKind] {
                &[InputKind::Pdf]
            }
            fn options_schema(&self) -> schemars::Schema {
                schemars::schema_for!($schema)
            }

            fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
                let ToolOptions::$options(options) = opts else {
                    return Err(wrong_options());
                };
                let input = first_input(inputs)?;
                options.validate(page_count(input))?;
                Ok(plan_for(ToolId::$id, inputs, opts))
            }

            fn execute(
                &self,
                ctx: &JobContext<'_>,
                plan: Plan,
            ) -> Result<Vec<OutputArtifact>, AppError> {
                let ToolOptions::$options(options) = &plan.options else {
                    return Err(wrong_options());
                };
                let edit = self.edit(ctx, options);
                run(ctx, &self.deps, &plan, self.label(), &edit)
            }
        }
    };
}

editor_tool!(
    /// Adds text, pictures, shapes, links and patches to pages.
    EditPdfTool,
    EditPdf,
    "edited",
    Edit,
    EditPdf,
    domain::tools::editor::EditPdfOptions
);

impl EditPdfTool {
    fn edit(&self, _: &JobContext<'_>, options: &domain::tools::editor::EditPdfOptions) -> PdfEdit {
        PdfEdit::Overlay(options.items.clone())
    }
}

editor_tool!(
    /// Adds highlights, notes and drawings.
    AnnotateTool,
    Annotate,
    "annotated",
    Edit,
    Annotate,
    domain::tools::editor::AnnotateOptions
);

impl AnnotateTool {
    fn edit(
        &self,
        _: &JobContext<'_>,
        options: &domain::tools::editor::AnnotateOptions,
    ) -> PdfEdit {
        PdfEdit::Annotate(options.annotations.clone())
    }
}

editor_tool!(
    /// Places visual signatures, initials, stamps and dates.
    SignPdfTool,
    SignPdf,
    "signed",
    Security,
    SignPdf,
    domain::tools::editor::SignPdfOptions
);

impl SignPdfTool {
    fn edit(
        &self,
        ctx: &JobContext<'_>,
        options: &domain::tools::editor::SignPdfOptions,
    ) -> PdfEdit {
        PdfEdit::Sign(with_dates(ctx, &options.signatures))
    }
}

editor_tool!(
    /// Fills in the form of a document and places signatures.
    FillSignTool,
    FillSign,
    "filled",
    Forms,
    FillSign,
    domain::tools::editor::FillSignOptions
);

impl FillSignTool {
    fn edit(
        &self,
        ctx: &JobContext<'_>,
        options: &domain::tools::editor::FillSignOptions,
    ) -> PdfEdit {
        let mut steps = Vec::new();
        if !options.values.is_empty() {
            steps.push(PdfEdit::FillForm(options.values.clone()));
        }
        if !options.signatures.is_empty() {
            steps.push(PdfEdit::Sign(with_dates(ctx, &options.signatures)));
        }
        if steps.len() == 1 {
            steps.remove(0)
        } else {
            PdfEdit::Sequence(steps)
        }
    }
}

editor_tool!(
    /// Adds form fields.
    CreateFormsTool,
    CreateForms,
    "form",
    Forms,
    CreateForms,
    domain::tools::editor::CreateFormsOptions
);

impl CreateFormsTool {
    fn edit(
        &self,
        _: &JobContext<'_>,
        options: &domain::tools::editor::CreateFormsOptions,
    ) -> PdfEdit {
        PdfEdit::CreateFields(options.fields.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::ErrorCode;
    use domain::tools::common::Region;
    use domain::tools::editor::{
        AnnotateOptions, Annotation, CreateFormsOptions, EditItem, EditPdfOptions, FieldValue,
        FillSignOptions, MarkupAnnotation, SignPdfOptions, TextAlign, TextItem, TextStyle,
        WhiteoutItem,
    };

    fn doc(pages: u32) -> InputDoc {
        InputDoc {
            page_count: Some(pages),
            ..InputDoc::new("C:\\a.pdf")
        }
    }

    fn region() -> Region {
        Region {
            x: 0.1,
            y: 0.1,
            width: 0.3,
            height: 0.1,
        }
    }

    #[test]
    fn items_are_checked_against_the_page_count_before_any_work() {
        let tool = EditPdfTool::new(ToolDeps::unavailable());
        let item = |page| {
            EditItem::Text(TextItem {
                page,
                region: region(),
                text: "Hi".to_owned(),
                style: TextStyle::default(),
                align: TextAlign::Left,
            })
        };
        let ok = ToolOptions::EditPdf(EditPdfOptions {
            items: vec![item(2)],
        });
        assert_eq!(tool.validate(&[doc(3)], &ok).unwrap().total_units, 1);
        assert_eq!(
            tool.validate(&[doc(1)], &ok).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
        let empty = ToolOptions::EditPdf(EditPdfOptions::default());
        assert_eq!(
            tool.validate(&[doc(3)], &empty).unwrap_err().code,
            ErrorCode::OptionInvalid
        );
    }

    #[test]
    fn every_tool_refuses_another_tools_options_and_empty_lists() {
        let deps = ToolDeps::unavailable();
        let wrong = ToolOptions::Flip(domain::tools::FlipOptions::default());
        assert!(
            AnnotateTool::new(deps.clone())
                .validate(&[doc(1)], &wrong)
                .is_err()
        );
        assert!(
            SignPdfTool::new(deps.clone())
                .validate(&[doc(1)], &wrong)
                .is_err()
        );
        assert!(
            FillSignTool::new(deps.clone())
                .validate(&[doc(1)], &wrong)
                .is_err()
        );
        assert!(
            CreateFormsTool::new(deps.clone())
                .validate(&[doc(1)], &wrong)
                .is_err()
        );
        let empty = |o: ToolOptions, t: &dyn Tool| t.validate(&[doc(1)], &o).unwrap_err().code;
        assert_eq!(
            empty(
                ToolOptions::Annotate(AnnotateOptions::default()),
                &AnnotateTool::new(deps.clone())
            ),
            ErrorCode::OptionInvalid
        );
        assert_eq!(
            empty(
                ToolOptions::SignPdf(SignPdfOptions::default()),
                &SignPdfTool::new(deps.clone())
            ),
            ErrorCode::OptionInvalid
        );
        assert_eq!(
            empty(
                ToolOptions::FillSign(FillSignOptions::default()),
                &FillSignTool::new(deps.clone())
            ),
            ErrorCode::OptionInvalid
        );
        assert_eq!(
            empty(
                ToolOptions::CreateForms(CreateFormsOptions::default()),
                &CreateFormsTool::new(deps)
            ),
            ErrorCode::OptionInvalid
        );
    }

    #[test]
    fn annotations_and_values_are_checked_too() {
        let deps = ToolDeps::unavailable();
        let mark = ToolOptions::Annotate(AnnotateOptions {
            annotations: vec![Annotation::Highlight(MarkupAnnotation {
                page: 5,
                regions: vec![region()],
                color: "#FFEB3B".to_owned(),
            })],
        });
        assert!(
            AnnotateTool::new(deps.clone())
                .validate(&[doc(2)], &mark)
                .is_err()
        );
        assert!(
            AnnotateTool::new(deps.clone())
                .validate(&[doc(5)], &mark)
                .is_ok()
        );
        let values = ToolOptions::FillSign(FillSignOptions {
            values: vec![FieldValue {
                name: "name".to_owned(),
                value: "Ada".to_owned(),
            }],
            signatures: vec![],
        });
        assert!(FillSignTool::new(deps).validate(&[doc(1)], &values).is_ok());
        let _ = WhiteoutItem {
            page: 1,
            region: region(),
            color: "#FFFFFF".to_owned(),
        };
    }
}
