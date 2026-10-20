//! Certificates of the Windows certificate store, signed with through CNG.
//!
//! The private key never leaves Windows: the engine hands CNG a digest and gets a signature
//! back, so keys that cannot be exported, and keys on smart cards and tokens, work. Windows
//! asks for the PIN itself when the key needs one.

use application::AppError;
use application::signing::StoredCertificate;

use crate::keys::Identity;

/// The identity of the certificate with this thumbprint (40 upper case hex digits).
pub fn identity(thumbprint: &str) -> Result<Identity, AppError> {
    imp::identity(thumbprint)
}

/// The certificates of the personal store that have a private key.
pub fn list() -> Result<Vec<StoredCertificate>, AppError> {
    imp::list()
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use domain::ErrorCode;

    fn unavailable() -> AppError {
        AppError::new(
            ErrorCode::ComponentMissing,
            "The Windows certificate store is only available on Windows. Use a PFX or P12 file instead.",
        )
    }

    pub fn identity(_thumbprint: &str) -> Result<Identity, AppError> {
        Err(unavailable())
    }

    pub fn list() -> Result<Vec<StoredCertificate>, AppError> {
        Err(unavailable())
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
pub(crate) mod imp {
    use super::*;
    use std::ffi::c_void;
    use std::ptr;

    use domain::ErrorCode;
    use spki::SubjectPublicKeyInfoOwned;
    use windows::Win32::Security::Cryptography::{
        BCRYPT_PAD_PKCS1, BCRYPT_PKCS1_PADDING_INFO, CERT_CONTEXT, CERT_KEY_PROV_INFO_PROP_ID,
        CERT_KEY_SPEC, CERT_NCRYPT_KEY_SPEC, CRYPT_ACQUIRE_PREFER_NCRYPT_KEY_FLAG, CertCloseStore,
        CertDuplicateCertificateContext, CertEnumCertificatesInStore, CertFreeCertificateContext,
        CertGetCertificateContextProperty, CertOpenSystemStoreW, CryptAcquireCertificatePrivateKey,
        HCERTSTORE, HCRYPTPROV_OR_NCRYPT_KEY_HANDLE, NCRYPT_FLAGS, NCRYPT_KEY_HANDLE,
        NCryptFreeObject, NCryptSignHash,
    };
    use windows::core::{BOOL, PCWSTR, w};

    use crate::algo::{self, Hash, KeyKind};
    use crate::certs::Cert;
    use crate::keys::{SignKey, order_chain};
    use crate::timefmt;

    fn failed(what: &str) -> AppError {
        AppError::new(
            ErrorCode::OptionInvalid,
            format!(
                "Windows could not {what}. Choose another certificate, or use a PFX or P12 file."
            ),
        )
    }

    /// An open system store, closed on drop.
    struct Store(HCERTSTORE);

    impl Store {
        fn open(name: PCWSTR) -> Result<Self, AppError> {
            // SAFETY: `name` is a valid, nul-terminated wide string for the call; the result is
            // checked by the binding and closed in `drop`.
            let handle = unsafe { CertOpenSystemStoreW(None, name) }
                .map_err(|_| failed("open the certificate store"))?;
            Ok(Self(handle))
        }

        /// Every certificate of the store, as owned contexts.
        fn contexts(&self) -> Vec<Context> {
            let mut out = Vec::new();
            let mut previous: *const CERT_CONTEXT = ptr::null();
            loop {
                let prev = (!previous.is_null()).then_some(previous);
                // SAFETY: the store handle is open; `previous` is null or the context returned by
                // the last call, which this call frees.
                let next = unsafe { CertEnumCertificatesInStore(self.0, prev) };
                if next.is_null() {
                    break;
                }
                // SAFETY: `next` is a live context; the duplicate keeps it after enumeration
                // moves on.
                let kept = unsafe { CertDuplicateCertificateContext(Some(next)) };
                if !kept.is_null() {
                    out.push(Context(kept));
                }
                previous = next;
            }
            out
        }
    }

    impl Drop for Store {
        fn drop(&mut self) {
            // SAFETY: the handle came from `CertOpenSystemStoreW` and is closed once.
            let _ = unsafe { CertCloseStore(Some(self.0), 0) };
        }
    }

    /// A certificate context, released on drop.
    pub(crate) struct Context(*const CERT_CONTEXT);

    impl Context {
        /// Takes over a context that the caller owns.
        #[cfg(test)]
        pub(crate) fn from_raw(raw: *const CERT_CONTEXT) -> Self {
            Self(raw)
        }

        fn der(&self) -> Vec<u8> {
            // SAFETY: a live context points at `cbCertEncoded` bytes at `pbCertEncoded`.
            unsafe {
                let c = &*self.0;
                std::slice::from_raw_parts(c.pbCertEncoded, c.cbCertEncoded as usize).to_vec()
            }
        }

        fn has_private_key(&self) -> bool {
            let mut size = 0u32;
            // SAFETY: asking for the size of a property with a null buffer is allowed.
            unsafe {
                CertGetCertificateContextProperty(
                    self.0,
                    CERT_KEY_PROV_INFO_PROP_ID,
                    None,
                    &mut size,
                )
            }
            .is_ok()
        }
    }

    impl Drop for Context {
        fn drop(&mut self) {
            // SAFETY: the context is owned by this value and freed once.
            let _ = unsafe { CertFreeCertificateContext(Some(self.0)) };
        }
    }

    /// A key of CNG that signs digests.
    pub struct CngKey {
        handle: NCRYPT_KEY_HANDLE,
        free: bool,
        kind: KeyKind,
        spki: SubjectPublicKeyInfoOwned,
    }

    // SAFETY: the handle is only used to sign, by the one job that holds the identity; CNG key
    // handles may be used from any thread.
    unsafe impl Send for CngKey {}
    unsafe impl Sync for CngKey {}

    impl Drop for CngKey {
        fn drop(&mut self) {
            if self.free {
                // SAFETY: the handle was handed to us to free (`pfCallerFree...` was true).
                let _ = unsafe {
                    NCryptFreeObject(windows::Win32::Security::Cryptography::NCRYPT_HANDLE(
                        self.handle.0,
                    ))
                };
            }
        }
    }

    impl SignKey for CngKey {
        fn kind(&self) -> KeyKind {
            self.kind
        }

        fn public_key(&self) -> Result<SubjectPublicKeyInfoOwned, String> {
            Ok(self.spki.clone())
        }

        fn sign_digest(&self, digest: &[u8]) -> Result<Vec<u8>, String> {
            let is_rsa = matches!(self.kind, KeyKind::Rsa(_));
            let hash_name = match self.hash() {
                Hash::Sha1 => w!("SHA1"),
                Hash::Sha256 => w!("SHA256"),
                Hash::Sha384 => w!("SHA384"),
                Hash::Sha512 => w!("SHA512"),
            };
            let padding = BCRYPT_PKCS1_PADDING_INFO {
                pszAlgId: hash_name,
            };
            let (info, flags): (Option<*const c_void>, NCRYPT_FLAGS) = if is_rsa {
                (
                    Some(ptr::from_ref(&padding).cast::<c_void>()),
                    NCRYPT_FLAGS(BCRYPT_PAD_PKCS1.0),
                )
            } else {
                (None, NCRYPT_FLAGS(0))
            };
            let mut size = 0u32;
            // SAFETY: the first call only asks for the size of the signature; `padding` outlives
            // both calls.
            unsafe { NCryptSignHash(self.handle, info, digest, None, &mut size, flags) }
                .map_err(|e| format!("Windows could not sign: {e}"))?;
            let mut signature = vec![0u8; size as usize];
            // SAFETY: the buffer has the size the first call reported.
            unsafe {
                NCryptSignHash(
                    self.handle,
                    info,
                    digest,
                    Some(&mut signature),
                    &mut size,
                    flags,
                )
            }
            .map_err(|e| format!("Windows could not sign: {e}"))?;
            signature.truncate(size as usize);
            if is_rsa {
                return Ok(signature);
            }
            // CNG gives ECDSA as r followed by s; CMS wants a DER sequence.
            match self.kind {
                KeyKind::P384 => p384::ecdsa::Signature::from_slice(&signature)
                    .map(|s| s.to_der().as_bytes().to_vec())
                    .map_err(|e| e.to_string()),
                _ => p256::ecdsa::Signature::from_slice(&signature)
                    .map(|s| s.to_der().as_bytes().to_vec())
                    .map_err(|e| e.to_string()),
            }
        }
    }

    /// The identity of a certificate context: its key in CNG and the chain from the stores.
    pub(crate) fn identity_of(context: &Context, pool: Vec<Cert>) -> Result<Identity, AppError> {
        let leaf = Cert::parse(&context.der()).map_err(|_| failed("read the certificate"))?;
        let spki = leaf.inner.tbs_certificate.subject_public_key_info.clone();
        let kind = algo::key_kind(&spki).map_err(|_| {
            failed("use this kind of key (only RSA, P-256 and P-384 are supported)")
        })?;
        let mut handle = HCRYPTPROV_OR_NCRYPT_KEY_HANDLE(0);
        let mut spec = CERT_KEY_SPEC(0);
        let mut free = BOOL(0);
        // SAFETY: the context is live; the out parameters point at locals.
        unsafe {
            CryptAcquireCertificatePrivateKey(
                context.0,
                CRYPT_ACQUIRE_PREFER_NCRYPT_KEY_FLAG,
                None,
                &mut handle,
                Some(&mut spec),
                Some(&mut free),
            )
        }
        .map_err(|e| {
            failed(&format!(
                "open the private key of this certificate (it may need a smart card or token; Windows error {:#010X})",
                e.code().0
            ))
        })?;
        let key = CngKey {
            handle: NCRYPT_KEY_HANDLE(handle.0),
            free: free.as_bool(),
            kind,
            spki,
        };
        if spec != CERT_NCRYPT_KEY_SPEC {
            // An older provider (CryptoAPI): its handle is not a CNG key.
            drop(key);
            return Err(failed(
                "use a key of an older provider. Export the certificate to a PFX file, or renew it with a modern provider",
            ));
        }
        Ok(Identity {
            chain: order_chain(leaf, pool),
            key: Box::new(key),
        })
    }

    fn pool_of(stores: &[PCWSTR]) -> Vec<Cert> {
        let mut pool = Vec::new();
        for name in stores {
            if let Ok(store) = Store::open(*name) {
                pool.extend(
                    store
                        .contexts()
                        .iter()
                        .filter_map(|c| Cert::parse(&c.der()).ok()),
                );
            }
        }
        pool
    }

    pub fn identity(thumbprint: &str) -> Result<Identity, AppError> {
        let personal = Store::open(w!("MY"))?;
        let found = personal
            .contexts()
            .into_iter()
            .find(|c| Cert::parse(&c.der()).is_ok_and(|cert| cert.thumbprint() == thumbprint))
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::OptionInvalid,
                    "No certificate with this thumbprint is in your personal certificate store. Check the thumbprint, or choose the certificate from the list.",
                )
            })?;
        identity_of(&found, pool_of(&[w!("MY"), w!("CA"), w!("ROOT")]))
    }

    pub fn list() -> Result<Vec<StoredCertificate>, AppError> {
        let store = Store::open(w!("MY"))?;
        let mut out = Vec::new();
        for context in store.contexts() {
            if !context.has_private_key() {
                continue;
            }
            let Ok(cert) = Cert::parse(&context.der()) else {
                continue;
            };
            if !cert.may_sign_documents()
                || algo::key_kind(&cert.inner.tbs_certificate.subject_public_key_info).is_err()
            {
                continue;
            }
            out.push(StoredCertificate {
                thumbprint: cert.thumbprint(),
                subject: cert.subject(),
                issuer: cert.issuer(),
                valid_until: timefmt::day(cert.not_after()),
            });
        }
        out.sort_by(|a, b| a.subject.cmp(&b.subject));
        Ok(out)
    }
}
