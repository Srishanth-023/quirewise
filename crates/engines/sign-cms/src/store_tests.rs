//! The Windows store code, exercised with keys made for the test, held in CNG without a name, and
//! a certificate store in memory: nothing in the stores of the person is touched.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr;

use application::runtime::CancellationToken;
use application::signing::{KeyChoice, SignRequest, SignatureChecker, Status, VerifyRequest};
use der::Decode;
use domain::tools::signing::{Certify, SignLevel};
use spki::SubjectPublicKeyInfoOwned;
use windows::Win32::Security::Cryptography::{
    BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALGORITHM, BCRYPT_ECDSA_P384_ALGORITHM,
    BCRYPT_RSA_ALGORITHM, BCRYPT_RSAPUBLIC_BLOB, CERT_CONTEXT, CERT_KEY_PROV_INFO_PROP_ID,
    CERT_KEY_SPEC, CERT_OPEN_STORE_FLAGS, CERT_QUERY_ENCODING_TYPE, CERT_STORE_ADD_ALWAYS,
    CERT_STORE_CREATE_NEW_FLAG, CERT_STORE_PROV_MEMORY, CRYPT_KEY_FLAGS, CRYPT_KEY_PROV_INFO,
    CertAddEncodedCertificateToStore, CertCloseStore, CertOpenStore,
    CertSetCertificateContextProperty, MS_KEY_STORAGE_PROVIDER, NCRYPT_FLAGS, NCRYPT_HANDLE,
    NCRYPT_KEY_HANDLE, NCRYPT_LENGTH_PROPERTY, NCRYPT_PROV_HANDLE, NCryptCreatePersistedKey,
    NCryptDeleteKey, NCryptExportKey, NCryptFinalizeKey, NCryptFreeObject,
    NCryptOpenStorageProvider, NCryptSetProperty, X509_ASN_ENCODING,
};
use windows::core::{PCWSTR, PWSTR};

use std::sync::atomic::{AtomicU32, Ordering};

use crate::algo::{self, KeyKind};
use crate::certs::Cert;
use crate::store::imp::{Context, identity_of};
use crate::testkit::Pki;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A key made for one test in the key container of the person, deleted when the test ends.
struct TestKey {
    handle: NCRYPT_KEY_HANDLE,
    name: Vec<u16>,
}

impl Drop for TestKey {
    fn drop(&mut self) {
        // SAFETY: the handle is the one that created the key; deleting it also frees it.
        let _ = unsafe { NCryptDeleteKey(self.handle, 0) };
    }
}

fn make_key(algorithm: PCWSTR, rsa_bits: Option<u32>) -> TestKey {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let name = wide(&format!(
        "quirewise-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let mut provider = NCRYPT_PROV_HANDLE(0);
    let mut key = NCRYPT_KEY_HANDLE(0);
    // SAFETY: out parameters point at locals; the name is nul-terminated and outlives the call;
    // the provider is freed before returning.
    unsafe {
        NCryptOpenStorageProvider(&mut provider, MS_KEY_STORAGE_PROVIDER, 0).expect("a provider");
        NCryptCreatePersistedKey(
            provider,
            &mut key,
            algorithm,
            PCWSTR(name.as_ptr()),
            CERT_KEY_SPEC(0),
            NCRYPT_FLAGS(0),
        )
        .expect("a key");
        if let Some(bits) = rsa_bits {
            NCryptSetProperty(
                NCRYPT_HANDLE(key.0),
                NCRYPT_LENGTH_PROPERTY,
                &bits.to_le_bytes(),
                NCRYPT_FLAGS(0),
            )
            .expect("a length");
        }
        NCryptFinalizeKey(key, NCRYPT_FLAGS(0)).expect("a finished key");
        let _ = NCryptFreeObject(NCRYPT_HANDLE(provider.0));
    }
    TestKey { handle: key, name }
}

fn export(key: NCRYPT_KEY_HANDLE, blob: PCWSTR) -> Vec<u8> {
    let mut size = 0u32;
    // SAFETY: the first call asks for the size; the second fills a buffer of that size.
    unsafe {
        NCryptExportKey(key, None, blob, None, None, &mut size, NCRYPT_FLAGS(0)).expect("a size");
        let mut out = vec![0u8; size as usize];
        NCryptExportKey(
            key,
            None,
            blob,
            None,
            Some(&mut out),
            &mut size,
            NCRYPT_FLAGS(0),
        )
        .expect("a blob");
        out.truncate(size as usize);
        out
    }
}

fn ec_public(
    key: NCRYPT_KEY_HANDLE,
    curve: const_oid::ObjectIdentifier,
) -> SubjectPublicKeyInfoOwned {
    let blob = export(key, BCRYPT_ECCPUBLIC_BLOB);
    // BCRYPT_ECCKEY_BLOB: magic, key length, then X and Y.
    let len = u32::from_le_bytes(blob[4..8].try_into().unwrap()) as usize;
    let mut point = vec![0x04];
    point.extend_from_slice(&blob[8..8 + 2 * len]);
    SubjectPublicKeyInfoOwned {
        algorithm: spki::AlgorithmIdentifierOwned {
            oid: algo::EC_PUBLIC_KEY,
            parameters: Some(der::Any::from(curve)),
        },
        subject_public_key: der::asn1::BitString::from_bytes(&point).unwrap(),
    }
}

fn rsa_public(key: NCRYPT_KEY_HANDLE) -> SubjectPublicKeyInfoOwned {
    use spki::EncodePublicKey;
    let blob = export(key, BCRYPT_RSAPUBLIC_BLOB);
    // BCRYPT_RSAKEY_BLOB: magic, bits, exponent length, modulus length, two prime lengths, then
    // the exponent and the modulus.
    let e_len = u32::from_le_bytes(blob[8..12].try_into().unwrap()) as usize;
    let n_len = u32::from_le_bytes(blob[12..16].try_into().unwrap()) as usize;
    let e = rsa::BigUint::from_bytes_be(&blob[24..24 + e_len]);
    let n = rsa::BigUint::from_bytes_be(&blob[24 + e_len..24 + e_len + n_len]);
    let public = rsa::RsaPublicKey::new(n, e).unwrap();
    SubjectPublicKeyInfoOwned::from_der(public.to_public_key_der().unwrap().as_bytes()).unwrap()
}

/// A context in a memory store for `certificate`, tied to `key` the way a certificate of the
/// personal store is: by the name of the key and of its provider.
fn context_with_key(certificate: &[u8], key: &TestKey) -> Context {
    let mut raw: *mut CERT_CONTEXT = ptr::null_mut();
    let mut provider = wide("Microsoft Software Key Storage Provider");
    let mut container = key.name.clone();
    let info = CRYPT_KEY_PROV_INFO {
        pwszContainerName: PWSTR(container.as_mut_ptr()),
        pwszProvName: PWSTR(provider.as_mut_ptr()),
        dwProvType: 0,
        dwFlags: CRYPT_KEY_FLAGS(0),
        cProvParam: 0,
        rgProvParam: ptr::null_mut(),
        dwKeySpec: 0,
    };
    // SAFETY: a memory store holds the certificate only for this test; the context returned by
    // the add call is released by `Context` on drop. The info and the strings it points at
    // outlive the call that copies them.
    unsafe {
        let store = CertOpenStore(
            CERT_STORE_PROV_MEMORY,
            CERT_QUERY_ENCODING_TYPE(0),
            None,
            CERT_STORE_CREATE_NEW_FLAG | CERT_OPEN_STORE_FLAGS(0),
            None,
        )
        .expect("a memory store");
        CertAddEncodedCertificateToStore(
            Some(store),
            X509_ASN_ENCODING,
            certificate,
            CERT_STORE_ADD_ALWAYS,
            Some(&mut raw),
        )
        .expect("a certificate in the store");
        CertSetCertificateContextProperty(
            raw,
            CERT_KEY_PROV_INFO_PROP_ID,
            0,
            Some(ptr::from_ref(&info).cast::<c_void>()),
        )
        .expect("a key for the certificate");
        let _ = CertCloseStore(Some(store), 0);
    }
    Context::from_raw(raw)
}

fn identity_for(pki: &Pki, kind: KeyKind) -> (crate::keys::Identity, TestKey) {
    let (key, spki) = match kind {
        KeyKind::P256 => {
            let k = make_key(BCRYPT_ECDSA_P256_ALGORITHM, None);
            let spki = ec_public(k.handle, algo::P256);
            (k, spki)
        }
        KeyKind::P384 => {
            let k = make_key(BCRYPT_ECDSA_P384_ALGORITHM, None);
            let spki = ec_public(k.handle, algo::P384);
            (k, spki)
        }
        KeyKind::Rsa(_) => {
            let k = make_key(BCRYPT_RSA_ALGORITHM, Some(2048));
            let spki = rsa_public(k.handle);
            (k, spki)
        }
    };
    let certificate = pki.issue_for(spki, "Store Signer");
    let context = context_with_key(&certificate, &key);
    let root = Cert::parse(&pki.root_der).unwrap();
    let identity = identity_of(&context, vec![root]).expect("an identity");
    // The key is deleted when the test drops it, after the identity was used.
    (identity, key)
}

#[test]
fn every_kind_of_cng_key_signs_digests_that_verify() {
    let pki = Pki::ec();
    for kind in [KeyKind::P256, KeyKind::P384, KeyKind::Rsa(2048)] {
        let (id, _key) = identity_for(&pki, kind);
        assert_eq!(id.signer().subject(), "Store Signer");
        assert_eq!(id.chain.len(), 2, "the leaf and the root");
        let message = b"signed through CNG";
        let digest = id.key.hash().digest(message);
        let signature = id.key.sign_digest(&digest).expect("a signature");
        let spki = id
            .signer()
            .inner
            .tbs_certificate
            .subject_public_key_info
            .clone();
        assert!(
            algo::verify(
                &spki,
                &id.key.algorithm(),
                id.key.hash(),
                message,
                &signature
            )
            .is_ok(),
            "{kind:?}"
        );
        assert!(
            algo::verify(
                &spki,
                &id.key.algorithm(),
                id.key.hash(),
                b"other",
                &signature
            )
            .is_err(),
            "{kind:?} must reject other data"
        );
    }
}

#[test]
fn a_pdf_signed_through_cng_verifies() {
    let pki = Pki::ec();
    let (id, _key) = identity_for(&pki, KeyKind::Rsa(2048));
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.pdf");
    std::fs::write(&input, test_support::pdfs::pdf_with_texts(&["one"])).unwrap();
    let out = dir.path().join("out.pdf");
    let request = SignRequest {
        key: KeyChoice::Store {
            thumbprint: id.signer().thumbprint(),
        },
        field: None,
        visible: None,
        signer_name: None,
        reason: None,
        location: None,
        contact: None,
        level: SignLevel::BB,
        timestamp_url: None,
        certify: Certify::None,
        signed_at: i64::try_from(crate::testkit::now()).unwrap(),
    };
    crate::sign::sign_with(&id, &input, &out, &request, None, &CancellationToken::new()).unwrap();
    let trust = dir.path().join("trust.pem");
    std::fs::write(&trust, pki.root_pem()).unwrap();
    let reports = crate::CmsEngine::isolated()
        .check(
            &out,
            &VerifyRequest {
                check_revocation: false,
                trusted_certificates: Some(trust),
                now: request.signed_at,
            },
            None,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].signer, "Store Signer");
    assert_eq!(
        reports[0].signature.status,
        Status::Pass,
        "{:?}",
        reports[0]
    );
    assert_eq!(reports[0].identity.status, Status::Pass, "{:?}", reports[0]);
}

#[test]
fn the_store_lists_without_failing() {
    // The content depends on the machine; listing must work and every entry must be complete.
    for c in crate::store::list().expect("the personal store opens") {
        assert_eq!(c.thumbprint.len(), 40);
        assert!(!c.subject.is_empty());
    }
}

#[test]
fn an_unknown_thumbprint_is_reported() {
    let err = crate::store::identity(&"0".repeat(40))
        .err()
        .expect("no such certificate");
    assert!(err.message.contains("thumbprint"), "{}", err.message);
}
