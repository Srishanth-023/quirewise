//! Recipes: a saved chain of tool steps (Appendix A row 58).
//!
//! A recipe feeds the PDF each step writes into the next step. The domain only knows the chain
//! rules; running the steps is the application's job.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::tools::{ToolId, ToolOptions};

/// Most steps one recipe may hold.
pub const MAX_STEPS: usize = 20;
/// Longest recipe name, in characters.
pub const MAX_NAME_CHARS: usize = 80;
/// The `format` marker of an exported recipe file.
pub const FILE_FORMAT: &str = "quirewise-recipe";
/// The version of the exported file layout.
pub const FILE_VERSION: u32 = 1;

/// Where a tool may sit in a chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ChainRole {
    /// Turns another file type into one PDF. Only as the first step.
    Source,
    /// Reads one PDF and writes one PDF. Anywhere.
    Middle,
    /// Writes a report, several files or another format. Only as the last step.
    Last,
    /// Needs the editor, several inputs or a person, so it cannot run unattended.
    Excluded,
}

impl ChainRole {
    /// The role of a tool in a recipe.
    pub const fn of(tool: ToolId) -> Self {
        use ToolId::*;
        match tool {
            WordToPdf | PowerpointToPdf | ExcelToPdf => Self::Source,
            Rotate | InsertPages | Compress | Repair | RemoveAnnotations | Flatten
            | EditMetadata | Unlock | Sanitize | Linearize | PageNumbers | HeaderFooter | Bates
            | Watermark | Crop | Resize | Flip | NUp | Grayscale | Bookmarks | Attachments
            | PageLabels | ExtractPages | RemovePages | TranslatePdf | PdfToPdfa | Redact | Ocr
            | Deskew => Self::Middle,
            SplitRanges | SplitEveryN | SplitByBookmarks | SplitInHalf | SplitBySize
            | SplitByText | SmartSplit | RenameByContent | Protect | DigitalSign | AiSummarizer
            | ValidatePdfa | CheckPdfua | VerifySignatures | PdfToWord | PdfToPowerpoint
            | PdfToExcel | PdfToMarkdown | PdfToHtml | PdfToImages | ExtractImages | PdfToText
            | PdfToTiff => Self::Last,
            Merge | AlternateMix | Organize | Compare | EditPdf | Annotate | FillSign | SignPdf
            | CreateForms | ScanToPdf | HtmlToPdf | ImagesToPdf | TiffToPdf => Self::Excluded,
        }
    }
}

/// One tool run in a recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecipeStep {
    /// The options; their tag names the tool.
    pub options: ToolOptions,
}

impl RecipeStep {
    /// The tool this step runs.
    pub const fn tool(&self) -> ToolId {
        self.options.tool_id()
    }
}

/// A named chain of steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Recipe {
    /// Stable identifier, assigned when the recipe is first saved.
    #[serde(default)]
    pub id: String,
    /// Name shown in the list.
    pub name: String,
    /// The steps, first to last.
    pub steps: Vec<RecipeStep>,
}

impl Recipe {
    /// Checks the name and the chain rules.
    pub fn validate(&self) -> Result<(), DomainError> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(DomainError::option("name", "give the recipe a name"));
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(DomainError::option(
                "name",
                format!("use at most {MAX_NAME_CHARS} characters"),
            ));
        }
        if self.steps.is_empty() {
            return Err(DomainError::option("steps", "add at least one step"));
        }
        if self.steps.len() > MAX_STEPS {
            return Err(DomainError::option(
                "steps",
                format!("a recipe has at most {MAX_STEPS} steps"),
            ));
        }
        let last = self.steps.len() - 1;
        for (i, step) in self.steps.iter().enumerate() {
            let tool = step.tool();
            let place = |why: &str| DomainError::option("steps", format!("step {}: {why}", i + 1));
            match ChainRole::of(tool) {
                ChainRole::Excluded => {
                    return Err(place(&format!("{tool} cannot run in a recipe")));
                }
                ChainRole::Source if i != 0 => {
                    return Err(place(&format!("{tool} can only be the first step")));
                }
                ChainRole::Last if i != last => {
                    return Err(place(&format!(
                        "{tool} writes its own files, so it can only be the last step"
                    )));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// True when the first step needs a file that is not a PDF.
    pub fn starts_with_source(&self) -> bool {
        self.steps
            .first()
            .is_some_and(|s| ChainRole::of(s.tool()) == ChainRole::Source)
    }
}

/// The layout of an exported recipe file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecipeFile {
    /// Always [`FILE_FORMAT`].
    pub format: String,
    /// Layout version.
    pub version: u32,
    /// The recipe.
    pub recipe: Recipe,
}

impl RecipeFile {
    /// Wraps a recipe for export. The id stays out: the importer assigns a new one.
    pub fn wrap(recipe: &Recipe) -> Self {
        Self {
            format: FILE_FORMAT.to_owned(),
            version: FILE_VERSION,
            recipe: Recipe {
                id: String::new(),
                ..recipe.clone()
            },
        }
    }

    /// Checks the marker and version, then the recipe itself.
    pub fn check(&self) -> Result<(), DomainError> {
        if self.format != FILE_FORMAT {
            return Err(DomainError::option(
                "file",
                "this is not a Quirewise recipe",
            ));
        }
        if self.version > FILE_VERSION {
            return Err(DomainError::option(
                "file",
                "this recipe was made by a newer version of Quirewise",
            ));
        }
        self.recipe.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(tool: ToolId) -> RecipeStep {
        RecipeStep {
            options: ToolOptions::defaults_for(tool),
        }
    }

    fn recipe(tools: &[ToolId]) -> Recipe {
        Recipe {
            id: String::new(),
            name: "Tidy".into(),
            steps: tools.iter().map(|t| step(*t)).collect(),
        }
    }

    #[test]
    fn every_tool_has_a_role_and_the_split_is_sane() {
        let roles: Vec<ChainRole> = ToolId::ALL.iter().map(|t| ChainRole::of(*t)).collect();
        assert!(roles.contains(&ChainRole::Source));
        assert!(roles.contains(&ChainRole::Middle));
        assert!(roles.contains(&ChainRole::Last));
        assert!(roles.contains(&ChainRole::Excluded));
        assert_eq!(ChainRole::of(ToolId::Merge), ChainRole::Excluded);
        assert_eq!(ChainRole::of(ToolId::Compress), ChainRole::Middle);
    }

    #[test]
    fn a_well_formed_chain_passes() {
        let r = recipe(&[
            ToolId::WordToPdf,
            ToolId::Rotate,
            ToolId::Compress,
            ToolId::Protect,
        ]);
        r.validate().unwrap();
        assert!(r.starts_with_source());
        assert!(!recipe(&[ToolId::Rotate]).starts_with_source());
    }

    #[test]
    fn chain_rules_are_enforced() {
        for bad in [
            vec![ToolId::Merge],
            vec![ToolId::Rotate, ToolId::WordToPdf],
            vec![ToolId::Protect, ToolId::Rotate],
            vec![ToolId::SplitEveryN, ToolId::Compress],
            vec![],
        ] {
            assert!(recipe(&bad).validate().is_err(), "{bad:?}");
        }
        assert!(
            recipe(&[ToolId::Rotate; MAX_STEPS + 1]).validate().is_err(),
            "too many steps"
        );
    }

    #[test]
    fn the_name_is_checked() {
        let mut r = recipe(&[ToolId::Rotate]);
        r.name = "   ".into();
        assert!(r.validate().is_err());
        r.name = "x".repeat(MAX_NAME_CHARS + 1);
        assert!(r.validate().is_err());
    }

    #[test]
    fn export_drops_the_id_and_round_trips() {
        let mut r = recipe(&[ToolId::Rotate, ToolId::Compress]);
        r.id = "abc".into();
        let file = RecipeFile::wrap(&r);
        assert!(file.recipe.id.is_empty());
        let json = serde_json::to_string(&file).unwrap();
        let back: RecipeFile = serde_json::from_str(&json).unwrap();
        back.check().unwrap();
        assert_eq!(back, file);
    }

    #[test]
    fn foreign_and_newer_files_are_refused() {
        let mut file = RecipeFile::wrap(&recipe(&[ToolId::Rotate]));
        file.format = "other".into();
        assert!(file.check().is_err());
        let mut newer = RecipeFile::wrap(&recipe(&[ToolId::Rotate]));
        newer.version = FILE_VERSION + 1;
        assert!(newer.check().is_err());
    }
}
