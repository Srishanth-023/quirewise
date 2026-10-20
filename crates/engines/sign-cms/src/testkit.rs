//! Certificate authorities, keys and PFX files made on the fly, for tests.
//!
//! Only compiled for tests and with the `testkit` feature. Nothing here is a secret: every key
//! is generated for one test run.
// Test helpers fail the test at once when something is wrong, so they may unwrap.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::time::Duration;

use der::asn1::{Ia5String, UtcTime};
use der::{Decode, Encode};
use ecdsa::SigningKey;
use p256::NistP256;
use p256::pkcs8::EncodePrivateKey;
use p384::NistP384;
use rand_core::OsRng;
use rsa::RsaPrivateKey;
use sha2::Sha256;
use signature::{Keypair, Signer};
use spki::{
    AlgorithmIdentifierOwned, DynSignatureAlgorithmIdentifier, EncodePublicKey,
    SignatureBitStringEncoding, SubjectPublicKeyInfoOwned,
};
use x509_cert::Certificate;
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::ext::pkix::name::{DistributionPointName, GeneralName};
use x509_cert::ext::pkix::{
    AccessDescription, AuthorityInfoAccessSyntax, CrlDistributionPoints, crl::dp::DistributionPoint,
};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::time::{Time, Validity};

use crate::algo;

/// The kind of key a test certificate gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeafKind {
    /// ECDSA on P-256.
    P256,
    /// ECDSA on P-384.
    P384,
    /// RSA with 2048 bits.
    Rsa,
}

/// What a test authority should look like.
#[derive(Debug, Clone)]
pub struct Spec {
    /// Key of the signer certificate.
    pub kind: LeafKind,
    /// Put OCSP, CRL and issuer addresses in the signer certificate.
    pub addresses: bool,
    /// Issue the signer certificate for a period that has ended.
    pub expired: bool,
    /// Add an intermediate authority between the root and the signer.
    pub intermediate: bool,
    /// The name of the signer certificate.
    pub leaf_name: String,
    /// The name of the root, so that two authorities in one test do not share a name.
    pub root_name: String,
    /// Make the signer certificate one for a time stamp server.
    pub time_stamping: bool,
    /// The address the servers of the authority listen at, for the addresses in the signer
    /// certificate. Default `http://ocsp.test`-style names that nothing answers.
    pub base_url: Option<String>,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            kind: LeafKind::P256,
            addresses: false,
            expired: false,
            intermediate: false,
            leaf_name: "Test Signer".to_owned(),
            root_name: "Test Root".to_owned(),
            time_stamping: false,
            base_url: None,
        }
    }
}

/// A private key made for a test.
pub enum TestKey {
    /// P-256.
    P256(SigningKey<NistP256>),
    /// P-384.
    P384(SigningKey<NistP384>),
    /// RSA.
    Rsa(Box<RsaPrivateKey>),
}

/// Extensions a test certificate may carry.
#[derive(Clone)]
enum Addresses {
    None,
    Some(String),
    /// The purpose of a time stamp server.
    TimeStamping,
}

fn text(s: &str) -> Ia5String {
    Ia5String::new(s).expect("ascii")
}

impl TestKey {
    /// Makes a key of `kind`.
    pub fn new(kind: LeafKind) -> Self {
        match kind {
            LeafKind::P256 => Self::P256(SigningKey::random(&mut OsRng)),
            LeafKind::P384 => Self::P384(SigningKey::random(&mut OsRng)),
            LeafKind::Rsa => Self::Rsa(Box::new(
                RsaPrivateKey::new(&mut OsRng, 2048).expect("an RSA key"),
            )),
        }
    }

    /// The key as PKCS#8 DER.
    pub fn pkcs8(&self) -> Vec<u8> {
        match self {
            Self::P256(k) => k.to_pkcs8_der().expect("pkcs8").as_bytes().to_vec(),
            Self::P384(k) => k.to_pkcs8_der().expect("pkcs8").as_bytes().to_vec(),
            Self::Rsa(k) => rsa::pkcs8::EncodePrivateKey::to_pkcs8_der(&**k)
                .expect("pkcs8")
                .as_bytes()
                .to_vec(),
        }
    }

    /// The public key.
    pub fn public_info(&self) -> SubjectPublicKeyInfoOwned {
        let der = match self {
            Self::P256(k) => EncodePublicKey::to_public_key_der(k.verifying_key()),
            Self::P384(k) => EncodePublicKey::to_public_key_der(k.verifying_key()),
            Self::Rsa(k) => EncodePublicKey::to_public_key_der(&rsa::RsaPublicKey::from(&**k)),
        }
        .expect("public key");
        SubjectPublicKeyInfoOwned::from_der(der.as_bytes()).expect("spki")
    }

    /// Signs `message` and returns the algorithm identifier and the signature bytes, in the
    /// form certificates and revocation lists use.
    pub fn sign(&self, message: &[u8]) -> (AlgorithmIdentifierOwned, Vec<u8>) {
        match self {
            Self::P256(k) => {
                let s: p256::ecdsa::DerSignature = k.sign(message);
                (
                    AlgorithmIdentifierOwned {
                        oid: algo::ECDSA_SHA256,
                        parameters: None,
                    },
                    s.to_bytes().to_vec(),
                )
            }
            Self::P384(k) => {
                let s: p384::ecdsa::DerSignature = k.sign(message);
                (
                    AlgorithmIdentifierOwned {
                        oid: algo::ECDSA_SHA384,
                        parameters: None,
                    },
                    s.to_bytes().to_vec(),
                )
            }
            Self::Rsa(k) => {
                let sk = rsa::pkcs1v15::SigningKey::<Sha256>::new((**k).clone());
                let s: rsa::pkcs1v15::Signature = sk.sign(message);
                (
                    AlgorithmIdentifierOwned {
                        oid: algo::SHA256_WITH_RSA,
                        parameters: Some(der::Any::from(der::asn1::Null)),
                    },
                    signature::SignatureEncoding::to_vec(&s),
                )
            }
        }
    }

    fn issue(
        &self,
        profile: Profile,
        serial: u32,
        validity: Validity,
        subject: Name,
        subject_key: SubjectPublicKeyInfoOwned,
        addresses: &Addresses,
    ) -> Vec<u8> {
        match self {
            Self::P256(k) => issue::<_, p256::ecdsa::DerSignature>(
                k,
                profile,
                serial,
                validity,
                subject,
                subject_key,
                addresses,
            ),
            Self::P384(k) => issue::<_, p384::ecdsa::DerSignature>(
                k,
                profile,
                serial,
                validity,
                subject,
                subject_key,
                addresses,
            ),
            Self::Rsa(k) => {
                let sk = rsa::pkcs1v15::SigningKey::<Sha256>::new((**k).clone());
                issue::<_, rsa::pkcs1v15::Signature>(
                    &sk,
                    profile,
                    serial,
                    validity,
                    subject,
                    subject_key,
                    addresses,
                )
            }
        }
    }
}

fn issue<S, Sig>(
    signer: &S,
    profile: Profile,
    serial: u32,
    validity: Validity,
    subject: Name,
    subject_key: SubjectPublicKeyInfoOwned,
    addresses: &Addresses,
) -> Vec<u8>
where
    S: Keypair + DynSignatureAlgorithmIdentifier + Signer<Sig>,
    S::VerifyingKey: EncodePublicKey,
    Sig: SignatureBitStringEncoding,
{
    let mut builder = CertificateBuilder::new(
        profile,
        SerialNumber::from(serial),
        validity,
        subject,
        subject_key,
        signer,
    )
    .expect("a builder");
    if matches!(addresses, Addresses::TimeStamping) {
        builder
            .add_extension(&x509_cert::ext::pkix::ExtendedKeyUsage(vec![
                crate::certs::TIME_STAMPING,
            ]))
            .expect("a key usage");
    }
    if let Addresses::Some(base) = addresses {
        let uri = |s: &str| GeneralName::UniformResourceIdentifier(text(s));
        builder
            .add_extension(&AuthorityInfoAccessSyntax(vec![
                AccessDescription {
                    access_method: "1.3.6.1.5.5.7.48.1".parse().expect("oid"),
                    access_location: uri(&format!("{base}/ocsp")),
                },
                AccessDescription {
                    access_method: "1.3.6.1.5.5.7.48.2".parse().expect("oid"),
                    access_location: uri(&format!("{base}/ca.cer")),
                },
            ]))
            .expect("aia");
        builder
            .add_extension(&CrlDistributionPoints(vec![DistributionPoint {
                distribution_point: Some(DistributionPointName::FullName(vec![uri(&format!(
                    "{base}/crl"
                ))])),
                reasons: None,
                crl_issuer: None,
            }]))
            .expect("crl dp");
    }
    let cert: Certificate = builder.build::<Sig>().expect("a certificate");
    cert.to_der().expect("der")
}

fn name(common: &str) -> Name {
    format!("CN={common}").parse().expect("a name")
}

/// A validity from `from` to `to`, in seconds since 1970.
pub fn period(from: u64, to: u64) -> Validity {
    let time = |s: u64| {
        Time::UtcTime(UtcTime::from_unix_duration(Duration::from_secs(s)).expect("a time"))
    };
    Validity {
        not_before: time(from),
        not_after: time(to),
    }
}

/// Seconds since 1970 now.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock")
        .as_secs()
}

/// A root authority, an optional intermediate one and a signer certificate, with their keys.
pub struct Pki {
    /// Key of the root.
    pub root_key: TestKey,
    /// The root certificate.
    pub root_der: Vec<u8>,
    /// Key and certificate of the intermediate authority.
    pub inter: Option<(TestKey, Vec<u8>)>,
    /// Key of the signer.
    pub leaf_key: TestKey,
    /// The signer certificate.
    pub leaf_der: Vec<u8>,
}

impl Pki {
    /// A P-256 authority with a P-256 signer called `Test Signer`.
    pub fn ec() -> Self {
        Self::build(&Spec::default())
    }

    /// Like [`Pki::ec`] with addresses in the signer certificate.
    pub fn ec_with_addresses() -> Self {
        Self::build(&Spec {
            addresses: true,
            ..Spec::default()
        })
    }

    /// An authority from a specification.
    pub fn build(spec: &Spec) -> Self {
        let t = now();
        let long = period(t - 1000 * 86_400, t + 20 * 365 * 86_400);
        let root_key = TestKey::new(LeafKind::P256);
        let root_name = name(&spec.root_name);
        let root_der = root_key.issue(
            Profile::Root,
            1,
            long,
            root_name.clone(),
            root_key.public_info(),
            &Addresses::None,
        );
        let (issuer_key, issuer_name, inter) = if spec.intermediate {
            let key = TestKey::new(LeafKind::P256);
            let inter_name = name("Test Intermediate");
            let der = root_key.issue(
                Profile::SubCA {
                    issuer: root_name.clone(),
                    path_len_constraint: Some(0),
                },
                2,
                long,
                inter_name.clone(),
                key.public_info(),
                &Addresses::None,
            );
            (None, inter_name, Some((key, der)))
        } else {
            (Some(()), root_name, None)
        };
        let leaf_key = TestKey::new(spec.kind);
        let validity = if spec.expired {
            period(t - 400 * 86_400, t - 30 * 86_400)
        } else {
            period(t - 86_400, t + 10 * 365 * 86_400)
        };
        let signer: &TestKey = match (&issuer_key, &inter) {
            (None, Some((key, _))) => key,
            _ => &root_key,
        };
        let leaf_der = signer.issue(
            Profile::Leaf {
                issuer: issuer_name,
                enable_key_agreement: false,
                enable_key_encipherment: false,
            },
            3,
            validity,
            name(&spec.leaf_name),
            leaf_key.public_info(),
            &if spec.time_stamping {
                Addresses::TimeStamping
            } else if spec.addresses {
                Addresses::Some(
                    spec.base_url
                        .clone()
                        .unwrap_or_else(|| "http://ocsp.test".to_owned()),
                )
            } else {
                Addresses::None
            },
        );
        Self {
            root_key,
            root_der,
            inter,
            leaf_key,
            leaf_der,
        }
    }

    /// The certificates from the signer up to the root.
    pub fn chain(&self) -> Vec<Vec<u8>> {
        let mut chain = vec![self.leaf_der.clone()];
        if let Some((_, der)) = &self.inter {
            chain.push(der.clone());
        }
        chain.push(self.root_der.clone());
        chain
    }

    /// A PFX file with the signer key and the whole chain.
    pub fn pfx(&self, password: &str) -> Vec<u8> {
        use p12_keystore::{
            Certificate as P12Cert, KeyStore, KeyStoreEntry, PrivateKey, PrivateKeyChain,
        };
        let certs: Vec<P12Cert> = self
            .chain()
            .iter()
            .map(|d| P12Cert::from_der(d).expect("a certificate"))
            .collect();
        let key = PrivateKey::from_der(&self.leaf_key.pkcs8()).expect("a key");
        let mut store = KeyStore::new();
        store.add_entry(
            "signer",
            KeyStoreEntry::PrivateKeyChain(PrivateKeyChain::new(b"1".to_vec(), key, certs)),
        );
        store.writer(password).write().expect("a pfx")
    }

    /// The certificate that issued the signer certificate.
    pub fn issuer_der(&self) -> Vec<u8> {
        match &self.inter {
            Some((_, der)) => der.clone(),
            None => self.root_der.clone(),
        }
    }

    /// A signer certificate named `common` for a public key that was made elsewhere, such as in
    /// the Windows key store.
    pub fn issue_for(&self, key: SubjectPublicKeyInfoOwned, common: &str) -> Vec<u8> {
        let issuer = Certificate::from_der(&self.issuer_der()).expect("a certificate");
        let t = now();
        self.issuer_key().issue(
            Profile::Leaf {
                issuer: issuer.tbs_certificate.subject.clone(),
                enable_key_agreement: false,
                enable_key_encipherment: false,
            },
            7,
            period(t - 86_400, t + 365 * 86_400),
            name(common),
            key,
            &Addresses::None,
        )
    }

    fn issuer_key(&self) -> &TestKey {
        match &self.inter {
            Some((key, _)) => key,
            None => &self.root_key,
        }
    }

    /// An OCSP response about `cert`, signed by its issuer: good, or revoked at `revoked_at`.
    pub fn ocsp(
        &self,
        cert_der: &[u8],
        revoked_at: Option<i64>,
        this_update: i64,
        next_update: Option<i64>,
    ) -> Vec<u8> {
        use sha1::Digest;
        use x509_ocsp::{
            BasicOcspResponse, CertId, CertStatus, OcspGeneralizedTime, OcspResponse, ResponderId,
            ResponseData, RevokedInfo, SingleResponse,
        };
        let cert = Certificate::from_der(cert_der).expect("a certificate");
        let issuer = Certificate::from_der(&self.issuer_der()).expect("a certificate");
        let name_der = issuer.tbs_certificate.subject.to_der().expect("der");
        let key_bits = issuer
            .tbs_certificate
            .subject_public_key_info
            .subject_public_key
            .raw_bytes()
            .to_vec();
        let general = |unix: i64| {
            OcspGeneralizedTime(
                der::asn1::GeneralizedTime::from_unix_duration(Duration::from_secs(
                    u64::try_from(unix).expect("a time"),
                ))
                .expect("a time"),
            )
        };
        let status = match revoked_at {
            None => CertStatus::good(),
            Some(at) => CertStatus::revoked(RevokedInfo {
                revocation_time: general(at),
                revocation_reason: None,
            }),
        };
        let single = SingleResponse {
            cert_id: CertId {
                hash_algorithm: algo::Hash::Sha1.identifier(),
                issuer_name_hash: der::asn1::OctetString::new(
                    sha1::Sha1::digest(&name_der).to_vec(),
                )
                .expect("octets"),
                issuer_key_hash: der::asn1::OctetString::new(
                    sha1::Sha1::digest(&key_bits).to_vec(),
                )
                .expect("octets"),
                serial_number: cert.tbs_certificate.serial_number.clone(),
            },
            cert_status: status,
            this_update: general(this_update),
            next_update: next_update.map(general),
            single_extensions: None,
        };
        let data = ResponseData {
            version: Default::default(),
            responder_id: ResponderId::ByName(issuer.tbs_certificate.subject.clone()),
            produced_at: general(this_update),
            responses: vec![single],
            response_extensions: None,
        };
        let (algorithm, signature) = self.issuer_key().sign(&data.to_der().expect("der"));
        let basic = BasicOcspResponse {
            tbs_response_data: data,
            signature_algorithm: algorithm,
            signature: der::asn1::BitString::from_bytes(&signature).expect("bits"),
            certs: None,
        };
        OcspResponse::successful(basic)
            .expect("a response")
            .to_der()
            .expect("der")
    }

    /// A revocation list of the issuer of the signer, listing `(serial, revoked at)` pairs.
    pub fn crl(
        &self,
        revoked: &[(Vec<u8>, i64)],
        this_update: i64,
        next_update: Option<i64>,
    ) -> Vec<u8> {
        use x509_cert::crl::{CertificateList, RevokedCert, TbsCertList};
        let issuer = Certificate::from_der(&self.issuer_der()).expect("a certificate");
        let time = |unix: i64| {
            Time::UtcTime(
                UtcTime::from_unix_duration(Duration::from_secs(
                    u64::try_from(unix).expect("a time"),
                ))
                .expect("a time"),
            )
        };
        let (algorithm, _) = self.issuer_key().sign(b"");
        let entries: Vec<RevokedCert> = revoked
            .iter()
            .map(|(serial, at)| RevokedCert {
                serial_number: SerialNumber::new(serial).expect("a serial"),
                revocation_date: time(*at),
                crl_entry_extensions: None,
            })
            .collect();
        let tbs = TbsCertList {
            version: x509_cert::Version::V2,
            signature: algorithm.clone(),
            issuer: issuer.tbs_certificate.subject.clone(),
            this_update: time(this_update),
            next_update: next_update.map(time),
            revoked_certificates: (!entries.is_empty()).then_some(entries),
            crl_extensions: None,
        };
        let (_, signature) = self.issuer_key().sign(&tbs.to_der().expect("der"));
        CertificateList {
            tbs_cert_list: tbs,
            signature_algorithm: algorithm,
            signature: der::asn1::BitString::from_bytes(&signature).expect("bits"),
        }
        .to_der()
        .expect("der")
    }

    /// The root certificate as PEM.
    pub fn root_pem(&self) -> String {
        pem(&self.root_der)
    }

    /// The signer certificate as PEM.
    pub fn leaf_pem(&self) -> String {
        pem(&self.leaf_der)
    }
}

fn pem(der: &[u8]) -> String {
    use der::EncodePem;
    use der::pem::LineEnding;
    Certificate::from_der(der)
        .expect("a certificate")
        .to_pem(LineEnding::LF)
        .expect("pem")
}
