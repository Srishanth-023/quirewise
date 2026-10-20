//! Digital signatures: the ports an engine implements and the reports it hands back.
//!
//! Signing adds a revision to the end of the file, so the earlier bytes stay as they were and
//! earlier signatures stay valid. Keys never leave the engine: the application only names where
//! a key comes from.

use std::path::{Path, PathBuf};

use domain::secret::Secret;
use domain::tools::signing::{Certify, SignLevel};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::runtime::CancellationToken;

/// Where the signing key comes from.
#[derive(Debug, Clone)]
pub enum KeyChoice {
    /// A PFX or P12 file. The password is used once and not kept.
    Pfx {
        /// The file.
        path: PathBuf,
        /// Its password.
        password: Secret,
    },
    /// A certificate of the Windows certificate store, by its SHA-1 thumbprint (40 upper case hex
    /// digits).
    Store {
        /// The thumbprint.
        thumbprint: String,
    },
}

/// Where a visible signature is drawn, as shares of the page with the origin at the top left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignatureBox {
    /// Page, counted from 0.
    pub page: u32,
    /// Left edge.
    pub left: f32,
    /// Top edge.
    pub top: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

/// Everything needed to sign one file.
#[derive(Debug, Clone)]
pub struct SignRequest {
    /// The key.
    pub key: KeyChoice,
    /// An empty signature field to fill, instead of adding a field.
    pub field: Option<String>,
    /// Where to draw the signature, `None` for an invisible one.
    pub visible: Option<SignatureBox>,
    /// Name shown instead of the one of the certificate.
    pub signer_name: Option<String>,
    /// Why it is signed.
    pub reason: Option<String>,
    /// Where it is signed.
    pub location: Option<String>,
    /// How to reach the signer.
    pub contact: Option<String>,
    /// Evidence stored with the signature.
    pub level: SignLevel,
    /// Time stamp server for levels above `b-b`.
    pub timestamp_url: Option<String>,
    /// Limit on later changes.
    pub certify: Certify,
    /// The signing time, in seconds since 1970 UTC.
    pub signed_at: i64,
}

/// What a signing run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignOutcome {
    /// Name of the signature field.
    pub field: String,
    /// Who signed, as the certificate names them.
    pub signer: String,
    /// Things the person should know, for example that some revocation answers were missing.
    pub warnings: Vec<String>,
}

/// A certificate the person can sign with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StoredCertificate {
    /// SHA-1 thumbprint, 40 upper case hex digits.
    pub thumbprint: String,
    /// Who the certificate names.
    pub subject: String,
    /// Who issued it.
    pub issuer: String,
    /// Last day it is valid, `YYYY-MM-DD` in UTC.
    pub valid_until: String,
}

/// Fetches answers from servers that sign things: time stamps and revocation status. Only
/// called after the person agreed to the connection.
pub trait WebClient: Send + Sync {
    /// Reads the body of `url`. Fails when it is larger than `max_bytes`.
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, AppError>;

    /// Sends `body` to `url` and reads the answer. Fails when it is larger than `max_bytes`.
    fn post(
        &self,
        url: &str,
        content_type: &str,
        body: &[u8],
        max_bytes: u64,
    ) -> Result<Vec<u8>, AppError>;
}

/// Signs PDF files with a CMS (PAdES) signature.
pub trait PdfSigner: Send + Sync {
    /// Writes `dest` as `input` plus a signed revision. `web` is `Some` only when the person
    /// agreed to use the internet.
    fn sign(
        &self,
        input: &Path,
        dest: &Path,
        request: &SignRequest,
        web: Option<&dyn WebClient>,
        cancel: &CancellationToken,
    ) -> Result<SignOutcome, AppError>;

    /// The certificates in the Windows store that hold a private key.
    fn certificates(&self) -> Result<Vec<StoredCertificate>, AppError>;
}

/// The answer to one question about a signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// The check passed.
    Pass,
    /// The check failed: the signature should not be relied on.
    Fail,
    /// The check could not be made or has no clear answer.
    Unknown,
}

/// One check with a sentence that says what was found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Check {
    /// The answer.
    pub status: Status,
    /// What was found, in plain words.
    pub detail: String,
}

impl Check {
    /// A passed check.
    pub fn pass(detail: impl Into<String>) -> Self {
        Self {
            status: Status::Pass,
            detail: detail.into(),
        }
    }

    /// A failed check.
    pub fn fail(detail: impl Into<String>) -> Self {
        Self {
            status: Status::Fail,
            detail: detail.into(),
        }
    }

    /// A check without a clear answer.
    pub fn unknown(detail: impl Into<String>) -> Self {
        Self {
            status: Status::Unknown,
            detail: detail.into(),
        }
    }
}

/// What kind of signature it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SignatureKind {
    /// A signature that approves the document.
    Approval,
    /// A certifying signature that limits later changes (DocMDP permission 1 to 3).
    Certification(u8),
    /// A time stamp over the document.
    DocumentTimestamp,
}

/// The time stamp inside a signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TimestampReport {
    /// When the time stamp server says the signature existed, `YYYY-MM-DD HH:MM:SS UTC`.
    pub time: String,
    /// Who issued it.
    pub authority: String,
    /// Whether the time stamp itself holds.
    pub check: Check,
}

/// What the checks found about one signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SignatureReport {
    /// Name of the signature field.
    pub field: String,
    /// Kind of signature.
    pub kind: SignatureKind,
    /// Who signed, as the certificate names them.
    pub signer: String,
    /// Who issued the certificate of the signer.
    pub issuer: String,
    /// Time the signer claims, `YYYY-MM-DD HH:MM:SS UTC`.
    pub signed_at: Option<String>,
    /// Reason given by the signer.
    pub reason: Option<String>,
    /// Place given by the signer.
    pub location: Option<String>,
    /// Contact given by the signer.
    pub contact: Option<String>,
    /// The kind of key and hash, for example `ECDSA P-256 with SHA-256`.
    pub algorithm: String,
    /// Whether the signed bytes and the signature match.
    pub signature: Check,
    /// Whether the file changed after signing.
    pub changes: Check,
    /// Whether the certificate chain leads to a trusted root.
    pub identity: Check,
    /// Whether the certificates were revoked.
    pub revocation: Check,
    /// The time stamp, when the signature has one.
    pub timestamp: Option<TimestampReport>,
    /// True when the document holds revocation answers for later checks (a long-term signature).
    pub long_term: bool,
    /// Other things worth knowing, for example a weak algorithm.
    pub notes: Vec<String>,
}

impl SignatureReport {
    /// True when the signature and the file are intact and the identity is trusted.
    pub fn is_trusted(&self) -> bool {
        self.signature.status == Status::Pass
            && self.changes.status == Status::Pass
            && self.identity.status == Status::Pass
            && self.revocation.status != Status::Fail
    }

    /// True when something failed.
    pub fn has_failure(&self) -> bool {
        [
            &self.signature,
            &self.changes,
            &self.identity,
            &self.revocation,
        ]
        .iter()
        .any(|c| c.status == Status::Fail)
            || self
                .timestamp
                .as_ref()
                .is_some_and(|t| t.check.status == Status::Fail)
    }
}

/// How to check the signatures of a file.
#[derive(Debug, Clone)]
pub struct VerifyRequest {
    /// Ask the certificate authorities about revocation. `web` must be `Some`.
    pub check_revocation: bool,
    /// A file of extra trusted certificates (PEM or DER).
    pub trusted_certificates: Option<PathBuf>,
    /// The time to check validity against, in seconds since 1970 UTC.
    pub now: i64,
}

/// Checks the signatures of PDF files.
pub trait SignatureChecker: Send + Sync {
    /// Reports on every signature of `input`, in the order they were added.
    fn check(
        &self,
        input: &Path,
        request: &VerifyRequest,
        web: Option<&dyn WebClient>,
        cancel: &CancellationToken,
    ) -> Result<Vec<SignatureReport>, AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> SignatureReport {
        SignatureReport {
            field: "Signature1".into(),
            kind: SignatureKind::Approval,
            signer: "A".into(),
            issuer: "B".into(),
            signed_at: None,
            reason: None,
            location: None,
            contact: None,
            algorithm: "x".into(),
            signature: Check::pass("ok"),
            changes: Check::pass("none"),
            identity: Check::pass("trusted"),
            revocation: Check::unknown("not checked"),
            timestamp: None,
            long_term: false,
            notes: Vec::new(),
        }
    }

    #[test]
    fn a_trusted_signature_needs_every_main_check() {
        let mut r = report();
        assert!(r.is_trusted() && !r.has_failure());
        r.identity = Check::unknown("self-signed");
        assert!(!r.is_trusted() && !r.has_failure());
        r.identity = Check::pass("trusted");
        r.changes = Check::unknown("form fields were added");
        assert!(!r.is_trusted() && !r.has_failure());
        r.changes = Check::fail("changed");
        assert!(!r.is_trusted() && r.has_failure());
        r.changes = Check::pass("none");
        r.timestamp = Some(TimestampReport {
            time: "t".into(),
            authority: "a".into(),
            check: Check::fail("bad"),
        });
        assert!(r.has_failure());
    }
}
