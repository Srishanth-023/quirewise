//! Options of the digital signature tools: Digital signature (row 61) and Verify signatures
//! (row 62).
//!
//! A digital signature proves who signed and that nothing changed afterwards. It is not the
//! drawn signature of Sign PDF (row 53), which is only a picture.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::secret::Secret;

/// Longest reason, place, contact or name, in characters.
pub const MAX_TEXT: usize = 200;
/// Smallest side of a visible signature box, as a share of the page.
pub const MIN_BOX_SIDE: f32 = 0.02;

/// Where the signing key comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum KeySource {
    /// A PFX or P12 file with a password (default). The key is read into memory only.
    #[default]
    Pfx,
    /// A certificate of the Windows certificate store, found by its thumbprint. The key stays
    /// in Windows, so keys on smart cards and tokens work.
    WindowsStore,
}

/// How much evidence is stored with the signature (the PAdES levels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub enum SignLevel {
    /// The signature and the certificates of the signer (default). Works offline.
    #[default]
    #[serde(rename = "b-b")]
    BB,
    /// B-B plus a trusted time stamp from a time stamp server.
    #[serde(rename = "b-t")]
    BT,
    /// B-T plus the revocation answers of the certificate chain, so the signature can be checked
    /// later without the internet.
    #[serde(rename = "b-lt")]
    BLt,
    /// B-LT plus a time stamp over the whole file, for checks that must hold for many years.
    #[serde(rename = "b-lta")]
    BLta,
}

impl SignLevel {
    /// True when a time stamp is added to the signature.
    pub const fn timestamped(self) -> bool {
        !matches!(self, Self::BB)
    }

    /// True when revocation answers are stored in the file.
    pub const fn long_term(self) -> bool {
        matches!(self, Self::BLt | Self::BLta)
    }

    /// True when the file ends with a time stamp over everything before it.
    pub const fn archival(self) -> bool {
        matches!(self, Self::BLta)
    }
}

/// What later changes a certifying signature allows (DocMDP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Certify {
    /// An ordinary approval signature that does not limit later changes (default).
    #[default]
    None,
    /// Certify the document: no changes are allowed afterwards.
    NoChanges,
    /// Certify the document: only filling in form fields and signing are allowed.
    FormFill,
    /// Certify the document: filling in form fields, signing and comments are allowed.
    Annotate,
}

impl Certify {
    /// The DocMDP permission number of the PDF specification, `None` for an approval signature.
    pub const fn permission(self) -> Option<u8> {
        match self {
            Self::None => Option::None,
            Self::NoChanges => Some(1),
            Self::FormFill => Some(2),
            Self::Annotate => Some(3),
        }
    }
}

/// Options for Digital signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DigitalSignOptions {
    /// Where the key comes from. Default a PFX or P12 file.
    pub source: KeySource,
    /// The PFX or P12 file, for `pfx`.
    pub pfx_path: Option<PathBuf>,
    /// Password of the PFX or P12 file. Kept in memory only.
    pub pfx_password: Option<Secret>,
    /// SHA-1 thumbprint of the certificate (40 hex digits, spaces allowed), for `windows-store`.
    pub thumbprint: String,
    /// Name of an empty signature field to sign. Default none: a new field is added.
    pub field: Option<String>,
    /// Show the signature on a page. Default off: the signature is invisible.
    pub visible: bool,
    /// Page of the visible signature, counted from 1. Default 1.
    pub page: u32,
    /// Left edge of the visible signature as a share of the page width, 0 to 1. Default 0.6.
    pub left: f32,
    /// Top edge as a share of the page height, 0 to 1. Default 0.86.
    pub top: f32,
    /// Width as a share of the page width. Default 0.35.
    pub width: f32,
    /// Height as a share of the page height. Default 0.1.
    pub height: f32,
    /// Name to show instead of the one in the certificate.
    pub signer_name: Option<String>,
    /// Why the document is signed.
    pub reason: Option<String>,
    /// Where it is signed.
    pub location: Option<String>,
    /// How to reach the signer.
    pub contact: Option<String>,
    /// Evidence stored with the signature. Default `b-b`.
    pub level: SignLevel,
    /// Address of the time stamp server, for `b-t` and above.
    pub timestamp_url: Option<String>,
    /// Allow the connection to the time stamp server and the revocation servers. Default off.
    pub allow_network: bool,
    /// Certify the document and limit later changes. Default off.
    pub certify: Certify,
}

impl Default for DigitalSignOptions {
    fn default() -> Self {
        Self {
            source: KeySource::Pfx,
            pfx_path: None,
            pfx_password: None,
            thumbprint: String::new(),
            field: None,
            visible: false,
            page: 1,
            left: 0.6,
            top: 0.86,
            width: 0.35,
            height: 0.1,
            signer_name: None,
            reason: None,
            location: None,
            contact: None,
            level: SignLevel::BB,
            timestamp_url: None,
            allow_network: false,
            certify: Certify::None,
        }
    }
}

fn check_text(field: &str, value: &Option<String>) -> Result<(), DomainError> {
    let Some(text) = value else { return Ok(()) };
    if text.chars().count() > MAX_TEXT {
        return Err(DomainError::option(
            field,
            format!("use at most {MAX_TEXT} characters"),
        ));
    }
    if text.chars().any(char::is_control) {
        return Err(DomainError::option(field, "use one line of plain text"));
    }
    Ok(())
}

/// True for an `http` or `https` address with a host.
pub fn is_web_address(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"));
    rest.is_some_and(|r| {
        !r.is_empty() && !r.starts_with('/') && !text.chars().any(|c| c.is_whitespace())
    })
}

impl DigitalSignOptions {
    /// The thumbprint as 40 upper case hex digits, or an error when it is not one.
    pub fn thumbprint_hex(&self) -> Result<String, DomainError> {
        let hex: String = self
            .thumbprint
            .chars()
            .filter(|c| !c.is_whitespace() && *c != ':')
            .map(|c| c.to_ascii_uppercase())
            .collect();
        if hex.len() != 40 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(DomainError::option(
                "thumbprint",
                "enter the 40 hex digits of the certificate thumbprint",
            ));
        }
        Ok(hex)
    }

    /// Checks the combination of options.
    pub fn validate(&self) -> Result<(), DomainError> {
        match self.source {
            KeySource::Pfx => {
                if self.pfx_path.is_none() {
                    return Err(DomainError::option(
                        "pfx_path",
                        "choose the PFX or P12 file that holds your certificate",
                    ));
                }
            }
            KeySource::WindowsStore => {
                self.thumbprint_hex()?;
            }
        }
        if self.visible {
            if self.page == 0 {
                return Err(DomainError::option("page", "pages count from 1"));
            }
            let sides = [self.left, self.top, self.width, self.height];
            if sides.iter().any(|v| !v.is_finite()) {
                return Err(DomainError::option("left", "use numbers from 0 to 1"));
            }
            if self.left < 0.0
                || self.top < 0.0
                || self.width < MIN_BOX_SIDE
                || self.height < MIN_BOX_SIDE
                || self.left + self.width > 1.0001
                || self.top + self.height > 1.0001
            {
                return Err(DomainError::option(
                    "width",
                    "the signature box must lie on the page and be at least 2 percent wide and high",
                ));
            }
        }
        check_text("signer_name", &self.signer_name)?;
        check_text("reason", &self.reason)?;
        check_text("location", &self.location)?;
        check_text("contact", &self.contact)?;
        if let Some(name) = &self.field
            && (name.is_empty()
                || name.chars().count() > MAX_TEXT
                || name.chars().any(char::is_control))
        {
            return Err(DomainError::option("field", "use a field name of one line"));
        }
        if self.level.timestamped() {
            if !self.allow_network {
                return Err(DomainError::ConsentNeeded {
                    what: "Asking a time stamp server".into(),
                    option: "Allow the internet connection".into(),
                });
            }
            match &self.timestamp_url {
                Some(url) if is_web_address(url) => {}
                _ => {
                    return Err(DomainError::option(
                        "timestamp_url",
                        "enter the http or https address of a time stamp server",
                    ));
                }
            }
        } else if self.timestamp_url.is_some() {
            return Err(DomainError::option(
                "level",
                "choose level b-t or higher to add a time stamp",
            ));
        }
        Ok(())
    }
}

/// Options for Verify signatures.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct VerifySignaturesOptions {
    /// Ask the certificate authorities whether a certificate was revoked. Needs
    /// `allow_network`. Default off.
    pub check_revocation: bool,
    /// Allow the connection to the revocation servers named in the certificates. Default off.
    pub allow_network: bool,
    /// A file of certificates (PEM or DER) to trust besides the ones of Windows, for example the
    /// root certificate of your company.
    pub trusted_certificates: Option<PathBuf>,
}

impl VerifySignaturesOptions {
    /// Checks that a revocation check has the consent it needs.
    pub fn validate(&self) -> Result<(), DomainError> {
        if self.check_revocation && !self.allow_network {
            return Err(DomainError::ConsentNeeded {
                what: "Asking the certificate authorities whether a certificate was revoked".into(),
                option: "Allow the internet connection".into(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_file() -> DigitalSignOptions {
        DigitalSignOptions {
            pfx_path: Some(PathBuf::from("me.pfx")),
            ..DigitalSignOptions::default()
        }
    }

    #[test]
    fn defaults_match_the_matrix() {
        let o = DigitalSignOptions::default();
        assert_eq!(o.source, KeySource::Pfx);
        assert_eq!(o.level, SignLevel::BB);
        assert_eq!(o.certify, Certify::None);
        assert!(!o.visible && !o.allow_network);
        assert!(o.timestamp_url.is_none());
    }

    #[test]
    fn a_pfx_file_is_needed() {
        assert!(DigitalSignOptions::default().validate().is_err());
        assert!(with_file().validate().is_ok());
    }

    #[test]
    fn a_store_certificate_is_named_by_a_40_digit_thumbprint() {
        let mut o = DigitalSignOptions {
            source: KeySource::WindowsStore,
            ..DigitalSignOptions::default()
        };
        assert!(o.validate().is_err());
        o.thumbprint = "ab cd".into();
        assert!(o.validate().is_err());
        o.thumbprint = "0123456789abcdef0123456789abcdef01234567".into();
        assert!(o.validate().is_ok());
        assert_eq!(
            o.thumbprint_hex().unwrap(),
            "0123456789ABCDEF0123456789ABCDEF01234567"
        );
        o.thumbprint = "01:23:45:67:89:ab:cd:ef:01:23:45:67:89:ab:cd:ef:01:23:45:67".into();
        assert!(o.validate().is_ok());
    }

    #[test]
    fn a_visible_signature_must_fit_on_the_page() {
        let mut o = with_file();
        o.visible = true;
        assert!(o.validate().is_ok());
        o.left = 0.8;
        assert!(o.validate().is_err(), "0.8 + 0.35 leaves the page");
        o.left = 0.6;
        o.page = 0;
        assert!(o.validate().is_err());
        o.page = 1;
        o.height = 0.0;
        assert!(o.validate().is_err());
        o.height = f32::NAN;
        assert!(o.validate().is_err());
        // An invisible signature ignores the box.
        o.visible = false;
        assert!(o.validate().is_ok());
    }

    #[test]
    fn text_fields_are_one_short_line() {
        let mut o = with_file();
        o.reason = Some("I approve".into());
        assert!(o.validate().is_ok());
        o.reason = Some("two\nlines".into());
        assert!(o.validate().is_err());
        o.reason = Some("x".repeat(MAX_TEXT + 1));
        assert!(o.validate().is_err());
    }

    #[test]
    fn evidence_levels_need_consent_and_a_server() {
        let mut o = with_file();
        o.level = SignLevel::BT;
        assert!(matches!(
            o.validate(),
            Err(DomainError::ConsentNeeded { .. })
        ));
        o.allow_network = true;
        assert!(o.validate().is_err(), "no server given");
        o.timestamp_url = Some("not a url".into());
        assert!(o.validate().is_err());
        o.timestamp_url = Some("ftp://tsa.example".into());
        assert!(o.validate().is_err());
        o.timestamp_url = Some("https://tsa.example/ts".into());
        assert!(o.validate().is_ok());
        for level in [SignLevel::BLt, SignLevel::BLta] {
            o.level = level;
            assert!(o.validate().is_ok());
        }
    }

    #[test]
    fn a_server_without_a_level_is_refused() {
        let mut o = with_file();
        o.timestamp_url = Some("https://tsa.example/ts".into());
        assert!(o.validate().is_err());
    }

    #[test]
    fn level_flags_build_on_each_other() {
        assert!(!SignLevel::BB.timestamped());
        assert!(SignLevel::BT.timestamped() && !SignLevel::BT.long_term());
        assert!(SignLevel::BLt.long_term() && !SignLevel::BLt.archival());
        assert!(SignLevel::BLta.archival() && SignLevel::BLta.long_term());
    }

    #[test]
    fn certify_maps_to_the_docmdp_numbers() {
        assert_eq!(Certify::None.permission(), None);
        assert_eq!(Certify::NoChanges.permission(), Some(1));
        assert_eq!(Certify::FormFill.permission(), Some(2));
        assert_eq!(Certify::Annotate.permission(), Some(3));
    }

    #[test]
    fn levels_and_modes_use_their_wire_names() {
        let o: DigitalSignOptions = serde_json::from_str(
            r#"{"level":"b-lta","certify":"form-fill","source":"windows-store"}"#,
        )
        .unwrap();
        assert_eq!(o.level, SignLevel::BLta);
        assert_eq!(o.certify, Certify::FormFill);
        assert_eq!(o.source, KeySource::WindowsStore);
        assert!(serde_json::from_str::<DigitalSignOptions>(r#"{"nope":1}"#).is_err());
    }

    #[test]
    fn the_password_never_shows_in_debug_output() {
        let o = DigitalSignOptions {
            pfx_password: Some(Secret::new("hunter2")),
            ..with_file()
        };
        assert!(!format!("{o:?}").contains("hunter2"));
    }

    #[test]
    fn a_revocation_check_needs_consent() {
        let mut v = VerifySignaturesOptions::default();
        assert!(v.validate().is_ok());
        v.check_revocation = true;
        assert!(matches!(
            v.validate(),
            Err(DomainError::ConsentNeeded { .. })
        ));
        v.allow_network = true;
        assert!(v.validate().is_ok());
    }

    #[test]
    fn web_addresses_need_a_scheme_and_a_host() {
        assert!(is_web_address("https://tsa.example/x"));
        assert!(is_web_address("HTTP://tsa.example"));
        assert!(!is_web_address("https://"));
        assert!(!is_web_address("https:///x"));
        assert!(!is_web_address("tsa.example"));
        assert!(!is_web_address("https://a b"));
    }
}
