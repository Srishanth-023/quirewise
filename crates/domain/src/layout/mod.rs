//! Reading the structure of a page from its words and lines: reading order, paragraphs, headings,
//! lists and tables. The PDF to Word, PowerPoint, Excel, Markdown and HTML tools build on it.
//!
//! The analysis is pure: an engine reads characters, lines and pictures from a page, this module
//! decides what they mean.

pub mod fields;
pub mod flow;
pub mod model;
pub mod render;
pub mod tables;
pub mod words;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use fields::suggest_fields;
pub use flow::{BodyStats, analyze, analyze_page, body_stats, list_marker, strip_running};
pub use model::{
    Block, Cell, CharBox, PageChars, PageContent, PageLayout, PictureBox, Rect, Rule, Span, Table,
    Word, plain,
};
pub use words::words_from_chars;

/// How tables are found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Detection {
    /// Ruled tables first, then tables made only of aligned text.
    #[default]
    Auto,
    /// Only tables drawn with lines.
    Lattice,
    /// Only tables made of aligned text.
    Stream,
}
