//! Options of the Optimize tools: Compress (row 14), Repair (row 15) and Linearize (row 64).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// How hard Compress works. Presets set the image quality and resolution; the advanced options
/// override them one by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CompressPreset {
    /// Smallest file: JPEG quality 40, images reduced to 72 ppi.
    Extreme,
    /// Good balance (default): JPEG quality 65, images reduced to 144 ppi.
    #[default]
    Recommended,
    /// Best quality: JPEG quality 80, images reduced to 288 ppi.
    Less,
}

/// The highest resolution images keep, in pixels per inch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ImagePpi {
    /// 72 ppi, screen only.
    #[serde(rename = "72")]
    Ppi72,
    /// 144 ppi, good on screen and acceptable in print.
    #[serde(rename = "144")]
    Ppi144,
    /// 288 ppi, print quality.
    #[serde(rename = "288")]
    Ppi288,
    /// 720 ppi, keeps almost everything.
    #[serde(rename = "720")]
    Ppi720,
}

impl ImagePpi {
    /// The resolution in pixels per inch.
    pub const fn value(self) -> u32 {
        match self {
            Self::Ppi72 => 72,
            Self::Ppi144 => 144,
            Self::Ppi288 => 288,
            Self::Ppi720 => 720,
        }
    }
}

/// Options for Compress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CompressOptions {
    /// Starting point for quality and resolution. Default recommended.
    pub preset: CompressPreset,
    /// JPEG quality 1 to 100, replacing the preset's value. Default none.
    pub jpeg_quality: Option<u8>,
    /// Highest image resolution, replacing the preset's value. Default none.
    pub max_image_ppi: Option<ImagePpi>,
    /// Turn color images gray. Default off.
    pub grayscale: bool,
    /// Remove the document information and XMP metadata. Default off.
    pub strip_metadata: bool,
    /// Pack objects into compressed object streams. Default on.
    pub object_streams: bool,
}

impl Default for CompressOptions {
    fn default() -> Self {
        Self {
            preset: CompressPreset::Recommended,
            jpeg_quality: None,
            max_image_ppi: None,
            grayscale: false,
            strip_metadata: false,
            object_streams: true,
        }
    }
}

/// The settings Compress works with once presets and overrides are combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompressSettings {
    /// JPEG quality 1 to 100.
    pub jpeg_quality: u8,
    /// Images above this resolution are reduced to it.
    pub max_ppi: u32,
    /// Convert color images to gray.
    pub grayscale: bool,
}

impl CompressOptions {
    /// Combines the preset with the overrides and checks them.
    pub fn settings(&self) -> Result<CompressSettings, DomainError> {
        if let Some(q) = self.jpeg_quality
            && !(1..=100).contains(&q)
        {
            return Err(DomainError::option(
                "jpeg_quality",
                "must be between 1 and 100",
            ));
        }
        let (quality, ppi) = match self.preset {
            CompressPreset::Extreme => (40, 72),
            CompressPreset::Recommended => (65, 144),
            CompressPreset::Less => (80, 288),
        };
        Ok(CompressSettings {
            jpeg_quality: self.jpeg_quality.unwrap_or(quality),
            max_ppi: self.max_image_ppi.map_or(ppi, ImagePpi::value),
            grayscale: self.grayscale,
        })
    }
}

/// Options for Repair. It needs none: it always tries the gentlest fix first.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RepairOptions {}

/// Options for Linearize (fast web view).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct LinearizeOptions {
    /// Write the file for fast web view (default). Off writes a normal, non-linearized copy.
    pub enabled: bool,
}

impl Default for LinearizeOptions {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn presets_match_appendix_a() {
        let s = |preset| {
            CompressOptions {
                preset,
                ..CompressOptions::default()
            }
            .settings()
            .unwrap()
        };
        assert_eq!(
            (
                s(CompressPreset::Recommended).jpeg_quality,
                s(CompressPreset::Recommended).max_ppi
            ),
            (65, 144)
        );
        assert_eq!(
            (
                s(CompressPreset::Extreme).jpeg_quality,
                s(CompressPreset::Extreme).max_ppi
            ),
            (40, 72)
        );
        assert_eq!(
            (
                s(CompressPreset::Less).jpeg_quality,
                s(CompressPreset::Less).max_ppi
            ),
            (80, 288)
        );
    }

    #[test]
    fn advanced_options_override_the_preset() {
        let o = CompressOptions {
            jpeg_quality: Some(90),
            max_image_ppi: Some(ImagePpi::Ppi720),
            grayscale: true,
            ..CompressOptions::default()
        };
        let s = o.settings().unwrap();
        assert_eq!((s.jpeg_quality, s.max_ppi, s.grayscale), (90, 720, true));
    }

    #[test]
    fn quality_outside_one_to_hundred_is_rejected() {
        for q in [0u8, 101] {
            let o = CompressOptions {
                jpeg_quality: Some(q),
                ..CompressOptions::default()
            };
            assert_eq!(o.settings().unwrap_err().code(), ErrorCode::OptionInvalid);
        }
    }

    #[test]
    fn json_uses_the_documented_names() {
        let o: CompressOptions =
            serde_json::from_str(r#"{"preset":"extreme","max_image_ppi":"288"}"#).unwrap();
        assert_eq!(o.preset, CompressPreset::Extreme);
        assert_eq!(o.max_image_ppi, Some(ImagePpi::Ppi288));
        assert!(o.object_streams);
        assert!(serde_json::from_str::<CompressOptions>(r#"{"max_image_ppi":"100"}"#).is_err());
        assert!(LinearizeOptions::default().enabled);
        assert!(serde_json::from_str::<RepairOptions>(r#"{"x":1}"#).is_err());
    }
}
