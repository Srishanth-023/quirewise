//! Safety limits applied to every job (MASTER_PROMPT.md section 12).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::units::Bytes;

/// Caps that bound what a hostile document can cost. Enforced in the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SafetyLimits {
    /// Memory cap for the worker Job Object. Default 2 GB.
    pub max_memory: Bytes,
    /// CPU time cap for the worker Job Object, in seconds. Default 600.
    pub max_cpu_seconds: u32,
    /// Wall-clock timeout per job, in seconds. Default 900.
    pub job_timeout_seconds: u32,
    /// Largest accepted input file. Default 2 GB.
    pub max_input_size: Bytes,
    /// Largest accepted page count. Default 20,000.
    pub max_pages: u32,
    /// Largest accepted PDF object count. Default 5,000,000.
    pub max_objects: u32,
    /// Largest accepted decompression ratio for a stream. Default 200.
    pub max_decompression_ratio: u32,
}

impl Default for SafetyLimits {
    fn default() -> Self {
        Self {
            max_memory: Bytes::new(2 * Bytes::GB),
            max_cpu_seconds: 600,
            job_timeout_seconds: 900,
            max_input_size: Bytes::new(2 * Bytes::GB),
            max_pages: 20_000,
            max_objects: 5_000_000,
            max_decompression_ratio: 200,
        }
    }
}

impl SafetyLimits {
    /// Rejects values that would disable a cap.
    pub fn validate(&self) -> Result<(), DomainError> {
        let positive = |field: &str, ok: bool| {
            if ok {
                Ok(())
            } else {
                Err(DomainError::option(field, "must be greater than zero"))
            }
        };
        positive("max_memory", self.max_memory.get() > 0)?;
        positive("max_cpu_seconds", self.max_cpu_seconds > 0)?;
        positive("job_timeout_seconds", self.job_timeout_seconds > 0)?;
        positive("max_input_size", self.max_input_size.get() > 0)?;
        positive("max_pages", self.max_pages > 0)?;
        positive("max_objects", self.max_objects > 0)?;
        positive("max_decompression_ratio", self.max_decompression_ratio > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_spec_and_validate() {
        let d = SafetyLimits::default();
        assert_eq!(d.max_memory.get(), 2 * Bytes::GB);
        assert!(d.validate().is_ok());
    }

    #[test]
    fn zero_caps_are_rejected() {
        let l = SafetyLimits {
            max_pages: 0,
            ..SafetyLimits::default()
        };
        assert!(l.validate().is_err());
    }
}
