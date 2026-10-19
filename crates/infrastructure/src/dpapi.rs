//! Windows Data Protection (DPAPI): encrypts bytes so that only the same Windows user on the same
//! computer can read them. The key never leaves Windows and Quirewise never sees it.
#![allow(unsafe_code)]

use application::AppError;
use application::ports::SecretVault;
use domain::ErrorCode;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::core::w;

/// Mixed into the key, so another program of the same user cannot read the files by asking
/// DPAPI the same question without knowing it.
const ENTROPY: &[u8] = b"Quirewise saved signatures v1";

/// [`SecretVault`] over DPAPI, for the current Windows user.
#[derive(Debug, Clone, Copy, Default)]
pub struct Dpapi;

fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
        pbData: bytes.as_ptr().cast_mut(),
    }
}

fn failed(what: &str) -> AppError {
    AppError::new(
        ErrorCode::Internal,
        format!(
            "Windows could not {what} the saved signature. It may have been saved by another Windows user."
        ),
    )
}

/// Copies what Windows allocated and frees it.
fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if out.pbData.is_null() {
        return Vec::new();
    }
    // SAFETY: Windows returned `cbData` bytes at `pbData`, which stay valid until `LocalFree`.
    let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    // SAFETY: the buffer was allocated by Windows for this call and is freed once.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
    }
    bytes
}

impl SecretVault for Dpapi {
    fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, AppError> {
        let input = blob(plain);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: the pointers refer to live buffers for the whole call; the flags forbid any UI.
        unsafe {
            CryptProtectData(
                &input,
                w!("Quirewise"),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
        .map_err(|_| failed("protect"))?;
        Ok(take(out))
    }

    fn unprotect(&self, sealed: &[u8]) -> Result<Vec<u8>, AppError> {
        let input = blob(sealed);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as above.
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
        .map_err(|_| failed("open"))?;
        Ok(take(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_protected_is_unreadable_until_it_is_opened_again() {
        let secret = b"a signature drawn with a pen";
        let sealed = Dpapi.protect(secret).unwrap();
        assert_ne!(sealed, secret);
        assert!(!sealed.windows(secret.len()).any(|w| w == secret));
        assert_eq!(Dpapi.unprotect(&sealed).unwrap(), secret);
        assert!(Dpapi.unprotect(b"not sealed by windows").is_err());
        assert_eq!(Dpapi.unprotect(&Dpapi.protect(b"").unwrap()).unwrap(), b"");
    }
}
