//! Revocation: asking whether a certificate was withdrawn (OCSP and revocation lists), and
//! judging the answers, whether fresh from a server or stored in a document.

use application::signing::WebClient;
use der::{Decode, Encode};
use x509_cert::crl::CertificateList;
use x509_ocsp::{
    BasicOcspResponse, CertId, CertStatus, OcspRequest, OcspResponse, OcspResponseStatus, Request,
    TbsRequest, Version,
};

use crate::algo::{self, Hash};
use crate::certs::{Cert, Pointer, same_name};

/// Largest OCSP answer or revocation list read from a server.
const MAX_ANSWER: u64 = 8 << 20;
/// How far a clock may differ from the one of a server, in seconds.
const SKEW: i64 = 300;
/// How long an answer without a next update is taken as fresh, in seconds.
const FRESH_WITHOUT_NEXT: i64 = 7 * 86_400;

/// Answers about the revocation of certificates, as DER.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Evidence {
    /// OCSP responses (the whole `OCSPResponse`).
    pub ocsp: Vec<Vec<u8>>,
    /// Certificate revocation lists.
    pub crls: Vec<Vec<u8>>,
}

impl Evidence {
    /// Adds the answers of `other`.
    pub fn merge(&mut self, other: Evidence) {
        for item in other.ocsp {
            if !self.ocsp.contains(&item) {
                self.ocsp.push(item);
            }
        }
        for item in other.crls {
            if !self.crls.contains(&item) {
                self.crls.push(item);
            }
        }
    }

    /// True when there are no answers.
    pub fn is_empty(&self) -> bool {
        self.ocsp.is_empty() && self.crls.is_empty()
    }
}

/// What the answers say about one certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Not revoked.
    Good,
    /// Revoked at this time, seconds since 1970 UTC.
    Revoked(i64),
    /// No answer that can be relied on, with the reason.
    Unknown(String),
}

/// How fresh an answer must be.
#[derive(Debug, Clone, Copy)]
pub enum Freshness {
    /// Asked just now: the answer must hold at this time.
    Live(i64),
    /// Stored with a signature made at this time: the answer must not predate it.
    Embedded(i64),
}

fn sha1(data: &[u8]) -> Vec<u8> {
    Hash::Sha1.digest(data)
}

fn cert_id(cert: &Cert, issuer: &Cert) -> Result<CertId, String> {
    let name = issuer
        .inner
        .tbs_certificate
        .subject
        .to_der()
        .map_err(|e| e.to_string())?;
    let key = issuer
        .inner
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .raw_bytes()
        .to_vec();
    Ok(CertId {
        hash_algorithm: Hash::Sha1.identifier(),
        issuer_name_hash: algo::octets(&sha1(&name))?,
        issuer_key_hash: algo::octets(&sha1(&key))?,
        serial_number: cert.inner.tbs_certificate.serial_number.clone(),
    })
}

fn unix_of(time: x509_ocsp::OcspGeneralizedTime) -> i64 {
    i64::try_from(time.0.to_unix_duration().as_secs()).unwrap_or(i64::MAX)
}

/// The request to send to an OCSP server about `cert`.
pub fn ocsp_request(cert: &Cert, issuer: &Cert) -> Result<Vec<u8>, String> {
    let request = OcspRequest {
        tbs_request: TbsRequest {
            version: Version::V1,
            requestor_name: None,
            request_list: vec![Request {
                req_cert: cert_id(cert, issuer)?,
                single_request_extensions: None,
            }],
            request_extensions: None,
        },
        optional_signature: None,
    };
    request.to_der().map_err(|e| e.to_string())
}

fn judge_window(
    this_update: i64,
    next_update: Option<i64>,
    freshness: Freshness,
) -> Result<(), String> {
    match freshness {
        Freshness::Live(now) => {
            if this_update > now + SKEW {
                return Err("the answer is dated in the future".to_owned());
            }
            let until = next_update.unwrap_or(this_update + FRESH_WITHOUT_NEXT);
            if now > until {
                return Err("the answer is out of date".to_owned());
            }
            Ok(())
        }
        Freshness::Embedded(signed) => {
            if this_update < signed - 86_400 {
                return Err("the stored answer is older than the signature".to_owned());
            }
            Ok(())
        }
    }
}

/// What one OCSP response says about `cert`, or why it says nothing.
fn from_ocsp(
    der: &[u8],
    cert: &Cert,
    issuer: &Cert,
    freshness: Freshness,
) -> Result<Outcome, String> {
    let response =
        OcspResponse::from_der(der).map_err(|_| "an OCSP answer that cannot be read".to_owned())?;
    if response.response_status != OcspResponseStatus::Successful {
        return Err("the OCSP server did not answer the question".to_owned());
    }
    let bytes = response
        .response_bytes
        .ok_or_else(|| "an OCSP answer without content".to_owned())?;
    let basic = BasicOcspResponse::from_der(bytes.response.as_bytes())
        .map_err(|_| "an OCSP answer that cannot be read".to_owned())?;
    let tbs = basic
        .tbs_response_data
        .to_der()
        .map_err(|e| e.to_string())?;
    let signature = basic
        .signature
        .as_bytes()
        .ok_or_else(|| "an OCSP signature of odd length".to_owned())?;
    // The answer is signed by the issuer, or by a responder the issuer certified.
    let mut signers: Vec<Cert> = vec![issuer.clone()];
    if let Some(extra) = &basic.certs {
        for c in extra {
            if let Ok(d) = c.to_der()
                && let Ok(candidate) = Cert::parse(&d)
                && candidate.check_issued_by(issuer).is_ok()
            {
                signers.push(candidate);
            }
        }
    }
    let signed_well = signers.iter().any(|s| {
        algo::verify(
            &s.inner.tbs_certificate.subject_public_key_info,
            &basic.signature_algorithm,
            Hash::Sha256,
            &tbs,
            signature,
        )
        .is_ok()
    });
    if !signed_well {
        return Err(
            "the OCSP answer is not signed by the issuer or a responder it certified".to_owned(),
        );
    }
    let wanted = cert_id(cert, issuer)?;
    for single in &basic.tbs_response_data.responses {
        let id = &single.cert_id;
        let same_serial = id.serial_number == wanted.serial_number;
        // Servers may hash with another algorithm than the one asked for.
        let hash = Hash::from_oid(&id.hash_algorithm.oid);
        let name_hash = hash.and_then(|h| {
            issuer
                .inner
                .tbs_certificate
                .subject
                .to_der()
                .ok()
                .map(|n| h.digest(&n))
        });
        let key_hash = hash.map(|h| {
            h.digest(
                issuer
                    .inner
                    .tbs_certificate
                    .subject_public_key_info
                    .subject_public_key
                    .raw_bytes(),
            )
        });
        let same_issuer = name_hash.as_deref() == Some(id.issuer_name_hash.as_bytes())
            && key_hash.as_deref() == Some(id.issuer_key_hash.as_bytes());
        if !(same_serial && same_issuer) {
            continue;
        }
        judge_window(
            unix_of(single.this_update),
            single.next_update.map(unix_of),
            freshness,
        )?;
        return Ok(match single.cert_status {
            CertStatus::Good(_) => Outcome::Good,
            CertStatus::Revoked(info) => Outcome::Revoked(unix_of(info.revocation_time)),
            CertStatus::Unknown(_) => {
                return Err("the OCSP server does not know this certificate".to_owned());
            }
        });
    }
    Err("the OCSP answer is about another certificate".to_owned())
}

fn crl_unix(time: x509_cert::time::Time) -> i64 {
    crate::certs::unix(time)
}

/// What one revocation list says about `cert`, or why it says nothing.
fn from_crl(
    der: &[u8],
    cert: &Cert,
    issuer: &Cert,
    freshness: Freshness,
) -> Result<Outcome, String> {
    let list = CertificateList::from_der(der)
        .map_err(|_| "a revocation list that cannot be read".to_owned())?;
    if !same_name(
        &list.tbs_cert_list.issuer,
        &issuer.inner.tbs_certificate.subject,
    ) {
        return Err("the revocation list belongs to another issuer".to_owned());
    }
    let tbs = list.tbs_cert_list.to_der().map_err(|e| e.to_string())?;
    let signature = list
        .signature
        .as_bytes()
        .ok_or_else(|| "a revocation list signature of odd length".to_owned())?;
    algo::verify(
        &issuer.inner.tbs_certificate.subject_public_key_info,
        &list.signature_algorithm,
        Hash::Sha256,
        &tbs,
        signature,
    )
    .map_err(|_| "the revocation list is not signed by the issuer".to_owned())?;
    judge_window(
        crl_unix(list.tbs_cert_list.this_update),
        list.tbs_cert_list.next_update.map(crl_unix),
        freshness,
    )?;
    let serial = &cert.inner.tbs_certificate.serial_number;
    if let Some(revoked) = &list.tbs_cert_list.revoked_certificates
        && let Some(entry) = revoked.iter().find(|r| &r.serial_number == serial)
    {
        return Ok(Outcome::Revoked(crl_unix(entry.revocation_date)));
    }
    Ok(Outcome::Good)
}

/// Judges the answers about `cert`, which `issuer` issued.
pub fn assess(cert: &Cert, issuer: &Cert, evidence: &Evidence, freshness: Freshness) -> Outcome {
    let mut reasons: Vec<String> = Vec::new();
    for der in &evidence.ocsp {
        match from_ocsp(der, cert, issuer, freshness) {
            Ok(outcome) => return outcome,
            Err(e) => reasons.push(e),
        }
    }
    for der in &evidence.crls {
        match from_crl(der, cert, issuer, freshness) {
            Ok(outcome) => return outcome,
            Err(e) => reasons.push(e),
        }
    }
    reasons.dedup();
    Outcome::Unknown(if reasons.is_empty() {
        "there is no revocation answer for it".to_owned()
    } else {
        reasons.join("; ")
    })
}

/// Asks the servers named in `cert` and returns what they answered.
pub fn fetch(web: &dyn WebClient, cert: &Cert, issuer: &Cert) -> (Evidence, Vec<String>) {
    let mut evidence = Evidence::default();
    let mut problems = Vec::new();
    for url in cert.addresses(Pointer::Ocsp) {
        match ocsp_request(cert, issuer).and_then(|body| {
            web.post(&url, "application/ocsp-request", &body, MAX_ANSWER)
                .map_err(|e| e.message)
        }) {
            Ok(answer) => {
                evidence.ocsp.push(answer);
                break;
            }
            Err(e) => problems.push(format!("OCSP server {url}: {e}")),
        }
    }
    if evidence.ocsp.is_empty() {
        for url in cert.addresses(Pointer::Crl) {
            match web.get(&url, MAX_ANSWER) {
                Ok(list) => {
                    evidence.crls.push(list);
                    break;
                }
                Err(e) => problems.push(format!("revocation list {url}: {}", e.message)),
            }
        }
    }
    if evidence.is_empty() && problems.is_empty() {
        problems.push(format!(
            "the certificate of {} names no server to ask",
            cert.subject()
        ));
    }
    (evidence, problems)
}

/// Asks for the revocation status of every certificate of the chain that has an issuer in it.
pub fn gather(web: &dyn WebClient, chain: &[Cert]) -> (Evidence, Vec<String>) {
    let mut all = Evidence::default();
    let mut problems = Vec::new();
    for pair in chain.windows(2) {
        let (e, p) = fetch(web, &pair[0], &pair[1]);
        all.merge(e);
        problems.extend(p);
    }
    (all, problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{Pki, Spec, now};
    use crate::testweb::MockWeb;

    fn t() -> i64 {
        i64::try_from(now()).unwrap()
    }

    fn pair(pki: &Pki) -> (Cert, Cert) {
        (
            Cert::parse(&pki.leaf_der).unwrap(),
            Cert::parse(&pki.issuer_der()).unwrap(),
        )
    }

    #[test]
    fn a_good_ocsp_answer_clears_the_certificate() {
        let pki = Pki::ec_with_addresses();
        let (leaf, issuer) = pair(&pki);
        let ev = Evidence {
            ocsp: vec![pki.ocsp(&leaf.der, None, t() - 60, Some(t() + 3600))],
            crls: vec![],
        };
        assert_eq!(
            assess(&leaf, &issuer, &ev, Freshness::Live(t())),
            Outcome::Good
        );
    }

    #[test]
    fn a_revoked_ocsp_answer_names_the_time() {
        let pki = Pki::ec_with_addresses();
        let (leaf, issuer) = pair(&pki);
        let at = t() - 3600;
        let ev = Evidence {
            ocsp: vec![pki.ocsp(&leaf.der, Some(at), t() - 60, Some(t() + 3600))],
            crls: vec![],
        };
        assert_eq!(
            assess(&leaf, &issuer, &ev, Freshness::Live(t())),
            Outcome::Revoked(at)
        );
    }

    #[test]
    fn an_old_answer_is_not_fresh() {
        let pki = Pki::ec_with_addresses();
        let (leaf, issuer) = pair(&pki);
        let ev = Evidence {
            ocsp: vec![pki.ocsp(&leaf.der, None, t() - 7200, Some(t() - 3600))],
            crls: vec![],
        };
        match assess(&leaf, &issuer, &ev, Freshness::Live(t())) {
            Outcome::Unknown(why) => assert!(why.contains("out of date"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_answer_signed_by_a_stranger_is_refused() {
        let pki = Pki::ec_with_addresses();
        let (leaf, issuer) = pair(&pki);
        let stranger = Pki::ec_with_addresses();
        let ev = Evidence {
            ocsp: vec![stranger.ocsp(&leaf.der, None, t() - 60, Some(t() + 3600))],
            crls: vec![],
        };
        assert!(matches!(
            assess(&leaf, &issuer, &ev, Freshness::Live(t())),
            Outcome::Unknown(_)
        ));
    }

    #[test]
    fn a_crl_lists_revoked_serials() {
        let pki = Pki::ec_with_addresses();
        let (leaf, issuer) = pair(&pki);
        let serial = leaf.inner.tbs_certificate.serial_number.as_bytes().to_vec();
        let clean = Evidence {
            ocsp: vec![],
            crls: vec![pki.crl(&[], t() - 60, Some(t() + 3600))],
        };
        assert_eq!(
            assess(&leaf, &issuer, &clean, Freshness::Live(t())),
            Outcome::Good
        );
        let at = t() - 1000;
        let listed = Evidence {
            ocsp: vec![],
            crls: vec![pki.crl(&[(serial, at)], t() - 60, Some(t() + 3600))],
        };
        assert_eq!(
            assess(&leaf, &issuer, &listed, Freshness::Live(t())),
            Outcome::Revoked(at)
        );
    }

    #[test]
    fn stored_answers_must_not_predate_the_signature() {
        let pki = Pki::ec_with_addresses();
        let (leaf, issuer) = pair(&pki);
        let signed = t();
        let after = Evidence {
            ocsp: vec![pki.ocsp(&leaf.der, None, signed + 10, None)],
            crls: vec![],
        };
        assert_eq!(
            assess(&leaf, &issuer, &after, Freshness::Embedded(signed)),
            Outcome::Good
        );
        let before = Evidence {
            ocsp: vec![pki.ocsp(&leaf.der, None, signed - 10 * 86_400, None)],
            crls: vec![],
        };
        assert!(matches!(
            assess(&leaf, &issuer, &before, Freshness::Embedded(signed)),
            Outcome::Unknown(_)
        ));
    }

    #[test]
    fn answers_are_fetched_from_the_addresses_in_the_certificate() {
        let pki = Pki::ec_with_addresses();
        let (leaf, issuer) = pair(&pki);
        let fixed = pki.ocsp(&leaf.der, None, t() - 10, Some(t() + 3600));
        let web = MockWeb::new().route("http://ocsp.test/ocsp", move |_| Ok(fixed.clone()));
        let (ev, problems) = fetch(&web, &leaf, &issuer);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(ev.ocsp.len(), 1);
        assert_eq!(
            assess(&leaf, &issuer, &ev, Freshness::Live(t())),
            Outcome::Good
        );
        assert_eq!(web.asked(), vec!["http://ocsp.test/ocsp"]);
    }

    #[test]
    fn the_crl_is_tried_when_ocsp_fails() {
        let pki = Pki::ec_with_addresses();
        let (leaf, issuer) = pair(&pki);
        let list = pki.crl(&[], t() - 10, Some(t() + 3600));
        let web = MockWeb::new().route("http://ocsp.test/crl", move |_| Ok(list.clone()));
        let (ev, problems) = fetch(&web, &leaf, &issuer);
        assert_eq!(ev.crls.len(), 1);
        assert_eq!(
            problems.len(),
            1,
            "the failed OCSP request is reported: {problems:?}"
        );
        assert_eq!(
            web.asked(),
            vec!["http://ocsp.test/ocsp", "http://ocsp.test/crl"]
        );
    }

    #[test]
    fn a_certificate_without_addresses_cannot_be_asked_about() {
        let pki = Pki::build(&Spec::default());
        let (leaf, issuer) = pair(&pki);
        let (ev, problems) = fetch(&MockWeb::new(), &leaf, &issuer);
        assert!(ev.is_empty());
        assert!(problems[0].contains("no server"), "{problems:?}");
    }

    #[test]
    fn garbage_answers_are_unknown_not_a_crash() {
        let pki = Pki::ec_with_addresses();
        let (leaf, issuer) = pair(&pki);
        let ev = Evidence {
            ocsp: vec![b"junk".to_vec()],
            crls: vec![b"junk".to_vec()],
        };
        assert!(matches!(
            assess(&leaf, &issuer, &ev, Freshness::Live(t())),
            Outcome::Unknown(_)
        ));
    }
}
