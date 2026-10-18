//! Options of the Security tools: Protect (row 51), Unlock (row 52) and Sanitize (row 63).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::secret::Secret;

/// What a person who opens a protected file may do. Everything is allowed by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Permissions {
    /// Print the document.
    pub print: bool,
    /// Copy text and images out of it.
    pub copy: bool,
    /// Change the document.
    pub modify: bool,
    /// Add or change comments and annotations.
    pub annotate: bool,
    /// Fill in form fields.
    pub fill_forms: bool,
    /// Insert, rotate, delete and reorder pages.
    pub assemble: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        Self {
            print: true,
            copy: true,
            modify: true,
            annotate: true,
            fill_forms: true,
            assemble: true,
        }
    }
}

impl Permissions {
    /// True when at least one action is forbidden.
    pub const fn restricts(&self) -> bool {
        !(self.print
            && self.copy
            && self.modify
            && self.annotate
            && self.fill_forms
            && self.assemble)
    }
}

/// The encryption algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Cipher {
    /// AES with a 256-bit key (default). Needs a PDF 2.0 capable viewer or Acrobat 9 or newer.
    #[default]
    Aes256,
    /// AES with a 128-bit key, for very old viewers.
    Aes128,
}

/// Options for Protect.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ProtectOptions {
    /// Password needed to open the file. Leave empty to let anyone open it.
    pub user_password: Option<Secret>,
    /// Password that lifts the restrictions. Required when any permission is turned off.
    pub owner_password: Option<Secret>,
    /// Algorithm. Default AES-256.
    pub cipher: Cipher,
    /// What opening the file allows. Default everything.
    pub permissions: Permissions,
}

/// Protect options that passed the checks, with every password decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectPlan {
    /// Password to open the file (may be empty).
    pub user: Secret,
    /// Password that lifts the restrictions.
    pub owner: Secret,
    /// Algorithm.
    pub cipher: Cipher,
    /// What opening the file allows.
    pub permissions: Permissions,
}

fn text(secret: &Option<Secret>) -> &str {
    secret.as_ref().map_or("", Secret::expose)
}

impl ProtectOptions {
    /// Checks the combination of passwords and permissions.
    ///
    /// Restrictions only hold when the owner password differs from the open password, because a
    /// person who knows the open password and the owner password is the same is the owner.
    pub fn plan(&self) -> Result<ProtectPlan, DomainError> {
        let user = text(&self.user_password);
        let owner = text(&self.owner_password);
        if user.is_empty() && owner.is_empty() {
            return Err(DomainError::option(
                "user_password",
                "enter a password to open the file, an owner password, or both",
            ));
        }
        let restricts = self.permissions.restricts();
        if restricts && owner.is_empty() {
            return Err(DomainError::option(
                "owner_password",
                "restrictions need an owner password, otherwise anyone could lift them",
            ));
        }
        if restricts && owner == user {
            return Err(DomainError::option(
                "owner_password",
                "use an owner password that differs from the open password, otherwise the restrictions can be lifted",
            ));
        }
        Ok(ProtectPlan {
            user: Secret::new(user),
            // Without an owner password the open password also owns the file.
            owner: Secret::new(if owner.is_empty() { user } else { owner }),
            cipher: self.cipher,
            permissions: self.permissions,
        })
    }
}

/// Options for Unlock. The password comes with the input file, so there is nothing to set.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct UnlockOptions {}

/// Options for Sanitize. Everything is removed by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SanitizeOptions {
    /// Remove the document information (title, author, dates).
    pub metadata: bool,
    /// Remove the XMP metadata packets.
    pub xmp: bool,
    /// Remove JavaScript and automatic actions.
    pub javascript: bool,
    /// Remove attached files.
    pub embedded_files: bool,
    /// Remove layers that are hidden, together with their content.
    pub hidden_layers: bool,
    /// Remove actions of form fields and links that run or open something.
    pub form_actions: bool,
    /// Remove page thumbnails.
    pub thumbnails: bool,
    /// Remove private data that applications stored in the file.
    pub private_data: bool,
}

impl Default for SanitizeOptions {
    fn default() -> Self {
        Self {
            metadata: true,
            xmp: true,
            javascript: true,
            embedded_files: true,
            hidden_layers: true,
            form_actions: true,
            thumbnails: true,
            private_data: true,
        }
    }
}

impl SanitizeOptions {
    /// Rejects a run that would remove nothing.
    pub fn validate(&self) -> Result<(), DomainError> {
        let any = self.metadata
            || self.xmp
            || self.javascript
            || self.embedded_files
            || self.hidden_layers
            || self.form_actions
            || self.thumbnails
            || self.private_data;
        if any {
            Ok(())
        } else {
            Err(DomainError::option(
                "metadata",
                "turn on at least one thing to remove",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn secret(s: &str) -> Option<Secret> {
        Some(Secret::new(s))
    }

    #[test]
    fn a_password_is_required() {
        let err = ProtectOptions::default().plan().unwrap_err();
        assert_eq!(err.code(), ErrorCode::OptionInvalid);
    }

    #[test]
    fn the_open_password_owns_the_file_when_nothing_is_restricted() {
        let plan = ProtectOptions {
            user_password: secret("open"),
            ..ProtectOptions::default()
        }
        .plan()
        .unwrap();
        assert_eq!((plan.user.expose(), plan.owner.expose()), ("open", "open"));
        assert_eq!(plan.cipher, Cipher::Aes256);
    }

    #[test]
    fn restrictions_need_a_different_owner_password() {
        let restricted = Permissions {
            copy: false,
            ..Permissions::default()
        };
        let base = ProtectOptions {
            user_password: secret("open"),
            permissions: restricted,
            ..ProtectOptions::default()
        };
        assert!(base.plan().is_err(), "no owner password");
        let same = ProtectOptions {
            owner_password: secret("open"),
            ..base.clone()
        };
        assert!(same.plan().is_err(), "owner equals open password");
        let ok = ProtectOptions {
            owner_password: secret("boss"),
            ..base
        };
        assert_eq!(ok.plan().unwrap().owner.expose(), "boss");
    }

    #[test]
    fn an_owner_password_alone_lets_anyone_open_but_not_change() {
        let plan = ProtectOptions {
            owner_password: secret("boss"),
            permissions: Permissions {
                modify: false,
                ..Permissions::default()
            },
            ..ProtectOptions::default()
        }
        .plan()
        .unwrap();
        assert!(plan.user.is_empty());
        assert!(plan.permissions.restricts());
    }

    #[test]
    fn debug_output_never_contains_a_password() {
        let o = ProtectOptions {
            user_password: secret("sup3rsecret"),
            ..ProtectOptions::default()
        };
        assert!(!format!("{o:?}").contains("sup3rsecret"));
    }

    #[test]
    fn sanitize_defaults_remove_everything_and_nothing_selected_is_rejected() {
        assert!(SanitizeOptions::default().validate().is_ok());
        let none = SanitizeOptions {
            metadata: false,
            xmp: false,
            javascript: false,
            embedded_files: false,
            hidden_layers: false,
            form_actions: false,
            thumbnails: false,
            private_data: false,
        };
        assert!(none.validate().is_err());
    }
}
