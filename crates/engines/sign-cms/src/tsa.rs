//! Time stamps (RFC 3161): asking a time stamp server and reading its answer.

use application::AppError;
use application::signing::WebClient;
use const_oid::ObjectIdentifier as Oid;
use der::asn1::{GeneralizedTime, OctetString, Uint};
use der::{Any, Decode, Encode, Sequence};
use domain::ErrorCode;
use rand_core::{OsRng, RngCore};
use spki::AlgorithmIdentifierOwned;

use crate::algo::{self, Hash};
use crate::certs::Cert;
use crate::cmsdoc::{self, ID_CT_TSTINFO, Parsed};

/// Largest answer accepted from a time stamp server.
const MAX_ANSWER: u64 = 1 << 20;

/// `MessageImprint ::= SEQUENCE { hashAlgorithm, hashedMessage }`
#[derive(Debug, Clone, Eq, PartialEq, Sequence)]
pub struct MessageImprint {
    /// The hash algorithm.
    pub hash_algorithm: AlgorithmIdentifierOwned,
    /// The hash of the stamped data.
    pub hashed_message: OctetString,
}

/// `TimeStampReq`
#[derive(Debug, Clone, Eq, PartialEq, Sequence)]
pub(crate) struct TimeStampReq {
    version: u8,
    pub(crate) message_imprint: MessageImprint,
    #[asn1(optional = "true")]
    req_policy: Option<Oid>,
    #[asn1(optional = "true")]
    pub(crate) nonce: Option<Uint>,
    #[asn1(default = "default_false")]
    cert_req: bool,
}

fn default_false() -> bool {
    false
}

/// `PKIStatusInfo`
#[derive(Debug, Clone, Eq, PartialEq, Sequence)]
pub(crate) struct PkiStatusInfo {
    pub(crate) status: u32,
    #[asn1(optional = "true")]
    pub(crate) status_string: Option<Any>,
    #[asn1(optional = "true")]
    pub(crate) fail_info: Option<der::asn1::BitString>,
}

/// `TimeStampResp`
#[derive(Debug, Clone, Eq, PartialEq, Sequence)]
pub(crate) struct TimeStampResp {
    pub(crate) status: PkiStatusInfo,
    #[asn1(optional = "true")]
    pub(crate) time_stamp_token: Option<Any>,
}

/// What a time stamp token says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    /// Hash of the stamped data.
    pub imprint: MessageImprint,
    /// The time, seconds since 1970 UTC.
    pub time: i64,
    /// The nonce of the request, when the server repeated it.
    pub nonce: Option<Vec<u8>>,
    /// The serial number of the stamp, as hex.
    pub serial: String,
}

fn failed(why: &str) -> AppError {
    AppError::new(
        ErrorCode::NetworkUnavailable,
        format!(
            "The time stamp server did not give a usable time stamp: {why}. Check the address, or try again later."
        ),
    )
}

/// Builds the request for a time stamp of `digest`, with a fresh nonce.
fn request(hash: Hash, digest: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut nonce = [0u8; 8];
    OsRng.fill_bytes(&mut nonce);
    // A positive number: keep the top bit clear and the first byte non-zero.
    nonce[0] = (nonce[0] & 0x7F) | 0x40;
    let req = TimeStampReq {
        version: 1,
        message_imprint: MessageImprint {
            hash_algorithm: hash.identifier(),
            hashed_message: algo::octets(digest)?,
        },
        req_policy: None,
        nonce: Some(Uint::new(&nonce).map_err(|e| e.to_string())?),
        cert_req: true,
    };
    Ok((req.to_der().map_err(|e| e.to_string())?, nonce.to_vec()))
}

/// Reads the time stamp from a token (a CMS ContentInfo) and checks the signature of the server.
/// Returns what it says, the parsed signature and the certificate of the server.
pub fn read_token(token: &[u8]) -> Result<(Stamp, Parsed, Cert), String> {
    let parsed = cmsdoc::parse(token)?;
    if parsed.content_type() != ID_CT_TSTINFO {
        return Err("the answer is not a time stamp".to_owned());
    }
    let content = parsed
        .content()
        .ok_or_else(|| "the time stamp holds no content".to_owned())?;
    let signer = parsed
        .signer()
        .ok_or_else(|| "the certificate of the time stamp server is missing".to_owned())?
        .clone();
    let hash = parsed.hash()?;
    parsed.verify(&signer, &hash.digest(&content))?;
    if !parsed.signing_certificate_matches(&signer) {
        return Err(
            "the time stamp names another certificate than the one that signed it".to_owned(),
        );
    }
    let items: Vec<Any> =
        Vec::<Any>::from_der(&content).map_err(|e| format!("damaged time stamp: {e}"))?;
    let imprint = items
        .get(2)
        .ok_or_else(|| "damaged time stamp".to_owned())
        .and_then(|a| {
            MessageImprint::from_der(&a.to_der().map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())
        })?;
    let serial = items
        .get(3)
        .and_then(|a| a.decode_as::<Uint>().ok())
        .map_or_else(String::new, |u| crate::certs::hex(u.as_bytes()));
    let time = items
        .get(4)
        .and_then(|a| a.decode_as::<GeneralizedTime>().ok())
        .map(|t| i64::try_from(t.to_unix_duration().as_secs()).unwrap_or(0))
        .ok_or_else(|| "the time stamp has no time".to_owned())?;
    let nonce = items
        .iter()
        .skip(5)
        .find_map(|a| a.decode_as::<Uint>().ok())
        .map(|u| u.as_bytes().to_vec());
    Ok((
        Stamp {
            imprint,
            time,
            nonce,
            serial,
        },
        parsed,
        signer,
    ))
}

/// True when the stamp is for exactly `data`.
pub fn stamps(stamp: &Stamp, data: &[u8]) -> bool {
    Hash::from_oid(&stamp.imprint.hash_algorithm.oid)
        .is_some_and(|h| h.digest(data) == stamp.imprint.hashed_message.as_bytes())
}

/// True when the stamp is for this digest, made with `hash`.
fn stamps_digest(stamp: &Stamp, hash: Hash, digest: &[u8]) -> bool {
    stamp.imprint.hash_algorithm.oid == hash.oid()
        && stamp.imprint.hashed_message.as_bytes() == digest
}

/// Asks the server at `url` for a time stamp of `digest` and returns the token.
///
/// `digest` was made with `hash`. The token is checked: it must stamp this digest, repeat the
/// nonce and be signed correctly. Whether the server is trusted is decided when verifying.
pub fn stamp(
    web: &dyn WebClient,
    url: &str,
    hash: Hash,
    digest: &[u8],
) -> Result<Vec<u8>, AppError> {
    let (body, nonce) = request(hash, digest)
        .map_err(|_| AppError::internal("preparing the time stamp request"))?;
    let answer = web.post(url, "application/timestamp-query", &body, MAX_ANSWER)?;
    let resp = TimeStampResp::from_der(&answer)
        .map_err(|_| failed("the answer is not a time stamp response"))?;
    if resp.status.status > 1 {
        return Err(failed(&format!(
            "the server refused (status {})",
            resp.status.status
        )));
    }
    let token = resp
        .time_stamp_token
        .ok_or_else(|| failed("the answer holds no time stamp"))?
        .to_der()
        .map_err(|_| failed("the time stamp is damaged"))?;
    let (info, _, _) = read_token(&token).map_err(|e| failed(&e))?;
    if !stamps_digest(&info, hash, digest) {
        return Err(failed("it stamps other data than what was sent"));
    }
    let echoed = info.nonce.as_deref().map(|n| {
        n.iter()
            .skip_while(|b| **b == 0)
            .copied()
            .collect::<Vec<_>>()
    });
    let sent: Vec<u8> = nonce.iter().skip_while(|b| **b == 0).copied().collect();
    if echoed.as_deref() != Some(sent.as_slice()) {
        return Err(failed(
            "it does not repeat the request number, so it could be an old answer",
        ));
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testweb::{FixedAnswer, Tsa};

    #[test]
    fn a_token_from_a_server_is_read_and_checked() {
        let tsa = Tsa::new();
        let data = b"signature value";
        let digest = Hash::Sha256.digest(data);
        let web = tsa.client();
        let token = stamp(&web, "http://tsa.test/ts", Hash::Sha256, &digest).unwrap();
        let (info, parsed, signer) = read_token(&token).unwrap();
        assert!(stamps(&info, data));
        assert!(!stamps(&info, b"other"));
        assert_eq!(signer.subject(), "Test TSA");
        assert!(!parsed.certs.is_empty());
        assert!(info.time > 1_700_000_000);
    }

    #[test]
    fn a_refusal_is_reported() {
        let tsa = Tsa::refusing();
        let web = tsa.client();
        let err = stamp(&web, "http://tsa.test/ts", Hash::Sha256, &[0u8; 32]).unwrap_err();
        assert_eq!(err.code, ErrorCode::NetworkUnavailable);
        assert!(err.message.contains("refused"), "{}", err.message);
    }

    #[test]
    fn an_answer_for_other_data_is_rejected() {
        let tsa = Tsa::stamping_other_data();
        let web = tsa.client();
        let err = stamp(&web, "http://tsa.test/ts", Hash::Sha256, &[1u8; 32]).unwrap_err();
        assert!(err.message.contains("other data"), "{}", err.message);
    }

    #[test]
    fn a_replayed_answer_is_rejected() {
        let tsa = Tsa::replaying();
        let web = tsa.client();
        let first = stamp(&web, "http://tsa.test/ts", Hash::Sha256, &[1u8; 32]);
        assert!(first.is_ok(), "the first answer matches its request");
        let second = stamp(&web, "http://tsa.test/ts", Hash::Sha256, &[1u8; 32]);
        assert!(second.unwrap_err().message.contains("request number"));
    }

    #[test]
    fn garbage_is_not_a_time_stamp() {
        let web = FixedAnswer(b"<html>nope</html>".to_vec());
        let err = stamp(&web, "http://tsa.test/ts", Hash::Sha256, &[0u8; 32]).unwrap_err();
        assert_eq!(err.code, ErrorCode::NetworkUnavailable);
        assert!(read_token(b"junk").is_err());
    }
}
