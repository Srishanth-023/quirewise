//! Newtypes for units so options are never stringly or bare-number typed.

use core::num::NonZeroU32;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// Zero-based page index.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct PageIndex(u32);

impl PageIndex {
    /// Creates an index from a zero-based value.
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// The zero-based value.
    pub const fn get(self) -> u32 {
        self.0
    }

    /// The matching one-based page number.
    pub fn to_number(self) -> PageNumber {
        PageNumber(NonZeroU32::MIN.saturating_add(self.0))
    }
}

/// One-based page number as shown to users.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct PageNumber(NonZeroU32);

impl PageNumber {
    /// Creates a page number, or `None` for zero.
    pub fn new(number: u32) -> Option<Self> {
        NonZeroU32::new(number).map(Self)
    }

    /// The one-based value.
    pub const fn get(self) -> u32 {
        self.0.get()
    }

    /// The matching zero-based index.
    pub const fn to_index(self) -> PageIndex {
        PageIndex(self.0.get() - 1)
    }
}

/// Dots per inch, limited to a range PDFium can render sensibly.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(try_from = "u32", into = "u32")]
#[schemars(with = "u32")]
pub struct Dpi(u32);

impl Dpi {
    /// Smallest accepted value.
    pub const MIN: u32 = 10;
    /// Largest accepted value.
    pub const MAX: u32 = 2400;

    /// Creates a validated resolution.
    pub fn new(value: u32) -> Result<Self, DomainError> {
        if (Self::MIN..=Self::MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(DomainError::option(
                "dpi",
                format!("must be between {} and {}", Self::MIN, Self::MAX),
            ))
        }
    }

    /// The value in dots per inch.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Default for Dpi {
    /// 150 dots per inch, the usual choice for screens and ordinary printing.
    fn default() -> Self {
        Self(150)
    }
}

impl TryFrom<u32> for Dpi {
    type Error = DomainError;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Dpi> for u32 {
    fn from(value: Dpi) -> Self {
        value.0
    }
}

/// A whole-number percentage, 0 to 1000.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(try_from = "u16", into = "u16")]
#[schemars(with = "u16")]
pub struct Percent(u16);

impl Percent {
    /// Largest accepted value (scale up to ten times).
    pub const MAX: u16 = 1000;

    /// Creates a validated percentage.
    pub fn new(value: u16) -> Result<Self, DomainError> {
        if value <= Self::MAX {
            Ok(Self(value))
        } else {
            Err(DomainError::option(
                "percent",
                format!("must be between 0 and {}", Self::MAX),
            ))
        }
    }

    /// The value as a whole percentage.
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for Percent {
    type Error = DomainError;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Percent> for u16 {
    fn from(value: Percent) -> Self {
        value.0
    }
}

/// A size in bytes. Human sizes use binary multiples (1 MB is 1,048,576 bytes).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(transparent)]
pub struct Bytes(u64);

impl Bytes {
    /// One mebibyte.
    pub const MB: u64 = 1024 * 1024;
    /// One gibibyte.
    pub const GB: u64 = 1024 * 1024 * 1024;

    /// Wraps a raw byte count.
    pub const fn new(bytes: u64) -> Self {
        Self(bytes)
    }

    /// Whole mebibytes, saturating on overflow.
    pub const fn from_mb(mb: u64) -> Self {
        Self(mb.saturating_mul(Self::MB))
    }

    /// The raw byte count.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A length in millimeters.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "f64", into = "f64")]
#[schemars(with = "f64")]
pub struct Millimeters(f64);

impl Millimeters {
    /// Largest accepted length, 5 meters.
    pub const MAX: f64 = 5000.0;

    /// Creates a validated length (finite, not negative, not absurdly large).
    pub fn new(value: f64) -> Result<Self, DomainError> {
        if value.is_finite() && (0.0..=Self::MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(DomainError::option(
                "millimeters",
                format!("must be between 0 and {}", Self::MAX),
            ))
        }
    }

    /// The value in millimeters.
    pub const fn get(self) -> f64 {
        self.0
    }

    /// The value in PDF points (1/72 inch).
    pub fn to_points(self) -> f64 {
        self.0 * 72.0 / 25.4
    }
}

impl TryFrom<f64> for Millimeters {
    type Error = DomainError;
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Millimeters> for f64 {
    fn from(value: Millimeters) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_index_and_number_convert_both_ways() {
        let idx = PageIndex::new(0);
        assert_eq!(idx.to_number().get(), 1);
        assert_eq!(PageNumber::new(5).map(|n| n.to_index().get()), Some(4));
        assert_eq!(PageNumber::new(0), None);
    }

    #[test]
    fn dpi_rejects_out_of_range_values() {
        assert!(Dpi::new(150).is_ok());
        assert!(Dpi::new(9).is_err());
        assert!(Dpi::new(2401).is_err());
        assert!(serde_json::from_str::<Dpi>("5").is_err());
        assert_eq!(serde_json::from_str::<Dpi>("150").unwrap().get(), 150);
    }

    #[test]
    fn percent_and_millimeters_validate() {
        assert!(Percent::new(1000).is_ok());
        assert!(Percent::new(1001).is_err());
        assert!(Millimeters::new(10.0).is_ok());
        assert!(Millimeters::new(-1.0).is_err());
        assert!(Millimeters::new(f64::NAN).is_err());
        assert!((Millimeters::new(25.4).unwrap().to_points() - 72.0).abs() < 1e-9);
    }

    #[test]
    fn bytes_use_binary_megabytes() {
        assert_eq!(Bytes::from_mb(10).get(), 10 * 1024 * 1024);
        assert_eq!(Bytes::from_mb(u64::MAX).get(), u64::MAX);
    }
}
