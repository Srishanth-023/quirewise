//! Redact (row 54): removes what lies under areas, words and patterns from a document for good.
//!
//! What to remove comes from areas drawn on pages and from a search of the text. The engine
//! removes the content under each area from the page itself and draws a box over it. The result
//! is then read back: text still found under an area, or a searched word still on a page, means
//! the page is not trusted and is replaced by a picture of itself with the boxes burned in.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use domain::tools::ToolGroup;
use domain::tools::common::Region;
use domain::tools::convert::{PictureColor, PictureFormat};
use domain::tools::document::MetadataChange;
use domain::tools::redact::RedactOptions;
use domain::{Dpi, ErrorCode, PageIndex, ToolId, ToolOptions};

use super::airedact::add_model_terms;
use super::common::{
    Progress, Scratch, ToolDeps, first_input, kept_protection, page_count, publish_pdf_checked,
    wrong_options,
};
use super::redactfind::{Matcher, PageText, find_matches};
use crate::error::AppError;
use crate::ports::{
    PackageOptions, PdfEdit, PictureSpec, ProtectionSource, RedactPage, RedactPlan,
};
use crate::tool::{InputDoc, InputKind, JobContext, OutputArtifact, Plan, Tool};

/// Resolution of the pictures that replace pages that could not be cleaned.
const REPLACEMENT_DPI: u32 = 200;

/// Removes content under areas, words and patterns.
pub struct RedactTool {
    deps: ToolDeps,
}

impl RedactTool {
    /// Creates the tool over its ports.
    pub fn new(deps: ToolDeps) -> Self {
        Self { deps }
    }

    /// The regions of every page: the drawn areas, and what the search finds.
    fn find(
        &self,
        ctx: &JobContext<'_>,
        input: &InputDoc,
        options: &RedactOptions,
        progress: &mut Progress<'_>,
    ) -> Result<BTreeMap<u32, Vec<Region>>, AppError> {
        let mut found: BTreeMap<u32, Vec<Region>> = BTreeMap::new();
        for area in &options.areas {
            found.entry(area.page - 1).or_default().push(area.region);
        }
        if options.searches() {
            let matches = find_matches(
                self.deps.layout.as_ref(),
                &input.path,
                input.password.as_ref(),
                options,
                page_count(input),
                ctx.cancel,
                &mut || progress.advance(1),
            )?;
            for (page, regions) in matches {
                found.entry(page.get()).or_default().extend(regions);
            }
        }
        Ok(found)
    }

    /// Reads a page of the result back: nothing may be left under an area, and nothing searched
    /// for may be left on the page.
    fn is_clean(
        &self,
        path: &Path,
        page: &RedactPage,
        matcher: Option<&Matcher>,
    ) -> Result<bool, AppError> {
        for region in &page.regions {
            // A little less than the area, so a neighbor that only touches it does not count.
            let dx = (region.width * 0.1).min(0.002);
            let dy = region.height * 0.15;
            let inner = Region {
                x: region.x + dx,
                y: region.y + dy,
                width: (region.width - 2.0 * dx).max(0.0005),
                height: (region.height - 2.0 * dy).max(0.0005),
            };
            let text = self
                .deps
                .text
                .text_in_region(path, None, page.page, &inner)?;
            if !text.trim().is_empty() {
                return Ok(false);
            }
        }
        if let Some(matcher) = matcher {
            let chars = self.deps.layout.page_chars(path, None, page.page)?;
            if !matcher.find(&PageText::of(&chars).text).is_empty() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// A picture of each page that could not be cleaned, with its areas filled in.
    fn burn<'a>(
        &self,
        ctx: &'a JobContext<'a>,
        source: &Path,
        plan: &RedactPlan,
        pages: &BTreeSet<u32>,
    ) -> Result<Vec<(PageIndex, Scratch<'a>)>, AppError> {
        let dpi = Dpi::new(REPLACEMENT_DPI)?;
        let spec = PictureSpec {
            format: PictureFormat::Png,
            jpeg_quality: 90,
            color: PictureColor::Color,
            dpi,
        };
        let fill = [plan.fill.r, plan.fill.g, plan.fill.b, 255];
        let mut out = Vec::new();
        for page_plan in plan.pages.iter().filter(|p| pages.contains(&p.page.get())) {
            ctx.cancel.check()?;
            let mut image = self
                .deps
                .renderer
                .render_page(source, None, page_plan.page, dpi)?;
            let (w, h) = (image.width as usize, image.height as usize);
            for region in &page_plan.regions {
                // A pixel more on every side, so no edge of what was there shows.
                let x0 = ((region.x * w as f32).floor() as isize - 1).clamp(0, w as isize) as usize;
                let x1 = (((region.x + region.width) * w as f32).ceil() as isize + 1)
                    .clamp(0, w as isize) as usize;
                let y0 = ((region.y * h as f32).floor() as isize - 1).clamp(0, h as isize) as usize;
                let y1 = (((region.y + region.height) * h as f32).ceil() as isize + 1)
                    .clamp(0, h as isize) as usize;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let at = (y * w + x) * 4;
                        image.rgba[at..at + 4].copy_from_slice(&fill);
                    }
                }
            }
            let file = Scratch::new(ctx, "png");
            self.deps
                .pictures
                .write_picture(&image, &spec, file.path())?;
            out.push((page_plan.page, file));
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    fn produce(
        &self,
        ctx: &JobContext<'_>,
        input: &InputDoc,
        options: &RedactOptions,
        plan: &RedactPlan,
        matcher: Option<&Matcher>,
        protection: Option<&ProtectionSource>,
        tmp: &Path,
    ) -> Result<(), AppError> {
        let deps = &self.deps;
        let unlocked = Scratch::new(ctx, "pdf");
        deps.security
            .unlock(&input.path, input.password.as_ref(), unlocked.path())?;
        ctx.cancel.check()?;
        let cleaned = Scratch::new(ctx, "pdf");
        let outcome = deps.editor.redact(unlocked.path(), plan, cleaned.path())?;
        ctx.cancel.check()?;

        // Pages whose content could not be cleaned, and pages that still show something when
        // read back.
        let mut raster: BTreeSet<u32> = outcome.raster.iter().map(|p| p.get()).collect();
        for page in &plan.pages {
            if !raster.contains(&page.page.get())
                && !self.is_clean(cleaned.path(), page, matcher)?
            {
                raster.insert(page.page.get());
            }
        }

        let replaced = Scratch::new(ctx, "pdf");
        let mut stage: PathBuf = cleaned.path().to_path_buf();
        if !raster.is_empty() {
            let pictures = self.burn(ctx, unlocked.path(), plan, &raster)?;
            let list: Vec<(PageIndex, PathBuf)> = pictures
                .iter()
                .map(|(page, file)| (*page, file.path().to_path_buf()))
                .collect();
            deps.editor.apply(
                cleaned.path(),
                &PdfEdit::ReplaceWithPictures(list),
                replaced.path(),
            )?;
            stage = replaced.path().to_path_buf();
        }

        let stripped = Scratch::new(ctx, "pdf");
        if options.strip_metadata {
            deps.editor.apply(
                &stage,
                &PdfEdit::SetMetadata(MetadataChange {
                    remove_all: true,
                    ..MetadataChange::default()
                }),
                stripped.path(),
            )?;
            stage = stripped.path().to_path_buf();
        }
        ctx.cancel.check()?;
        deps.packager.package(
            &stage,
            None,
            &PackageOptions {
                keep_protection: protection.cloned(),
                ..PackageOptions::new(false, true)
            },
            tmp,
        )
    }
}

impl Tool for RedactTool {
    fn id(&self) -> ToolId {
        ToolId::Redact
    }
    fn group(&self) -> ToolGroup {
        ToolGroup::Security
    }
    fn label(&self) -> &'static str {
        "redacted"
    }
    fn accepts(&self) -> &'static [InputKind] {
        &[InputKind::Pdf]
    }
    fn options_schema(&self) -> schemars::Schema {
        schemars::schema_for!(RedactOptions)
    }

    fn validate(&self, inputs: &[InputDoc], opts: &ToolOptions) -> Result<Plan, AppError> {
        let ToolOptions::Redact(options) = opts else {
            return Err(wrong_options());
        };
        let input = first_input(inputs)?;
        let count = page_count(input);
        options.validate(count)?;
        // A pattern that is not valid is reported before any work is done.
        if options.searches() {
            Matcher::compile(options)?;
        }
        let searched = if options.reads_pages() {
            options.search_pages(count)?.len() as u64
        } else {
            0
        };
        // The model reads each searched page once more.
        let reading = if options.ai_suggestions { searched } else { 0 };
        Ok(Plan {
            tool: ToolId::Redact,
            inputs: inputs.to_vec(),
            options: opts.clone(),
            total_units: searched + reading + 2,
        })
    }

    fn execute(&self, ctx: &JobContext<'_>, plan: Plan) -> Result<Vec<OutputArtifact>, AppError> {
        let ToolOptions::Redact(chosen) = &plan.options else {
            return Err(wrong_options());
        };
        let input = first_input(&plan.inputs)?;
        let count = page_count(input);
        let fill = chosen.fill()?;
        let mut progress = Progress::start(ctx, plan.total_units);

        // What the model finds is added to the words the person gave.
        let extended;
        let options = if chosen.ai_suggestions {
            extended = add_model_terms(
                self.deps.llm.as_ref(),
                self.deps.text.as_ref(),
                &input.path,
                input.password.as_ref(),
                chosen,
                count,
                ctx.cancel,
                &mut || progress.advance(1),
            )?;
            &extended
        } else {
            chosen
        };

        let found = self.find(ctx, input, options, &mut progress)?;
        if found.is_empty() {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                "Nothing was found to remove on the pages you chose. Check the words and patterns, or draw an area.",
            ));
        }
        let redact_plan = RedactPlan {
            pages: found
                .into_iter()
                .map(|(page, regions)| RedactPage {
                    page: PageIndex::new(page),
                    regions,
                })
                .collect(),
            fill,
        };
        let matcher = if options.searches() {
            Some(Matcher::compile(options)?)
        } else {
            None
        };
        let protection = kept_protection(&self.deps, input)?;
        let password = protection.as_ref().map(|p| &p.password);
        progress.advance(1);
        let out = publish_pdf_checked(
            ctx,
            &self.deps,
            input,
            self.label(),
            Some(count),
            password,
            &mut |tmp| {
                self.produce(
                    ctx,
                    input,
                    options,
                    &redact_plan,
                    matcher.as_ref(),
                    protection.as_ref(),
                    tmp,
                )
            },
        )?;
        progress.advance(1);
        Ok(vec![out])
    }
}
