//! Hash and signature algorithms: names, identifiers and checking a signature against a key.

use const_oid::ObjectIdentifier as Oid;
use der::asn1::OctetString;
use der::{Decode, Reader, Tagged};
use sha2::Digest;
use spki::{AlgorithmIdentifierOwned, SubjectPublicKeyInfoOwned};

/// SHA-1, only to read old signatures.
pub const SHA1: Oid = Oid::new_unwrap("1.3.14.3.2.26");
/// SHA-256.
pub const SHA256: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.2.1");
/// SHA-384.
pub const SHA384: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.2.2");
/// SHA-512.
pub const SHA512: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.2.3");

/// RSA keys.
pub const RSA_ENCRYPTION: Oid = Oid::new_unwrap("1.2.840.113549.1.1.1");
/// RSA PKCS#1 v1.5 with SHA-1.
pub const SHA1_WITH_RSA: Oid = Oid::new_unwrap("1.2.840.113549.1.1.5");
/// RSA PKCS#1 v1.5 with SHA-256.
pub const SHA256_WITH_RSA: Oid = Oid::new_unwrap("1.2.840.113549.1.1.11");
/// RSA PKCS#1 v1.5 with SHA-384.
pub const SHA384_WITH_RSA: Oid = Oid::new_unwrap("1.2.840.113549.1.1.12");
/// RSA PKCS#1 v1.5 with SHA-512.
pub const SHA512_WITH_RSA: Oid = Oid::new_unwrap("1.2.840.113549.1.1.13");
/// RSA with PSS padding.
pub const RSA_PSS: Oid = Oid::new_unwrap("1.2.840.113549.1.1.10");

/// Elliptic curve keys.
pub const EC_PUBLIC_KEY: Oid = Oid::new_unwrap("1.2.840.10045.2.1");
/// ECDSA with SHA-1.
pub const ECDSA_SHA1: Oid = Oid::new_unwrap("1.2.840.10045.4.1");
/// ECDSA with SHA-256.
pub const ECDSA_SHA256: Oid = Oid::new_unwrap("1.2.840.10045.4.3.2");
/// ECDSA with SHA-384.
pub const ECDSA_SHA384: Oid = Oid::new_unwrap("1.2.840.10045.4.3.3");
/// ECDSA with SHA-512.
pub const ECDSA_SHA512: Oid = Oid::new_unwrap("1.2.840.10045.4.3.4");
/// The curve P-256.
pub const P256: Oid = Oid::new_unwrap("1.2.840.10045.3.1.7");
/// The curve P-384.
pub const P384: Oid = Oid::new_unwrap("1.3.132.0.34");

/// A hash algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hash {
    /// SHA-1: read, never written.
    Sha1,
    /// SHA-256.
    Sha256,
    /// SHA-384.
    Sha384,
    /// SHA-512.
    Sha512,
}

impl Hash {
    /// The identifier.
    pub const fn oid(self) -> Oid {
        match self {
            Self::Sha1 => SHA1,
            Self::Sha256 => SHA256,
            Self::Sha384 => SHA384,
            Self::Sha512 => SHA512,
        }
    }

    /// The hash with this identifier, if it is one we know.
    pub fn from_oid(oid: &Oid) -> Option<Self> {
        [Self::Sha1, Self::Sha256, Self::Sha384, Self::Sha512]
            .into_iter()
            .find(|h| h.oid() == *oid)
    }

    /// Name for people, for example `SHA-256`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sha1 => "SHA-1",
            Self::Sha256 => "SHA-256",
            Self::Sha384 => "SHA-384",
            Self::Sha512 => "SHA-512",
        }
    }

    /// True for hashes that are no longer safe for signatures.
    pub const fn is_weak(self) -> bool {
        matches!(self, Self::Sha1)
    }

    /// The hash of `data`.
    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::Sha1 => sha1::Sha1::digest(data).to_vec(),
            Self::Sha256 => sha2::Sha256::digest(data).to_vec(),
            Self::Sha384 => sha2::Sha384::digest(data).to_vec(),
            Self::Sha512 => sha2::Sha512::digest(data).to_vec(),
        }
    }

    /// An incremental hasher.
    pub fn hasher(self) -> Hasher {
        match self {
            Self::Sha1 => Hasher::Sha1(sha1::Sha1::new()),
            Self::Sha256 => Hasher::Sha256(sha2::Sha256::new()),
            Self::Sha384 => Hasher::Sha384(sha2::Sha384::new()),
            Self::Sha512 => Hasher::Sha512(sha2::Sha512::new()),
        }
    }

    /// The algorithm identifier with the NULL parameter that CMS expects.
    pub fn identifier(self) -> AlgorithmIdentifierOwned {
        AlgorithmIdentifierOwned {
            oid: self.oid(),
            parameters: Some(der::Any::from(der::asn1::Null)),
        }
    }
}

/// Hashes data fed in pieces.
pub enum Hasher {
    /// SHA-1.
    Sha1(sha1::Sha1),
    /// SHA-256.
    Sha256(sha2::Sha256),
    /// SHA-384.
    Sha384(sha2::Sha384),
    /// SHA-512.
    Sha512(sha2::Sha512),
}

impl Hasher {
    /// Adds data.
    pub fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha1(h) => h.update(data),
            Self::Sha256(h) => h.update(data),
            Self::Sha384(h) => h.update(data),
            Self::Sha512(h) => h.update(data),
        }
    }

    /// The hash of everything added.
    pub fn finish(self) -> Vec<u8> {
        match self {
            Self::Sha1(h) => h.finalize().to_vec(),
            Self::Sha256(h) => h.finalize().to_vec(),
            Self::Sha384(h) => h.finalize().to_vec(),
            Self::Sha512(h) => h.finalize().to_vec(),
        }
    }
}

/// The kind of key behind a public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// RSA with a modulus of this many bits.
    Rsa(usize),
    /// ECDSA on P-256.
    P256,
    /// ECDSA on P-384.
    P384,
}

impl KeyKind {
    /// Name for people, for example `RSA 2048` or `ECDSA P-256`.
    pub fn name(self) -> String {
        match self {
            Self::Rsa(bits) => format!("RSA {bits}"),
            Self::P256 => "ECDSA P-256".to_owned(),
            Self::P384 => "ECDSA P-384".to_owned(),
        }
    }

    /// True for keys too short to trust.
    pub fn is_weak(self) -> bool {
        matches!(self, Self::Rsa(bits) if bits < 2048)
    }

    /// The hash that suits this key when signing.
    pub const fn signing_hash(self) -> Hash {
        match self {
            Self::P384 => Hash::Sha384,
            _ => Hash::Sha256,
        }
    }
}

/// The kind of key in a SubjectPublicKeyInfo.
pub fn key_kind(spki: &SubjectPublicKeyInfoOwned) -> Result<KeyKind, String> {
    use rsa::traits::PublicKeyParts;
    use spki::DecodePublicKey;
    let der = der::Encode::to_der(spki).map_err(|e| e.to_string())?;
    let oid = spki.algorithm.oid;
    if oid == RSA_ENCRYPTION {
        let key = rsa::RsaPublicKey::from_public_key_der(&der).map_err(|e| e.to_string())?;
        return Ok(KeyKind::Rsa(key.n().bits()));
    }
    if oid == EC_PUBLIC_KEY {
        let curve = spki
            .algorithm
            .parameters
            .as_ref()
            .and_then(|p| p.decode_as::<Oid>().ok());
        return match curve {
            Some(c) if c == P256 => Ok(KeyKind::P256),
            Some(c) if c == P384 => Ok(KeyKind::P384),
            _ => Err("an elliptic curve other than P-256 and P-384".to_owned()),
        };
    }
    Err("a kind of key that is not supported".to_owned())
}

/// The signature scheme named by a signature algorithm identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// RSA PKCS#1 v1.5, hash taken from the identifier or the digest algorithm.
    RsaPkcs1(Option<Hash>),
    /// RSA PSS with the hash in its parameters.
    RsaPss(Hash),
    /// ECDSA, hash from the identifier or the digest algorithm.
    Ecdsa(Option<Hash>),
}

/// Reads a signature algorithm identifier.
pub fn scheme(alg: &AlgorithmIdentifierOwned) -> Result<Scheme, String> {
    let oid = alg.oid;
    let table: [(Oid, Scheme); 9] = [
        (RSA_ENCRYPTION, Scheme::RsaPkcs1(None)),
        (SHA1_WITH_RSA, Scheme::RsaPkcs1(Some(Hash::Sha1))),
        (SHA256_WITH_RSA, Scheme::RsaPkcs1(Some(Hash::Sha256))),
        (SHA384_WITH_RSA, Scheme::RsaPkcs1(Some(Hash::Sha384))),
        (SHA512_WITH_RSA, Scheme::RsaPkcs1(Some(Hash::Sha512))),
        (ECDSA_SHA1, Scheme::Ecdsa(Some(Hash::Sha1))),
        (ECDSA_SHA256, Scheme::Ecdsa(Some(Hash::Sha256))),
        (ECDSA_SHA384, Scheme::Ecdsa(Some(Hash::Sha384))),
        (ECDSA_SHA512, Scheme::Ecdsa(Some(Hash::Sha512))),
    ];
    if let Some((_, s)) = table.iter().find(|(o, _)| *o == oid) {
        return Ok(*s);
    }
    if oid == EC_PUBLIC_KEY {
        return Ok(Scheme::Ecdsa(None));
    }
    if oid == RSA_PSS {
        return Ok(Scheme::RsaPss(pss_hash(alg)?));
    }
    Err(format!("the signature algorithm {oid} is not supported"))
}

/// The hash named in the parameters of an RSA-PSS identifier.
fn pss_hash(alg: &AlgorithmIdentifierOwned) -> Result<Hash, String> {
    // RSASSA-PSS-params ::= SEQUENCE { hashAlgorithm [0] AlgorithmIdentifier DEFAULT sha1, ... }
    let params = alg
        .parameters
        .as_ref()
        .ok_or_else(|| "an RSA-PSS signature without parameters".to_owned())?;
    let bytes = der::Encode::to_der(params).map_err(|e| e.to_string())?;
    let mut reader = der::SliceReader::new(&bytes).map_err(|e| e.to_string())?;
    let header = <der::Header as der::Decode>::decode(&mut reader).map_err(|e| e.to_string())?;
    let body = reader
        .read_slice(header.length)
        .map_err(|e| e.to_string())?;
    let mut inner = der::SliceReader::new(body).map_err(|e| e.to_string())?;
    // The first element, when present, is [0] EXPLICIT hashAlgorithm.
    let Ok(tagged) = <der::Any as der::Decode>::decode(&mut inner) else {
        return Ok(Hash::Sha1);
    };
    let explicit_zero = der::Tag::ContextSpecific {
        constructed: true,
        number: der::TagNumber::N0,
    };
    if tagged.tag() != explicit_zero {
        return Ok(Hash::Sha1);
    }
    let identifier =
        AlgorithmIdentifierOwned::from_der(tagged.value()).map_err(|e| e.to_string())?;
    Hash::from_oid(&identifier.oid)
        .ok_or_else(|| "an RSA-PSS signature with an unknown hash".to_owned())
}

impl std::fmt::Display for Scheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RsaPkcs1(_) => f.write_str("RSA"),
            Self::RsaPss(_) => f.write_str("RSA-PSS"),
            Self::Ecdsa(_) => f.write_str("ECDSA"),
        }
    }
}

/// Checks that `signature` is a valid signature of `message` by the owner of `spki`.
///
/// `default_hash` is used when the scheme does not name one, as in CMS signatures that carry the
/// plain key algorithm and name the hash separately.
pub fn verify(
    spki: &SubjectPublicKeyInfoOwned,
    alg: &AlgorithmIdentifierOwned,
    default_hash: Hash,
    message: &[u8],
    signature: &[u8],
) -> Result<(), String> {
    use rsa::pkcs1v15::Pkcs1v15Sign;
    use rsa::pss::Pss;
    use signature::hazmat::PrehashVerifier;
    use spki::DecodePublicKey;

    let spki_der = der::Encode::to_der(spki).map_err(|e| e.to_string())?;
    let kind = key_kind(spki)?;
    let scheme = scheme(alg)?;
    let bad = || "the signature does not match".to_owned();
    match (kind, scheme) {
        (KeyKind::Rsa(_), Scheme::RsaPkcs1(named)) => {
            let hash = named.unwrap_or(default_hash);
            let key =
                rsa::RsaPublicKey::from_public_key_der(&spki_der).map_err(|e| e.to_string())?;
            let digest = hash.digest(message);
            let result = match hash {
                Hash::Sha1 => key.verify(Pkcs1v15Sign::new::<sha1::Sha1>(), &digest, signature),
                Hash::Sha256 => key.verify(Pkcs1v15Sign::new::<sha2::Sha256>(), &digest, signature),
                Hash::Sha384 => key.verify(Pkcs1v15Sign::new::<sha2::Sha384>(), &digest, signature),
                Hash::Sha512 => key.verify(Pkcs1v15Sign::new::<sha2::Sha512>(), &digest, signature),
            };
            result.map_err(|_| bad())
        }
        (KeyKind::Rsa(_), Scheme::RsaPss(hash)) => {
            let key =
                rsa::RsaPublicKey::from_public_key_der(&spki_der).map_err(|e| e.to_string())?;
            let digest = hash.digest(message);
            let result = match hash {
                Hash::Sha1 => key.verify(Pss::new::<sha1::Sha1>(), &digest, signature),
                Hash::Sha256 => key.verify(Pss::new::<sha2::Sha256>(), &digest, signature),
                Hash::Sha384 => key.verify(Pss::new::<sha2::Sha384>(), &digest, signature),
                Hash::Sha512 => key.verify(Pss::new::<sha2::Sha512>(), &digest, signature),
            };
            result.map_err(|_| bad())
        }
        (KeyKind::P256, Scheme::Ecdsa(named)) => {
            let digest = named.unwrap_or(default_hash).digest(message);
            let key = p256::ecdsa::VerifyingKey::from_public_key_der(&spki_der)
                .map_err(|e| e.to_string())?;
            let sig = p256::ecdsa::Signature::from_der(signature).map_err(|_| bad())?;
            key.verify_prehash(&digest, &sig).map_err(|_| bad())
        }
        (KeyKind::P384, Scheme::Ecdsa(named)) => {
            let digest = named.unwrap_or(default_hash).digest(message);
            let key = p384::ecdsa::VerifyingKey::from_public_key_der(&spki_der)
                .map_err(|e| e.to_string())?;
            let sig = p384::ecdsa::Signature::from_der(signature).map_err(|_| bad())?;
            key.verify_prehash(&digest, &sig).map_err(|_| bad())
        }
        _ => Err(format!(
            "a {scheme} signature does not fit a {} key",
            kind.name()
        )),
    }
}

/// An octet string from bytes.
pub fn octets(bytes: &[u8]) -> Result<OctetString, String> {
    OctetString::new(bytes.to_vec()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_have_the_known_values() {
        assert_eq!(
            Hash::Sha256.digest(b"abc")[..4],
            [0xba, 0x78, 0x16, 0xbf],
            "SHA-256 of abc"
        );
        assert_eq!(Hash::Sha1.digest(b"abc")[..4], [0xa9, 0x99, 0x3e, 0x36]);
        let mut h = Hash::Sha384.hasher();
        h.update(b"a");
        h.update(b"bc");
        assert_eq!(h.finish(), Hash::Sha384.digest(b"abc"));
    }

    #[test]
    fn hash_identifiers_round_trip() {
        for h in [Hash::Sha1, Hash::Sha256, Hash::Sha384, Hash::Sha512] {
            assert_eq!(Hash::from_oid(&h.oid()), Some(h));
        }
        assert_eq!(Hash::from_oid(&RSA_ENCRYPTION), None);
        assert!(Hash::Sha1.is_weak() && !Hash::Sha256.is_weak());
    }

    #[test]
    fn schemes_are_read_from_identifiers() {
        let alg = |oid| AlgorithmIdentifierOwned {
            oid,
            parameters: None,
        };
        assert_eq!(
            scheme(&alg(SHA256_WITH_RSA)).unwrap(),
            Scheme::RsaPkcs1(Some(Hash::Sha256))
        );
        assert_eq!(
            scheme(&alg(RSA_ENCRYPTION)).unwrap(),
            Scheme::RsaPkcs1(None)
        );
        assert_eq!(
            scheme(&alg(ECDSA_SHA384)).unwrap(),
            Scheme::Ecdsa(Some(Hash::Sha384))
        );
        assert!(scheme(&alg(SHA256)).is_err());
    }

    #[test]
    fn short_rsa_keys_are_weak() {
        assert!(KeyKind::Rsa(1024).is_weak());
        assert!(!KeyKind::Rsa(2048).is_weak());
        assert!(!KeyKind::P256.is_weak());
        assert_eq!(KeyKind::P384.signing_hash(), Hash::Sha384);
        assert_eq!(KeyKind::Rsa(3072).signing_hash(), Hash::Sha256);
    }
}
