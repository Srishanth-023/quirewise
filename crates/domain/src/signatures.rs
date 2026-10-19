//! Signatures, initials and stamps the person keeps to use again.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::tools::editor::SignatureSource;

/// Most signatures that are kept.
pub const MAX_SAVED: usize = 50;
/// Longest name of a saved signature, in characters.
pub const MAX_NAME: usize = 80;

/// What a saved item is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SavedKind {
    /// A full signature.
    Signature,
    /// Initials.
    Initials,
    /// A company stamp.
    Stamp,
}

/// What the person gives when saving.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewSavedSignature {
    /// A name to find it by.
    pub name: String,
    /// What it is for.
    pub kind: SavedKind,
    /// Width divided by height of the box it was made in, so it is placed in proportion.
    pub aspect: f32,
    /// What it looks like: strokes, typed text or a picture kept inside.
    pub source: SignatureSource,
}

impl NewSavedSignature {
    /// Checks the name, the proportions and that the source can be kept.
    pub fn validate(&self) -> Result<(), DomainError> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(char::is_control)
        {
            return Err(DomainError::option(
                "name",
                "give the signature a name of at most 80 characters",
            ));
        }
        if !self.aspect.is_finite() || !(0.1..=20.0).contains(&self.aspect) {
            return Err(DomainError::option(
                "aspect",
                "the proportions are not usable",
            ));
        }
        match &self.source {
            SignatureSource::Drawn { .. }
            | SignatureSource::Typed { .. }
            | SignatureSource::ImageData { .. } => {}
            SignatureSource::Image { .. } | SignatureSource::Date { .. } => {
                return Err(DomainError::option(
                    "source",
                    "only drawn, typed or picture signatures can be kept",
                ));
            }
        }
        // The same checks as placing it, on a box that always fits.
        crate::tools::editor::SignatureItem {
            page: 1,
            region: crate::tools::common::Region {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            source: self.source.clone(),
        }
        .validate(1)
    }
}

/// A kept signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SavedSignature {
    /// Its identifier.
    pub id: String,
    /// The name it was saved with.
    pub name: String,
    /// What it is for.
    pub kind: SavedKind,
    /// Width divided by height.
    pub aspect: f32,
    /// What it looks like.
    pub source: SignatureSource,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::editor::{DateFormat, Point, TextStyle};

    fn typed() -> SignatureSource {
        SignatureSource::Typed {
            text: "A. Lovelace".to_owned(),
            style: TextStyle::default(),
        }
    }

    fn item(source: SignatureSource) -> NewSavedSignature {
        NewSavedSignature {
            name: "My signature".to_owned(),
            kind: SavedKind::Signature,
            aspect: 3.0,
            source,
        }
    }

    #[test]
    fn drawn_typed_and_picture_signatures_can_be_kept() {
        assert!(item(typed()).validate().is_ok());
        let drawn = SignatureSource::Drawn {
            strokes: vec![vec![Point { x: 0.1, y: 0.5 }, Point { x: 0.9, y: 0.5 }]],
            color: "#111827".to_owned(),
            width_pt: 1.5,
        };
        assert!(item(drawn).validate().is_ok());
        assert!(
            item(SignatureSource::ImageData {
                data: "AAAA".to_owned()
            })
            .validate()
            .is_ok()
        );
        assert!(
            item(SignatureSource::ImageData {
                data: String::new()
            })
            .validate()
            .is_err()
        );
    }

    #[test]
    fn dates_files_odd_names_and_odd_proportions_are_refused() {
        let date = SignatureSource::Date {
            format: DateFormat::Iso,
            style: TextStyle::default(),
        };
        assert!(item(date).validate().is_err());
        assert!(
            item(SignatureSource::Image {
                path: "C:\\a.png".into()
            })
            .validate()
            .is_err()
        );
        for name in ["", "   ", &"x".repeat(81)] {
            let bad = NewSavedSignature {
                name: name.to_owned(),
                ..item(typed())
            };
            assert!(bad.validate().is_err(), "{name:?}");
        }
        for aspect in [0.0, f32::NAN, 99.0] {
            let bad = NewSavedSignature {
                aspect,
                ..item(typed())
            };
            assert!(bad.validate().is_err());
        }
    }
}
