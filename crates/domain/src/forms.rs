//! What the form fields of a document look like, as the editor needs to show and fill them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::tools::common::Region;

/// The kind of a form field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FormFieldKind {
    /// Text.
    Text,
    /// A box that is ticked or not.
    Checkbox,
    /// One choice of a group.
    Radio,
    /// A drop-down list.
    Dropdown,
    /// A list box.
    ListBox,
    /// A place to sign.
    Signature,
    /// A push button.
    Button,
}

/// A field of the form of a document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FormField {
    /// The full name, with its parents' names joined by dots.
    pub name: String,
    /// The kind.
    pub kind: FormFieldKind,
    /// The page the field is on, from 1.
    pub page: u32,
    /// The area it covers, as fractions of the displayed page.
    pub region: Region,
    /// The current value: the text, the selected choice, or `true` or `false` for a check box.
    pub value: String,
    /// The choices of a list, or the choice a radio button stands for.
    pub options: Vec<String>,
    /// The field cannot be changed.
    pub read_only: bool,
    /// The field must be filled.
    pub required: bool,
    /// A text field with several lines.
    pub multiline: bool,
}

/// A block of text on a page, found so that it can be changed: the editor covers it and writes
/// new text in its place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextBlock {
    /// The area the text covers, as fractions of the displayed page.
    pub region: Region,
    /// The text, lines joined by spaces.
    pub text: String,
    /// The usual size of the text, in points.
    pub size_pt: f32,
    /// Bold throughout.
    pub bold: bool,
    /// Italic throughout.
    pub italic: bool,
}

/// What a field found on a page could be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SuggestedKind {
    /// A text field.
    Text,
    /// A check box.
    Checkbox,
}

/// A place on a page that looks like a field to fill: a line, an empty box or a small square.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FieldSuggestion {
    /// The page, from 1.
    pub page: u32,
    /// The area.
    pub region: Region,
    /// What it could be.
    pub kind: SuggestedKind,
    /// The words next to it, which usually say what to enter.
    pub label: String,
    /// A name for the field made from the label.
    pub name: String,
}
