//! Quirewise engine adapter: sign-cms.
//!
//! Digital signatures for PDF files, built on RustCrypto: PAdES (CMS) signatures that are
//! appended as a new revision, with optional time stamps and long-term validation data, and the
//! checks of such signatures.
// Only the Windows certificate store talks to Windows; it allows unsafe code for that call and
// nothing else does.
#![deny(unsafe_code)]
// Tests fail at once when something is wrong, so they may unwrap.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod acro;
mod algo;
mod certs;
mod cmsdoc;
mod dss;
#[cfg(test)]
mod engine_tests;
mod keys;
mod pdfrev;
mod pdfsign;
mod revocation;
mod sigfind;
mod sign;
mod store;
#[cfg(all(test, windows))]
mod store_tests;
mod timefmt;
mod trust;
mod tsa;
mod verify;

#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
#[cfg(any(test, feature = "testkit"))]
pub mod testweb;

use std::path::Path;

use application::AppError;
use application::runtime::CancellationToken;
use application::signing::{
    PdfSigner, SignOutcome, SignRequest, SignatureChecker, SignatureReport, StoredCertificate,
    VerifyRequest, WebClient,
};
use domain::ErrorCode;

use crate::certs::read_many;
use crate::trust::TrustStore;

/// Signs PDF files and checks their signatures.
#[derive(Debug, Clone)]
pub struct CmsEngine {
    system_roots: bool,
}

impl Default for CmsEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl CmsEngine {
    /// An engine that trusts the root certificates of Windows.
    pub fn new() -> Self {
        Self { system_roots: true }
    }

    /// An engine that trusts only the certificates it is given, for tests that must not depend
    /// on the machine.
    pub fn isolated() -> Self {
        Self {
            system_roots: false,
        }
    }

    fn trust(&self, extra: Option<&Path>) -> Result<TrustStore, AppError> {
        let mut store = if self.system_roots {
            TrustStore::system()
        } else {
            TrustStore::empty()
        };
        if let Some(path) = extra {
            let bytes = std::fs::read(path)
                .map_err(|e| AppError::from_io("read the file of trusted certificates", &e))?;
            let certs = read_many(&bytes);
            if certs.is_empty() {
                return Err(AppError::new(
                    ErrorCode::OptionInvalid,
                    "The file of trusted certificates holds no certificate. Choose a .cer, .crt or .pem file.",
                ));
            }
            store.add(certs);
        }
        Ok(store)
    }
}

impl PdfSigner for CmsEngine {
    fn sign(
        &self,
        input: &Path,
        dest: &Path,
        request: &SignRequest,
        web: Option<&dyn WebClient>,
        cancel: &CancellationToken,
    ) -> Result<SignOutcome, AppError> {
        sign::sign_file(input, dest, request, web, cancel)
    }

    fn certificates(&self) -> Result<Vec<StoredCertificate>, AppError> {
        store::list()
    }
}

impl SignatureChecker for CmsEngine {
    fn check(
        &self,
        input: &Path,
        request: &VerifyRequest,
        web: Option<&dyn WebClient>,
        cancel: &CancellationToken,
    ) -> Result<Vec<SignatureReport>, AppError> {
        let bytes = std::fs::read(input).map_err(|e| AppError::from_io("read the PDF", &e))?;
        let trust = self.trust(request.trusted_certificates.as_deref())?;
        let setup = verify::Setup {
            trust: &trust,
            revocation: if request.check_revocation { web } else { None },
            now: request.now,
        };
        verify::check_all(&bytes, &setup, cancel)
    }
}
