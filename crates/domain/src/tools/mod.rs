//! Tool identifiers and per-tool option structs. One module per tool.

pub mod rotate;

use core::fmt;
use core::str::FromStr;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
pub use rotate::RotateOptions;

/// Identifier of a tool, kebab-case on the wire (`split-by-size`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ToolId {
    /// Rotate pages (Appendix A row 32).
    Rotate,
}

impl ToolId {
    /// Every tool id the domain knows. The runtime tool list comes from the registry.
    pub const ALL: [ToolId; 1] = [ToolId::Rotate];

    /// The kebab-case identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rotate => "rotate",
        }
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ToolId {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|id| id.as_str() == s)
            .ok_or_else(|| DomainError::option("tool", format!("unknown tool \"{s}\"")))
    }
}

/// Options for any tool. The tag is the tool id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "tool", rename_all = "kebab-case")]
pub enum ToolOptions {
    /// Options for [`ToolId::Rotate`].
    Rotate(RotateOptions),
}

impl ToolOptions {
    /// The tool these options belong to.
    pub const fn tool_id(&self) -> ToolId {
        match self {
            Self::Rotate(_) => ToolId::Rotate,
        }
    }

    /// Default options for a tool, matching Appendix A.
    pub fn defaults_for(id: ToolId) -> Self {
        match id {
            ToolId::Rotate => Self::Rotate(RotateOptions::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_ids_are_kebab_case_and_parse_back() {
        for id in ToolId::ALL {
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(json, format!("\"{}\"", id.as_str()));
            assert_eq!(id.as_str().parse::<ToolId>().unwrap(), id);
            assert!(
                id.as_str()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-')
            );
        }
        assert!("nope".parse::<ToolId>().is_err());
    }

    #[test]
    fn options_are_tagged_by_tool_id() {
        let opts = ToolOptions::defaults_for(ToolId::Rotate);
        let json = serde_json::to_value(&opts).unwrap();
        assert_eq!(json["tool"], "rotate");
        let back: ToolOptions = serde_json::from_value(json).unwrap();
        assert_eq!(back, opts);
        assert_eq!(back.tool_id(), ToolId::Rotate);
    }
}
