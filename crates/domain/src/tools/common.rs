//! Types shared by several tools' options.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// A rotation in quarter turns, clockwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub enum RightAngle {
    /// No rotation.
    #[default]
    #[serde(rename = "0")]
    Deg0,
    /// 90 degrees clockwise.
    #[serde(rename = "90")]
    Deg90,
    /// 180 degrees.
    #[serde(rename = "180")]
    Deg180,
    /// 270 degrees clockwise (90 counter-clockwise).
    #[serde(rename = "270")]
    Deg270,
}

impl RightAngle {
    /// The angle in degrees.
    pub const fn degrees(self) -> u16 {
        match self {
            Self::Deg0 => 0,
            Self::Deg90 => 90,
            Self::Deg180 => 180,
            Self::Deg270 => 270,
        }
    }

    /// Builds an angle from degrees, which must be a multiple of 90 from 0 to 270.
    pub fn from_degrees(degrees: u16) -> Result<Self, DomainError> {
        match degrees {
            0 => Ok(Self::Deg0),
            90 => Ok(Self::Deg90),
            180 => Ok(Self::Deg180),
            270 => Ok(Self::Deg270),
            _ => Err(DomainError::option("rotation", "must be 0, 90, 180 or 270")),
        }
    }

    /// Adds two rotations.
    pub fn plus(self, other: Self) -> Self {
        // Both values are multiples of 90 below 360, so the sum modulo 360 always converts.
        Self::from_degrees((self.degrees() + other.degrees()) % 360).unwrap_or(Self::Deg0)
    }
}

/// Page size for pages the app creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PageSizeChoice {
    /// Same size as the neighboring page (default).
    #[default]
    SameAsNeighbor,
    /// ISO A4.
    A4,
    /// US Letter.
    Letter,
    /// US Legal.
    Legal,
    /// ISO A3.
    A3,
}

impl PageSizeChoice {
    /// Width and height in points, or `None` for [`Self::SameAsNeighbor`].
    pub const fn points(self) -> Option<(f32, f32)> {
        match self {
            Self::SameAsNeighbor => None,
            Self::A4 => Some((595.0, 842.0)),
            Self::Letter => Some((612.0, 792.0)),
            Self::Legal => Some((612.0, 1008.0)),
            Self::A3 => Some((842.0, 1191.0)),
        }
    }
}

/// A rectangle on a page, as fractions of the page width and height measured from the top-left.
/// This is how the user draws a region, independent of page size and rotation of the preview.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Region {
    /// Left edge, 0 to 1.
    pub x: f32,
    /// Top edge, 0 to 1.
    pub y: f32,
    /// Width, greater than 0 and at most 1.
    pub width: f32,
    /// Height, greater than 0 and at most 1.
    pub height: f32,
}

impl Region {
    /// Checks that the rectangle lies inside the page.
    pub fn validate(&self) -> Result<(), DomainError> {
        let ok = |v: f32| v.is_finite();
        let inside = ok(self.x)
            && ok(self.y)
            && ok(self.width)
            && ok(self.height)
            && self.x >= 0.0
            && self.y >= 0.0
            && self.width > 0.0
            && self.height > 0.0
            && self.x + self.width <= 1.001
            && self.y + self.height <= 1.001;
        if inside {
            Ok(())
        } else {
            Err(DomainError::option(
                "region",
                "draw a rectangle that lies inside the page",
            ))
        }
    }
}

/// A box as fractions of a page's visible area, measured from the bottom-left like PDF space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FractionBox {
    /// Left edge, 0 to 1.
    pub left: f32,
    /// Bottom edge, 0 to 1.
    pub bottom: f32,
    /// Right edge, 0 to 1.
    pub right: f32,
    /// Top edge, 0 to 1.
    pub top: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_angles_add_and_wrap() {
        assert_eq!(RightAngle::Deg90.plus(RightAngle::Deg270), RightAngle::Deg0);
        assert_eq!(
            RightAngle::Deg180.plus(RightAngle::Deg90),
            RightAngle::Deg270
        );
        assert_eq!(
            RightAngle::from_degrees(45).unwrap_err().code(),
            crate::ErrorCode::OptionInvalid
        );
        assert_eq!(
            serde_json::to_string(&RightAngle::Deg180).unwrap(),
            "\"180\""
        );
    }

    #[test]
    fn named_sizes_have_points() {
        assert_eq!(PageSizeChoice::Letter.points(), Some((612.0, 792.0)));
        assert_eq!(PageSizeChoice::SameAsNeighbor.points(), None);
    }

    #[test]
    fn regions_must_lie_on_the_page() {
        assert!(
            Region {
                x: 0.1,
                y: 0.1,
                width: 0.5,
                height: 0.2
            }
            .validate()
            .is_ok()
        );
        for bad in [
            Region {
                x: -0.1,
                y: 0.0,
                width: 0.5,
                height: 0.5,
            },
            Region {
                x: 0.6,
                y: 0.0,
                width: 0.5,
                height: 0.5,
            },
            Region {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.5,
            },
            Region {
                x: f32::NAN,
                y: 0.0,
                width: 0.5,
                height: 0.5,
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }
}
