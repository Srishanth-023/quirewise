//! X.509 certificates: reading names, validity, usage and addresses, and checking who issued one.

use const_oid::ObjectIdentifier as Oid;
use der::asn1::{Ia5StringRef, PrintableStringRef, TeletexStringRef, Utf8StringRef};
use der::{Decode, Encode};
use x509_cert::Certificate;
use x509_cert::ext::pkix::name::{DistributionPointName, GeneralName};
use x509_cert::ext::pkix::{
    AuthorityInfoAccessSyntax, BasicConstraints, CrlDistributionPoints, ExtendedKeyUsage, KeyUsage,
    KeyUsages, SubjectKeyIdentifier,
};
use x509_cert::name::Name;

use crate::algo::{self, Hash};

const COMMON_NAME: Oid = Oid::new_unwrap("2.5.4.3");
const ORGANIZATION: Oid = Oid::new_unwrap("2.5.4.10");
/// The purpose of a time stamp server certificate.
pub const TIME_STAMPING: Oid = Oid::new_unwrap("1.3.6.1.5.5.7.3.8");
/// A certificate with this purpose may be used for anything.
const ANY_EXTENDED_KEY_USAGE: Oid = Oid::new_unwrap("2.5.29.37.0");
/// Access method of an OCSP server in the authority information access extension.
const OCSP: Oid = Oid::new_unwrap("1.3.6.1.5.5.7.48.1");

/// A certificate with the bytes it was read from.
#[derive(Debug, Clone)]
pub struct Cert {
    /// The DER encoding, as read.
    pub der: Vec<u8>,
    /// The parsed certificate.
    pub inner: Certificate,
}

impl PartialEq for Cert {
    fn eq(&self, other: &Self) -> bool {
        self.der == other.der
    }
}

/// Where a certificate points to for the answers about its status or its issuer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    /// An OCSP responder.
    Ocsp,
    /// A certificate revocation list.
    Crl,
}

impl Cert {
    /// Reads one DER certificate.
    pub fn parse(der: &[u8]) -> Result<Self, String> {
        let inner = Certificate::from_der(der).map_err(|e| format!("a certificate: {e}"))?;
        Ok(Self {
            der: der.to_vec(),
            inner,
        })
    }

    /// The common name of the subject, or its full name when there is none.
    pub fn subject(&self) -> String {
        display_name(&self.inner.tbs_certificate.subject)
    }

    /// The common name of the issuer, or its full name when there is none.
    pub fn issuer(&self) -> String {
        display_name(&self.inner.tbs_certificate.issuer)
    }

    /// First moment the certificate is valid, in seconds since 1970 UTC.
    pub fn not_before(&self) -> i64 {
        unix(self.inner.tbs_certificate.validity.not_before)
    }

    /// Last moment the certificate is valid, in seconds since 1970 UTC.
    pub fn not_after(&self) -> i64 {
        unix(self.inner.tbs_certificate.validity.not_after)
    }

    /// True when the subject and the issuer are the same name.
    pub fn is_self_issued(&self) -> bool {
        same_name(
            &self.inner.tbs_certificate.subject,
            &self.inner.tbs_certificate.issuer,
        )
    }

    /// True when the certificate may issue other certificates.
    pub fn is_ca(&self) -> bool {
        match self.inner.tbs_certificate.get::<BasicConstraints>() {
            Ok(Some((_, bc))) => bc.ca,
            // A version 1 root has no extensions at all.
            _ => self.inner.tbs_certificate.extensions.is_none() && self.is_self_issued(),
        }
    }

    /// The longest chain of CAs allowed below this one, when it says.
    pub fn path_length(&self) -> Option<u8> {
        match self.inner.tbs_certificate.get::<BasicConstraints>() {
            Ok(Some((_, bc))) => bc.path_len_constraint,
            _ => None,
        }
    }

    /// True when the key may sign documents: no key usage extension, or one that allows
    /// signatures or non-repudiation.
    pub fn may_sign_documents(&self) -> bool {
        match self.inner.tbs_certificate.get::<KeyUsage>() {
            Ok(Some((_, usage))) => {
                usage.0.contains(KeyUsages::DigitalSignature)
                    || usage.0.contains(KeyUsages::NonRepudiation)
            }
            _ => true,
        }
    }

    /// True when the key may sign certificates and revocation lists, or no usage is limited.
    pub fn may_sign_certificates(&self) -> bool {
        match self.inner.tbs_certificate.get::<KeyUsage>() {
            Ok(Some((_, usage))) => usage.0.contains(KeyUsages::KeyCertSign),
            _ => true,
        }
    }

    /// Whether the certificate allows `purpose`: `None` when it names no purposes at all,
    /// otherwise whether the purpose (or any purpose) is among them.
    pub fn allows_purpose(&self, purpose: Oid) -> Option<bool> {
        match self.inner.tbs_certificate.get::<ExtendedKeyUsage>() {
            Ok(Some((_, usage))) => Some(
                usage
                    .0
                    .iter()
                    .any(|u| *u == purpose || *u == ANY_EXTENDED_KEY_USAGE),
            ),
            _ => None,
        }
    }

    /// The SHA-1 thumbprint as 40 upper case hex digits.
    pub fn thumbprint(&self) -> String {
        hex(&Hash::Sha1.digest(&self.der)).to_ascii_uppercase()
    }

    /// The subject key identifier, when there is one.
    pub fn key_id(&self) -> Option<Vec<u8>> {
        match self.inner.tbs_certificate.get::<SubjectKeyIdentifier>() {
            Ok(Some((_, id))) => Some(id.0.as_bytes().to_vec()),
            _ => None,
        }
    }

    /// The addresses of the given kind named in the certificate.
    pub fn addresses(&self, kind: Pointer) -> Vec<String> {
        let mut found = Vec::new();
        match kind {
            Pointer::Ocsp => {
                if let Ok(Some((_, aia))) = self
                    .inner
                    .tbs_certificate
                    .get::<AuthorityInfoAccessSyntax>()
                {
                    for item in aia.0 {
                        if item.access_method == OCSP
                            && let GeneralName::UniformResourceIdentifier(uri) =
                                item.access_location
                        {
                            found.push(uri.to_string());
                        }
                    }
                }
            }
            Pointer::Crl => {
                if let Ok(Some((_, dps))) =
                    self.inner.tbs_certificate.get::<CrlDistributionPoints>()
                {
                    for dp in dps.0 {
                        if let Some(DistributionPointName::FullName(names)) = dp.distribution_point
                        {
                            for name in names {
                                if let GeneralName::UniformResourceIdentifier(uri) = name {
                                    found.push(uri.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        found.retain(|u| u.starts_with("http://") || u.starts_with("https://"));
        found
    }

    /// Checks that `issuer` signed this certificate.
    pub fn check_issued_by(&self, issuer: &Cert) -> Result<(), String> {
        if !same_name(
            &self.inner.tbs_certificate.issuer,
            &issuer.inner.tbs_certificate.subject,
        ) {
            return Err("the issuer name does not match".to_owned());
        }
        let tbs = self
            .inner
            .tbs_certificate
            .to_der()
            .map_err(|e| e.to_string())?;
        let signature = self
            .inner
            .signature
            .as_bytes()
            .ok_or_else(|| "the certificate signature is not whole bytes".to_owned())?;
        let default =
            algo::scheme(&self.inner.signature_algorithm).map_or(Hash::Sha256, |s| match s {
                algo::Scheme::RsaPkcs1(Some(h))
                | algo::Scheme::Ecdsa(Some(h))
                | algo::Scheme::RsaPss(h) => h,
                _ => Hash::Sha256,
            });
        algo::verify(
            &issuer.inner.tbs_certificate.subject_public_key_info,
            &self.inner.signature_algorithm,
            default,
            &tbs,
            signature,
        )
    }

    /// Name of the key, for example `ECDSA P-256`.
    pub fn key_name(&self) -> String {
        algo::key_kind(&self.inner.tbs_certificate.subject_public_key_info)
            .map_or_else(|e| e, |k| k.name())
    }
}

/// Reads certificates from a file that holds one DER certificate or any number of PEM ones.
pub fn read_many(bytes: &[u8]) -> Vec<Cert> {
    if let Ok(chain) = Certificate::load_pem_chain(bytes) {
        let found: Vec<Cert> = chain
            .into_iter()
            .filter_map(|c| c.to_der().ok().and_then(|d| Cert::parse(&d).ok()))
            .collect();
        if !found.is_empty() {
            return found;
        }
    }
    Cert::parse(bytes).into_iter().collect()
}

/// True when two names are the same after DER encoding.
pub fn same_name(a: &Name, b: &Name) -> bool {
    match (a.to_der(), b.to_der()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Seconds since 1970 UTC of a certificate time.
pub fn unix(time: x509_cert::time::Time) -> i64 {
    i64::try_from(time.to_unix_duration().as_secs()).unwrap_or(i64::MAX)
}

/// Lower case hex.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn attribute_text(name: &Name, wanted: Oid) -> Option<String> {
    for rdn in &name.0 {
        for atv in rdn.0.iter() {
            if atv.oid != wanted {
                continue;
            }
            let v = &atv.value;
            if let Ok(s) = v.decode_as::<Utf8StringRef<'_>>() {
                return Some(s.as_str().to_owned());
            }
            if let Ok(s) = v.decode_as::<PrintableStringRef<'_>>() {
                return Some(s.as_str().to_owned());
            }
            if let Ok(s) = v.decode_as::<TeletexStringRef<'_>>() {
                return Some(s.as_str().to_owned());
            }
            if let Ok(s) = v.decode_as::<Ia5StringRef<'_>>() {
                return Some(s.as_str().to_owned());
            }
        }
    }
    None
}

/// A name for people: the common name, else the organization, else the full distinguished name.
pub fn display_name(name: &Name) -> String {
    attribute_text(name, COMMON_NAME)
        .or_else(|| attribute_text(name, ORGANIZATION))
        .unwrap_or_else(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::Pki;

    #[test]
    fn names_validity_and_flags_are_read() {
        let pki = Pki::ec();
        let leaf = Cert::parse(&pki.leaf_der).unwrap();
        let root = Cert::parse(&pki.root_der).unwrap();
        assert_eq!(leaf.subject(), "Test Signer");
        assert_eq!(leaf.issuer(), "Test Root");
        assert!(root.is_ca() && root.is_self_issued());
        assert!(!leaf.is_ca() && !leaf.is_self_issued());
        assert!(leaf.may_sign_documents());
        assert!(leaf.not_before() < leaf.not_after());
        assert_eq!(leaf.thumbprint().len(), 40);
        assert_eq!(leaf.key_name(), "ECDSA P-256");
    }

    #[test]
    fn the_issuer_is_checked_by_signature() {
        let pki = Pki::ec();
        let leaf = Cert::parse(&pki.leaf_der).unwrap();
        let root = Cert::parse(&pki.root_der).unwrap();
        assert!(leaf.check_issued_by(&root).is_ok());
        assert!(leaf.check_issued_by(&leaf).is_err());
        let other = Cert::parse(&Pki::ec().root_der).unwrap();
        assert!(
            leaf.check_issued_by(&other).is_err(),
            "same name, different key"
        );
    }

    #[test]
    fn certificates_are_read_from_der_and_pem() {
        let pki = Pki::ec();
        assert_eq!(read_many(&pki.root_der).len(), 1);
        let pem = pki.root_pem();
        assert_eq!(read_many(pem.as_bytes()).len(), 1);
        let both = format!("{}{}", pem, pki.leaf_pem());
        assert_eq!(read_many(both.as_bytes()).len(), 2);
        assert!(read_many(b"nothing").is_empty());
    }

    #[test]
    fn addresses_come_from_the_extensions() {
        let pki = Pki::ec_with_addresses();
        let leaf = Cert::parse(&pki.leaf_der).unwrap();
        assert_eq!(leaf.addresses(Pointer::Ocsp), vec!["http://ocsp.test/ocsp"]);
        assert_eq!(leaf.addresses(Pointer::Crl), vec!["http://ocsp.test/crl"]);
    }
}
