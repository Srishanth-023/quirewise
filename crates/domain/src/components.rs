//! Component manifest entries (ADR-0005). Signature checks happen in infrastructure; this module
//! validates the shape of the data so a malformed manifest never reaches the installer.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::units::Bytes;

/// Supported manifest schema version.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// CPU architecture of a component build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Arch {
    /// Windows x64.
    X64,
    /// Windows arm64.
    Arm64,
    /// Architecture-independent data such as language packs.
    Any,
}

/// One downloadable component build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentEntry {
    /// Component id, kebab-case (`libreoffice`, `tessdata-eng`).
    pub id: String,
    /// Version string.
    pub version: String,
    /// Target architecture.
    pub arch: Arch,
    /// HTTPS download URL.
    pub url: String,
    /// Download size.
    pub size: Bytes,
    /// Lowercase hex SHA-256 of the download.
    pub sha256: String,
}

/// The signed `components.json` document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentManifest {
    /// Manifest schema version.
    pub schema_version: u32,
    /// Available components.
    pub components: Vec<ComponentEntry>,
}

impl ComponentEntry {
    /// Validates id, version, URL scheme, size and hash format.
    pub fn validate(&self) -> Result<(), DomainError> {
        let bad = |reason: &str| {
            Err(DomainError::option(
                format!("component {}", self.id),
                reason,
            ))
        };
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return bad("id must be 1 to 64 characters of a-z, 0-9 and -");
        }
        if self.version.is_empty()
            || self.version.len() > 64
            || self.version.chars().any(char::is_control)
        {
            return bad("version is empty, too long or has control characters");
        }
        if !self.url.starts_with("https://")
            || self
                .url
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
        {
            return bad("url must be an https URL without spaces");
        }
        if self.size.get() == 0 {
            return bad("size must be greater than zero");
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .chars()
                .all(|c| matches!(c, '0'..='9' | 'a'..='f'))
        {
            return bad("sha256 must be 64 lowercase hex characters");
        }
        Ok(())
    }
}

impl ComponentManifest {
    /// Validates the schema version and every entry, and rejects duplicate id and arch pairs.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.schema_version != MANIFEST_SCHEMA_VERSION {
            return Err(DomainError::option(
                "manifest schema_version",
                format!(
                    "expected {MANIFEST_SCHEMA_VERSION}, found {}",
                    self.schema_version
                ),
            ));
        }
        for (i, entry) in self.components.iter().enumerate() {
            entry.validate()?;
            if self.components[..i]
                .iter()
                .any(|e| e.id == entry.id && e.arch == entry.arch && e.version == entry.version)
            {
                return Err(DomainError::option(
                    format!("component {}", entry.id),
                    "listed twice with the same version and architecture",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> ComponentEntry {
        ComponentEntry {
            id: "libreoffice".to_owned(),
            version: "24.8.3".to_owned(),
            arch: Arch::Arm64,
            url: "https://example.invalid/lo.zip".to_owned(),
            size: Bytes::from_mb(300),
            sha256: "a".repeat(64),
        }
    }

    #[test]
    fn valid_entry_passes() {
        assert!(entry().validate().is_ok());
    }

    #[test]
    fn malformed_entries_fail() {
        let cases = [
            ComponentEntry {
                id: "Bad Id".to_owned(),
                ..entry()
            },
            ComponentEntry {
                url: "http://example.invalid/x".to_owned(),
                ..entry()
            },
            ComponentEntry {
                sha256: "XYZ".to_owned(),
                ..entry()
            },
            ComponentEntry {
                sha256: "A".repeat(64),
                ..entry()
            },
            ComponentEntry {
                size: Bytes::new(0),
                ..entry()
            },
        ];
        for c in cases {
            assert!(c.validate().is_err(), "should reject {c:?}");
        }
    }

    #[test]
    fn manifest_rejects_wrong_schema_and_duplicates() {
        let ok = ComponentManifest {
            schema_version: 1,
            components: vec![entry()],
        };
        assert!(ok.validate().is_ok());
        let wrong = ComponentManifest {
            schema_version: 2,
            components: vec![entry()],
        };
        assert!(wrong.validate().is_err());
        let dup = ComponentManifest {
            schema_version: 1,
            components: vec![entry(), entry()],
        };
        assert!(dup.validate().is_err());
    }
}
