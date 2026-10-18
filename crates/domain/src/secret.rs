//! A secret text value carried inside tool options, such as a password to set on a file.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Text that must never reach a log: `Debug` hides it. It serializes as a plain string because
/// the worker receives it over its private stdio pipe.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// Wraps a secret.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The secret text. Pass it straight to the engine; do not store or log it.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// True when there is no text.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_text_but_json_round_trips_it() {
        let s = Secret::new("hunter2");
        assert!(!format!("{s:?}").contains("hunter2"));
        assert_eq!(serde_json::to_string(&s).unwrap(), "\"hunter2\"");
        assert_eq!(
            serde_json::from_str::<Secret>("\"x\"").unwrap().expose(),
            "x"
        );
    }
}
