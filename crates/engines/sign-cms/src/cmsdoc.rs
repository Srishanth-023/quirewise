//! CMS SignedData (RFC 5652) as PAdES uses it: building a signature and reading one back.

use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::content_info::{CmsVersion, ContentInfo};
use cms::signed_data::{
    CertificateSet, EncapsulatedContentInfo, SignedAttributes, SignedData, SignerIdentifier,
    SignerInfo, SignerInfos,
};
use const_oid::ObjectIdentifier as Oid;
use der::asn1::{OctetString, SetOfVec};
use der::{Any, Decode, Encode, Sequence};
use spki::AlgorithmIdentifierOwned;
use x509_cert::attr::Attribute;
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::serial_number::SerialNumber;

use crate::algo::{self, Hash};
use crate::certs::{Cert, same_name};
use crate::keys::Identity;

/// Plain data, the content type of a detached PDF signature.
pub const ID_DATA: Oid = Oid::new_unwrap("1.2.840.113549.1.7.1");
/// SignedData.
pub const ID_SIGNED_DATA: Oid = Oid::new_unwrap("1.2.840.113549.1.7.2");
/// The content type attribute.
pub const ATTR_CONTENT_TYPE: Oid = Oid::new_unwrap("1.2.840.113549.1.9.3");
/// The message digest attribute.
pub const ATTR_MESSAGE_DIGEST: Oid = Oid::new_unwrap("1.2.840.113549.1.9.4");
/// The signing certificate attribute with SHA-2 hashes (ESS).
pub const ATTR_SIGNING_CERT_V2: Oid = Oid::new_unwrap("1.2.840.113549.1.9.16.2.47");
/// The signature time stamp attribute.
pub const ATTR_TIMESTAMP_TOKEN: Oid = Oid::new_unwrap("1.2.840.113549.1.9.16.2.14");
/// The content type of a time stamp token.
pub const ID_CT_TSTINFO: Oid = Oid::new_unwrap("1.2.840.113549.1.9.16.1.4");

/// `ESSCertIDv2 ::= SEQUENCE { hashAlgorithm DEFAULT sha256, certHash, issuerSerial OPTIONAL }`
#[derive(Debug, Clone, Eq, PartialEq, Sequence)]
struct EssCertIdV2 {
    #[asn1(optional = "true")]
    hash_algorithm: Option<AlgorithmIdentifierOwned>,
    cert_hash: OctetString,
    #[asn1(optional = "true")]
    issuer_serial: Option<IssuerSerial>,
}

/// `IssuerSerial ::= SEQUENCE { issuer GeneralNames, serialNumber }`
#[derive(Debug, Clone, Eq, PartialEq, Sequence)]
struct IssuerSerial {
    issuer: Vec<GeneralName>,
    serial_number: SerialNumber,
}

/// `SigningCertificateV2 ::= SEQUENCE { certs SEQUENCE OF ESSCertIDv2 }`
#[derive(Debug, Clone, Eq, PartialEq, Sequence)]
struct SigningCertificateV2 {
    certs: Vec<EssCertIdV2>,
}

/// An attribute with one value.
pub fn attribute(oid: Oid, value: &impl Encode) -> Result<Attribute, String> {
    let any =
        Any::from_der(&value.to_der().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let values = SetOfVec::try_from(vec![any]).map_err(|e| e.to_string())?;
    Ok(Attribute { oid, values })
}

fn signing_certificate(cert: &Cert, hash: Hash) -> Result<Attribute, String> {
    let id = EssCertIdV2 {
        hash_algorithm: (hash != Hash::Sha256).then(|| hash.identifier()),
        cert_hash: algo::octets(&hash.digest(&cert.der))?,
        issuer_serial: Some(IssuerSerial {
            issuer: vec![GeneralName::DirectoryName(
                cert.inner.tbs_certificate.issuer.clone(),
            )],
            serial_number: cert.inner.tbs_certificate.serial_number.clone(),
        }),
    };
    attribute(
        ATTR_SIGNING_CERT_V2,
        &SigningCertificateV2 { certs: vec![id] },
    )
}

/// What the signature covers.
pub struct Content<'a> {
    /// Content type: `ID_DATA` for a PDF, `ID_CT_TSTINFO` inside a time stamp token.
    pub oid: Oid,
    /// The content itself when it travels inside the signature, `None` when detached.
    pub attached: Option<&'a [u8]>,
    /// The hash of the content, made with the hash of the key.
    pub digest: Vec<u8>,
}

/// A signature made but not yet encoded, so that a time stamp of its value can be added.
pub struct Built {
    signed_data: SignedData,
    /// The signature value, which a time stamp is requested for.
    pub signature: Vec<u8>,
}

/// Signs `content` with the identity.
pub fn sign(identity: &Identity, content: &Content<'_>) -> Result<Built, String> {
    let hash = identity.key.hash();
    let signer = identity.signer();
    let mut attrs: Vec<Attribute> = vec![
        attribute(ATTR_CONTENT_TYPE, &content.oid)?,
        attribute(ATTR_MESSAGE_DIGEST, &algo::octets(&content.digest)?)?,
        signing_certificate(signer, hash)?,
    ];
    let signed_attrs: SignedAttributes =
        SetOfVec::try_from(std::mem::take(&mut attrs)).map_err(|e| e.to_string())?;
    // The signature covers the attributes with the SET tag, not the context tag they carry in
    // the SignerInfo.
    let to_sign = signed_attrs.to_der().map_err(|e| e.to_string())?;
    let signature = identity.key.sign_digest(&hash.digest(&to_sign))?;
    let info = SignerInfo {
        version: CmsVersion::V1,
        sid: SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
            issuer: signer.inner.tbs_certificate.issuer.clone(),
            serial_number: signer.inner.tbs_certificate.serial_number.clone(),
        }),
        digest_alg: hash.identifier(),
        signed_attrs: Some(signed_attrs),
        signature_algorithm: identity.key.algorithm(),
        signature: algo::octets(&signature)?,
        unsigned_attrs: None,
    };
    let certificates = SetOfVec::try_from(
        identity
            .chain
            .iter()
            .map(|c| Certificate::from_der(&c.der).map(CertificateChoices::Certificate))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let econtent = match content.attached {
        Some(bytes) => Some(
            Any::from_der(
                &OctetString::new(bytes.to_vec())
                    .map_err(|e| e.to_string())?
                    .to_der()
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?,
        ),
        None => None,
    };
    let signed_data = SignedData {
        version: if content.oid == ID_DATA {
            CmsVersion::V1
        } else {
            CmsVersion::V3
        },
        digest_algorithms: SetOfVec::try_from(vec![hash.identifier()])
            .map_err(|e| e.to_string())?,
        encap_content_info: EncapsulatedContentInfo {
            econtent_type: content.oid,
            econtent,
        },
        certificates: Some(CertificateSet(certificates)),
        crls: None,
        signer_infos: SignerInfos(SetOfVec::try_from(vec![info]).map_err(|e| e.to_string())?),
    };
    Ok(Built {
        signed_data,
        signature,
    })
}

use x509_cert::Certificate;

impl Built {
    /// Encodes the signature, with unsigned attributes such as a time stamp token when given.
    pub fn to_der(&self, unsigned: Vec<Attribute>) -> Result<Vec<u8>, String> {
        let mut data = self.signed_data.clone();
        if !unsigned.is_empty() {
            let mut info = data
                .signer_infos
                .0
                .iter()
                .next()
                .cloned()
                .ok_or_else(|| "no signer".to_owned())?;
            info.unsigned_attrs = Some(SetOfVec::try_from(unsigned).map_err(|e| e.to_string())?);
            data.signer_infos =
                SignerInfos(SetOfVec::try_from(vec![info]).map_err(|e| e.to_string())?);
        }
        let content =
            Any::from_der(&data.to_der().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        ContentInfo {
            content_type: ID_SIGNED_DATA,
            content,
        }
        .to_der()
        .map_err(|e| e.to_string())
    }
}

/// A signature read from a file.
pub struct Parsed {
    data: SignedData,
    info: SignerInfo,
    /// The certificates stored in the signature.
    pub certs: Vec<Cert>,
}

/// Reads a CMS signature. Trailing zero bytes, which fill the space reserved in a PDF, are
/// ignored.
pub fn parse(bytes: &[u8]) -> Result<Parsed, String> {
    let der = trim_padding(bytes)?;
    let outer = ContentInfo::from_der(der)
        .map_err(|e| format!("the signature is not a CMS structure: {e}"))?;
    if outer.content_type != ID_SIGNED_DATA {
        return Err("the signature is not signed data".to_owned());
    }
    let data = outer
        .content
        .decode_as::<SignedData>()
        .map_err(|e| format!("the signed data is damaged: {e}"))?;
    let mut infos = data.signer_infos.0.iter();
    let info = infos
        .next()
        .cloned()
        .ok_or_else(|| "the signature has no signer".to_owned())?;
    if infos.next().is_some() {
        return Err("signatures with several signers are not supported".to_owned());
    }
    let mut certs = Vec::new();
    if let Some(set) = &data.certificates {
        for choice in set.0.iter() {
            if let CertificateChoices::Certificate(c) = choice {
                let der = c.to_der().map_err(|e| e.to_string())?;
                certs.push(Cert::parse(&der)?);
            }
        }
    }
    Ok(Parsed { data, info, certs })
}

/// The DER value at the start of `bytes`, without the zero fill after it.
pub fn trim_padding(bytes: &[u8]) -> Result<&[u8], String> {
    let mut reader = der::SliceReader::new(bytes).map_err(|e| e.to_string())?;
    let header = <der::Header as Decode>::decode(&mut reader).map_err(|e| e.to_string())?;
    let total = usize::try_from(u32::from(header.length))
        .map_err(|_| "a length that is too large".to_owned())?
        + usize::try_from(u32::from(header.encoded_len().map_err(|e| e.to_string())?))
            .map_err(|_| "a length that is too large".to_owned())?;
    bytes
        .get(..total)
        .ok_or_else(|| "the signature is cut short".to_owned())
}

impl Parsed {
    /// The hash named by the signer.
    pub fn hash(&self) -> Result<Hash, String> {
        Hash::from_oid(&self.info.digest_alg.oid)
            .ok_or_else(|| format!("the hash {} is not supported", self.info.digest_alg.oid))
    }

    /// The signature value.
    pub fn signature(&self) -> &[u8] {
        self.info.signature.as_bytes()
    }

    /// The certificate of the signer, found in the signature by the identifier it names.
    pub fn signer(&self) -> Option<&Cert> {
        match &self.info.sid {
            SignerIdentifier::IssuerAndSerialNumber(id) => self.certs.iter().find(|c| {
                c.inner.tbs_certificate.serial_number == id.serial_number
                    && same_name(&c.inner.tbs_certificate.issuer, &id.issuer)
            }),
            SignerIdentifier::SubjectKeyIdentifier(id) => self
                .certs
                .iter()
                .find(|c| c.key_id().as_deref() == Some(id.0.as_bytes())),
        }
    }

    fn attr(&self, oid: Oid) -> Option<Any> {
        let attrs = self.info.signed_attrs.as_ref()?;
        attrs
            .iter()
            .find(|a| a.oid == oid)
            .and_then(|a| a.values.iter().next().cloned())
    }

    /// The digest of the content the signer put in the signed attributes.
    pub fn message_digest(&self) -> Option<Vec<u8>> {
        self.attr(ATTR_MESSAGE_DIGEST)
            .and_then(|a| a.decode_as::<OctetString>().ok())
            .map(|o| o.into_bytes())
    }

    /// The content that travels inside the signature, as time stamp tokens do.
    pub fn content(&self) -> Option<Vec<u8>> {
        let any = self.data.encap_content_info.econtent.as_ref()?;
        any.decode_as::<OctetString>().ok().map(|o| o.into_bytes())
    }

    /// The content type of the signed content.
    pub fn content_type(&self) -> Oid {
        self.data.encap_content_info.econtent_type
    }

    /// Checks the signature over the signed attributes and that they name `content_digest`.
    pub fn verify(&self, signer: &Cert, content_digest: &[u8]) -> Result<(), String> {
        let attrs =
            self.info.signed_attrs.as_ref().ok_or_else(|| {
                "signatures without signed attributes are not supported".to_owned()
            })?;
        let named = self
            .message_digest()
            .ok_or_else(|| "the signature does not name the digest of the document".to_owned())?;
        if named != content_digest {
            return Err("the document does not match the signature".to_owned());
        }
        if let Some(any) = self.attr(ATTR_CONTENT_TYPE)
            && any.decode_as::<Oid>().ok() != Some(self.content_type())
        {
            return Err("the content type of the signature is inconsistent".to_owned());
        }
        let hash = self.hash()?;
        let signed = attrs.to_der().map_err(|e| e.to_string())?;
        algo::verify(
            &signer.inner.tbs_certificate.subject_public_key_info,
            &self.info.signature_algorithm,
            hash,
            &signed,
            self.signature(),
        )
    }

    /// Checks that the signing certificate attribute, when there is one, names the certificate
    /// that was used. Returns false when it names another.
    pub fn signing_certificate_matches(&self, signer: &Cert) -> bool {
        let Some(any) = self.attr(ATTR_SIGNING_CERT_V2) else {
            return true;
        };
        let Ok(value) = any.decode_as::<SigningCertificateV2>() else {
            return false;
        };
        let Some(first) = value.certs.first() else {
            return false;
        };
        let hash = first
            .hash_algorithm
            .as_ref()
            .and_then(|a| Hash::from_oid(&a.oid))
            .unwrap_or(Hash::Sha256);
        hash.digest(&signer.der) == first.cert_hash.as_bytes()
    }

    /// The time stamp token of the signature, as a DER ContentInfo.
    pub fn timestamp_token(&self) -> Option<Vec<u8>> {
        let attrs = self.info.unsigned_attrs.as_ref()?;
        let attr = attrs.iter().find(|a| a.oid == ATTR_TIMESTAMP_TOKEN)?;
        attr.values.iter().next().and_then(|v| v.to_der().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::load_pfx;
    use crate::testkit::{LeafKind, Pki, Spec};

    fn identity(kind: LeafKind) -> Identity {
        let pki = Pki::build(&Spec {
            kind,
            ..Spec::default()
        });
        load_pfx(&pki.pfx("pw"), "pw").unwrap()
    }

    fn document_digest(id: &Identity) -> Vec<u8> {
        id.key.hash().digest(b"the document bytes")
    }

    #[test]
    fn a_detached_signature_reads_back_and_verifies() {
        for kind in [LeafKind::P256, LeafKind::P384, LeafKind::Rsa] {
            let id = identity(kind);
            let digest = document_digest(&id);
            let built = sign(
                &id,
                &Content {
                    oid: ID_DATA,
                    attached: None,
                    digest: digest.clone(),
                },
            )
            .unwrap();
            let der = built.to_der(Vec::new()).unwrap();
            let parsed = parse(&der).unwrap();
            let signer = parsed.signer().expect("the signer certificate is inside");
            assert_eq!(signer.subject(), "Test Signer");
            assert!(parsed.verify(signer, &digest).is_ok(), "{kind:?}");
            assert!(parsed.signing_certificate_matches(signer));
            assert_eq!(parsed.certs.len(), 2, "signer and root");
            assert!(
                parsed.verify(signer, &[0u8; 32]).is_err(),
                "a different document must not verify"
            );
        }
    }

    #[test]
    fn zero_fill_after_the_signature_is_ignored() {
        let id = identity(LeafKind::P256);
        let digest = document_digest(&id);
        let built = sign(
            &id,
            &Content {
                oid: ID_DATA,
                attached: None,
                digest: digest.clone(),
            },
        )
        .unwrap();
        let mut der = built.to_der(Vec::new()).unwrap();
        der.extend(std::iter::repeat_n(0u8, 500));
        let parsed = parse(&der).unwrap();
        assert!(parsed.verify(parsed.signer().unwrap(), &digest).is_ok());
    }

    #[test]
    fn a_changed_signature_value_is_caught() {
        let id = identity(LeafKind::P256);
        let digest = document_digest(&id);
        let built = sign(
            &id,
            &Content {
                oid: ID_DATA,
                attached: None,
                digest: digest.clone(),
            },
        )
        .unwrap();
        let mut der = built.to_der(Vec::new()).unwrap();
        // Flip a bit in the last bytes, which belong to the signature value.
        let n = der.len();
        der[n - 5] ^= 0x01;
        if let Ok(parsed) = parse(&der) {
            assert!(parsed.verify(parsed.signer().unwrap(), &digest).is_err());
        }
    }

    #[test]
    fn attached_content_travels_inside() {
        let id = identity(LeafKind::P256);
        let content = b"inside";
        let built = sign(
            &id,
            &Content {
                oid: ID_CT_TSTINFO,
                attached: Some(content),
                digest: id.key.hash().digest(content),
            },
        )
        .unwrap();
        let parsed = parse(&built.to_der(Vec::new()).unwrap()).unwrap();
        assert_eq!(parsed.content().as_deref(), Some(&content[..]));
        assert_eq!(parsed.content_type(), ID_CT_TSTINFO);
    }

    #[test]
    fn unsigned_attributes_carry_a_token() {
        let id = identity(LeafKind::P256);
        let digest = document_digest(&id);
        let built = sign(
            &id,
            &Content {
                oid: ID_DATA,
                attached: None,
                digest,
            },
        )
        .unwrap();
        let token = attribute(
            ATTR_TIMESTAMP_TOKEN,
            &OctetString::new(vec![1, 2, 3]).unwrap(),
        )
        .unwrap();
        let parsed = parse(&built.to_der(vec![token]).unwrap()).unwrap();
        assert!(parsed.timestamp_token().is_some());
        let plain = parse(&built.to_der(Vec::new()).unwrap()).unwrap();
        assert!(plain.timestamp_token().is_none());
    }

    #[test]
    fn garbage_is_refused_with_a_reason() {
        assert!(parse(b"garbage").is_err());
        assert!(parse(&[]).is_err());
    }
}
