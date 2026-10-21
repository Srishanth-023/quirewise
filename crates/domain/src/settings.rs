//! Versioned user settings. Persistence and migrations live in infrastructure.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::explorer::{DEFAULT_MENU_TOOLS, MAX_MENU_TOOLS, offered_in_menu};
use crate::limits::SafetyLimits;
use crate::naming::OutputNamingPattern;
use crate::tools::ToolId;

/// Current settings schema version. Bump it and add a migration when the shape changes.
pub const CURRENT_SETTINGS_VERSION: u32 = 1;

/// Largest value of `max_heavy_jobs`.
pub const MAX_HEAVY_JOBS: u8 = 4;

/// Theme choice. `System` follows the Windows app theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ThemePreference {
    /// Follow Windows (default).
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// User preferences stored in `settings.toml`. Never holds passwords or keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Schema version of this file.
    pub version: u32,
    /// Theme choice.
    pub theme: ThemePreference,
    /// Default output naming pattern.
    pub naming_pattern: OutputNamingPattern,
    /// Concurrent heavy jobs, 1 to 4. `None` means physical cores minus one, capped at 4.
    pub max_heavy_jobs: Option<u8>,
    /// Keep a local list of recently opened files.
    pub remember_recent_files: bool,
    /// Safety limits for the worker.
    pub limits: SafetyLimits,
    /// The tools the Explorer right-click menu offers, when the person turns that menu on.
    pub explorer_tools: Vec<ToolId>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: CURRENT_SETTINGS_VERSION,
            theme: ThemePreference::default(),
            naming_pattern: OutputNamingPattern::default(),
            max_heavy_jobs: None,
            remember_recent_files: true,
            limits: SafetyLimits::default(),
            explorer_tools: DEFAULT_MENU_TOOLS.to_vec(),
        }
    }
}

impl Settings {
    /// Checks cross-field rules that the types alone cannot express.
    pub fn validate(&self) -> Result<(), DomainError> {
        if let Some(n) = self.max_heavy_jobs
            && !(1..=MAX_HEAVY_JOBS).contains(&n)
        {
            return Err(DomainError::option(
                "max_heavy_jobs",
                format!("must be between 1 and {MAX_HEAVY_JOBS}"),
            ));
        }
        if self.explorer_tools.len() > MAX_MENU_TOOLS
            || self.explorer_tools.iter().any(|t| !offered_in_menu(*t))
        {
            return Err(DomainError::option(
                "explorer_tools",
                format!("choose up to {MAX_MENU_TOOLS} tools that can start from a right-click"),
            ));
        }
        self.limits.validate()
    }

    /// Resolves the heavy-job concurrency for a machine with `physical_cores` cores.
    pub fn heavy_job_slots(&self, physical_cores: usize) -> usize {
        match self.max_heavy_jobs {
            Some(n) => usize::from(n),
            None => physical_cores
                .saturating_sub(1)
                .clamp(1, usize::from(MAX_HEAVY_JOBS)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_validate() {
        let s = Settings::default();
        assert_eq!(s.version, CURRENT_SETTINGS_VERSION);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn heavy_job_slots_follow_cores_minus_one_capped_at_four() {
        let s = Settings::default();
        assert_eq!(s.heavy_job_slots(1), 1);
        assert_eq!(s.heavy_job_slots(4), 3);
        assert_eq!(s.heavy_job_slots(16), 4);
        let fixed = Settings {
            max_heavy_jobs: Some(2),
            ..Settings::default()
        };
        assert_eq!(fixed.heavy_job_slots(16), 2);
    }

    #[test]
    fn the_explorer_menu_defaults_are_valid_and_too_many_or_unfit_tools_are_not() {
        assert_eq!(Settings::default().explorer_tools.len(), MAX_MENU_TOOLS);
        let mut s = Settings::default();
        s.explorer_tools.push(ToolId::Flatten);
        assert!(s.validate().is_err());
        let s = Settings {
            explorer_tools: vec![ToolId::EditPdf],
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn out_of_range_concurrency_is_rejected() {
        for bad in [0u8, 5] {
            let s = Settings {
                max_heavy_jobs: Some(bad),
                ..Settings::default()
            };
            assert!(s.validate().is_err());
        }
    }
}
