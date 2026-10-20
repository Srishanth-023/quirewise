//! Signing keys: what a key can do, and reading one from a PFX or P12 file.

use application::AppError;
use der::{Decode, Encode};
use domain::ErrorCode;
use p12_keystore::{KeyStore, Pkcs12ImportPolicy};
use rand_core::OsRng;
use signature::hazmat::PrehashSigner;
use spki::{AlgorithmIdentifierOwned, SubjectPublicKeyInfoOwned};

use crate::algo::{self, Hash, KeyKind};
use crate::certs::{Cert, same_name};

/// A private key that signs digests. Implementations never reveal the key.
pub trait SignKey: Send + Sync {
    /// The kind of key.
    fn kind(&self) -> KeyKind;

    /// The public key, as in a certificate.
    fn public_key(&self) -> Result<SubjectPublicKeyInfoOwned, String>;

    /// Signs `digest`, which was made with [`SignKey::hash`], and returns the signature as a CMS
    /// signature value holds it: PKCS#1 bytes for RSA, a DER sequence for ECDSA.
    fn sign_digest(&self, digest: &[u8]) -> Result<Vec<u8>, String>;

    /// The hash to use with this key.
    fn hash(&self) -> Hash {
        self.kind().signing_hash()
    }

    /// The signature algorithm identifier that goes with [`SignKey::hash`].
    fn algorithm(&self) -> AlgorithmIdentifierOwned {
        let oid = match (self.kind(), self.hash()) {
            (KeyKind::Rsa(_), Hash::Sha384) => algo::SHA384_WITH_RSA,
            (KeyKind::Rsa(_), Hash::Sha512) => algo::SHA512_WITH_RSA,
            (KeyKind::Rsa(_), _) => algo::SHA256_WITH_RSA,
            (_, Hash::Sha384) => algo::ECDSA_SHA384,
            (_, Hash::Sha512) => algo::ECDSA_SHA512,
            _ => algo::ECDSA_SHA256,
        };
        let parameters =
            matches!(self.kind(), KeyKind::Rsa(_)).then(|| der::Any::from(der::asn1::Null));
        AlgorithmIdentifierOwned { oid, parameters }
    }
}

/// A key held in memory, read from a file. It is wiped when dropped.
enum SoftKey {
    Rsa(Box<rsa::RsaPrivateKey>),
    P256(p256::ecdsa::SigningKey),
    P384(p384::ecdsa::SigningKey),
}

impl SignKey for SoftKey {
    fn kind(&self) -> KeyKind {
        use rsa::traits::PublicKeyParts;
        match self {
            Self::Rsa(k) => KeyKind::Rsa(k.n().bits()),
            Self::P256(_) => KeyKind::P256,
            Self::P384(_) => KeyKind::P384,
        }
    }

    fn public_key(&self) -> Result<SubjectPublicKeyInfoOwned, String> {
        use spki::EncodePublicKey;
        let der = match self {
            Self::Rsa(k) => rsa::RsaPublicKey::from(&**k).to_public_key_der(),
            Self::P256(k) => k.verifying_key().to_public_key_der(),
            Self::P384(k) => k.verifying_key().to_public_key_der(),
        }
        .map_err(|e| e.to_string())?;
        SubjectPublicKeyInfoOwned::from_der(der.as_bytes()).map_err(|e| e.to_string())
    }

    fn sign_digest(&self, digest: &[u8]) -> Result<Vec<u8>, String> {
        use rsa::pkcs1v15::Pkcs1v15Sign;
        match self {
            Self::Rsa(k) => {
                let hash = self.hash();
                let result = match hash {
                    Hash::Sha1 => {
                        k.sign_with_rng(&mut OsRng, Pkcs1v15Sign::new::<sha1::Sha1>(), digest)
                    }
                    Hash::Sha256 => {
                        k.sign_with_rng(&mut OsRng, Pkcs1v15Sign::new::<sha2::Sha256>(), digest)
                    }
                    Hash::Sha384 => {
                        k.sign_with_rng(&mut OsRng, Pkcs1v15Sign::new::<sha2::Sha384>(), digest)
                    }
                    Hash::Sha512 => {
                        k.sign_with_rng(&mut OsRng, Pkcs1v15Sign::new::<sha2::Sha512>(), digest)
                    }
                };
                result.map_err(|e| e.to_string())
            }
            Self::P256(k) => {
                let sig: p256::ecdsa::Signature =
                    k.sign_prehash(digest).map_err(|e| e.to_string())?;
                Ok(sig.to_der().as_bytes().to_vec())
            }
            Self::P384(k) => {
                let sig: p384::ecdsa::Signature =
                    k.sign_prehash(digest).map_err(|e| e.to_string())?;
                Ok(sig.to_der().as_bytes().to_vec())
            }
        }
    }
}

/// A signer: its certificate and the ones above it, with the key.
pub struct Identity {
    /// The certificate of the signer first, then the ones that issued it.
    pub chain: Vec<Cert>,
    /// The key.
    pub key: Box<dyn SignKey>,
}

impl Identity {
    /// The certificate of the signer.
    pub fn signer(&self) -> &Cert {
        &self.chain[0]
    }
}

fn pfx_error(what: &str) -> AppError {
    AppError::new(
        ErrorCode::OptionInvalid,
        format!(
            "The certificate file could not be used: {what}. Check the file and the password, or choose another certificate."
        ),
    )
}

/// Reads the key from PKCS#8 DER.
fn soft_key(pkcs8: &[u8]) -> Result<SoftKey, String> {
    use p256::pkcs8::DecodePrivateKey;
    let info = pkcs8_oids(pkcs8)?;
    if info.0 == algo::RSA_ENCRYPTION {
        return rsa::RsaPrivateKey::from_pkcs8_der(pkcs8)
            .map(|k| SoftKey::Rsa(Box::new(k)))
            .map_err(|e| e.to_string());
    }
    if info.0 == algo::EC_PUBLIC_KEY {
        return match info.1 {
            Some(c) if c == algo::P256 => p256::ecdsa::SigningKey::from_pkcs8_der(pkcs8)
                .map(SoftKey::P256)
                .map_err(|e| e.to_string()),
            Some(c) if c == algo::P384 => p384::ecdsa::SigningKey::from_pkcs8_der(pkcs8)
                .map(SoftKey::P384)
                .map_err(|e| e.to_string()),
            _ => Err("the key uses an elliptic curve other than P-256 and P-384".to_owned()),
        };
    }
    Err("the key is of a kind that is not supported (use RSA, P-256 or P-384)".to_owned())
}

/// The algorithm and curve identifiers of a PKCS#8 private key.
fn pkcs8_oids(
    pkcs8: &[u8],
) -> Result<
    (
        const_oid::ObjectIdentifier,
        Option<const_oid::ObjectIdentifier>,
    ),
    String,
> {
    let info = rsa::pkcs8::PrivateKeyInfo::try_from(pkcs8).map_err(|e| e.to_string())?;
    let curve = info
        .algorithm
        .parameters
        .and_then(|p| p.decode_as::<const_oid::ObjectIdentifier>().ok());
    Ok((info.algorithm.oid, curve))
}

/// Orders certificates from the one named by `leaf` up through their issuers, dropping the ones
/// that are not part of that chain.
pub fn order_chain(leaf: Cert, mut others: Vec<Cert>) -> Vec<Cert> {
    let mut chain = vec![leaf];
    while let Some(last) = chain.last() {
        if last.is_self_issued() {
            break;
        }
        let issuer_name = last.inner.tbs_certificate.issuer.clone();
        let position = others.iter().position(|c| {
            same_name(&c.inner.tbs_certificate.subject, &issuer_name)
                && last.check_issued_by(c).is_ok()
        });
        match position {
            Some(i) => chain.push(others.remove(i)),
            None => break,
        }
    }
    chain
}

/// Reads the signing identity from the bytes of a PFX or P12 file.
pub fn load_pfx(bytes: &[u8], password: &str) -> Result<Identity, AppError> {
    let store =
        KeyStore::from_pkcs12(bytes, password, Pkcs12ImportPolicy::Relaxed).map_err(|e| {
            let text = e.to_string();
            if matches!(e, p12_keystore::error::Error::MacError(_))
                || text.contains("MAC")
                || text.contains("mac")
            {
                AppError::new(
                    ErrorCode::OptionInvalid,
                    "The password of the certificate file is not correct. Enter it again.",
                )
            } else {
                pfx_error("it is not a PFX or P12 file, or it is damaged")
            }
        })?;
    let (_, chain) = store
        .private_key_chain()
        .ok_or_else(|| pfx_error("it holds no private key"))?;
    let key = soft_key(chain.key().as_der()).map_err(|e| pfx_error(&e))?;
    let public = key.public_key().map_err(|e| pfx_error(&e))?;
    let public_der = public.to_der().map_err(|e| pfx_error(&e.to_string()))?;
    let mut certs: Vec<Cert> = Vec::new();
    for c in chain.certs() {
        certs.push(Cert::parse(c.as_der()).map_err(|e| pfx_error(&e))?);
    }
    let leaf_at = certs
        .iter()
        .position(|c| {
            c.inner
                .tbs_certificate
                .subject_public_key_info
                .to_der()
                .is_ok_and(|d| d == public_der)
        })
        .ok_or_else(|| pfx_error("no certificate in it belongs to the private key"))?;
    let leaf = certs.remove(leaf_at);
    Ok(Identity {
        chain: order_chain(leaf, certs),
        key: Box::new(key),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{LeafKind, Pki, Spec};

    #[test]
    fn a_pfx_gives_the_key_and_the_ordered_chain() {
        let pki = Pki::build(&Spec {
            intermediate: true,
            ..Spec::default()
        });
        let id = load_pfx(&pki.pfx("secret"), "secret").unwrap();
        let names: Vec<String> = id.chain.iter().map(Cert::subject).collect();
        assert_eq!(names, ["Test Signer", "Test Intermediate", "Test Root"]);
        assert_eq!(id.key.kind(), KeyKind::P256);
    }

    #[test]
    fn a_wrong_password_is_reported_as_such() {
        let pki = Pki::ec();
        let err = load_pfx(&pki.pfx("secret"), "guess").err().unwrap();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
        assert!(err.message.contains("password"), "{}", err.message);
    }

    #[test]
    fn something_that_is_not_a_pfx_is_refused() {
        let err = load_pfx(b"not a pfx at all", "x").err().unwrap();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
        assert!(err.message.contains("certificate file"), "{}", err.message);
    }

    #[test]
    fn every_kind_of_key_signs_and_verifies() {
        for kind in [LeafKind::P256, LeafKind::P384, LeafKind::Rsa] {
            let pki = Pki::build(&Spec {
                kind,
                ..Spec::default()
            });
            let id = load_pfx(&pki.pfx("pw"), "pw").unwrap();
            let message = b"to be signed";
            let digest = id.key.hash().digest(message);
            let signature = id.key.sign_digest(&digest).unwrap();
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
    fn a_chain_stops_where_the_issuer_is_missing() {
        let pki = Pki::build(&Spec {
            intermediate: true,
            ..Spec::default()
        });
        let leaf = Cert::parse(&pki.leaf_der).unwrap();
        let root = Cert::parse(&pki.root_der).unwrap();
        let chain = order_chain(leaf, vec![root]);
        assert_eq!(
            chain.len(),
            1,
            "the intermediate is not there, so the root is not linked"
        );
    }
}
