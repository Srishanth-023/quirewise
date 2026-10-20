//! Checking the signatures of a PDF and writing what was found as reports.

use application::AppError;
use application::runtime::CancellationToken;
use application::signing::{
    Check, SignatureKind, SignatureReport, Status, TimestampReport, WebClient,
};
use lopdf::Document;

use crate::algo::Hash;
use crate::certs::{Cert, TIME_STAMPING};
use crate::cmsdoc::{self, Parsed};
use crate::dss::{self, Stored};
use crate::revocation::{self, Evidence, Freshness, Outcome};
use crate::sigfind::{self, Found, Kind, Reach};
use crate::timefmt;
use crate::trust::{self, Chain, Ending, TrustStore};
use crate::tsa;

/// What the checks need besides the file.
pub struct Setup<'a> {
    /// The trusted certificates.
    pub trust: &'a TrustStore,
    /// Ask the certificate authorities about revocation, over this client.
    pub revocation: Option<&'a dyn WebClient>,
    /// The time to check validity against.
    pub now: i64,
}

/// Reports on every signature of the file.
pub fn check_all(
    bytes: &[u8],
    setup: &Setup<'_>,
    cancel: &CancellationToken,
) -> Result<Vec<SignatureReport>, AppError> {
    let doc = Document::load_mem(bytes).map_err(|_| {
        AppError::new(
            domain::ErrorCode::PdfDamaged,
            "The PDF could not be read, so its signatures cannot be checked. Run Repair first.",
        )
    })?;
    if doc.is_encrypted() || doc.trailer.has(b"Encrypt") {
        return Err(AppError::new(
            domain::ErrorCode::PdfEncrypted,
            "This PDF is protected with a password, and the signatures of a protected file cannot be checked here. Unlock a copy first.",
        ));
    }
    let stored = dss::read(&doc);
    let mut reports = Vec::new();
    for found in sigfind::find(&doc) {
        cancel.check()?;
        reports.push(check_one(&doc, bytes, &found, &stored, setup));
    }
    Ok(reports)
}

fn kind_of(kind: Kind) -> SignatureKind {
    match kind {
        Kind::Approval => SignatureKind::Approval,
        Kind::Certification(p) => SignatureKind::Certification(p),
        Kind::Timestamp => SignatureKind::DocumentTimestamp,
    }
}

fn empty_report(found: &Found) -> SignatureReport {
    SignatureReport {
        field: found.field.clone(),
        kind: kind_of(found.kind),
        signer: found.name.clone().unwrap_or_else(|| "Unknown".to_owned()),
        issuer: String::new(),
        signed_at: found.claimed.map(timefmt::utc),
        reason: found.reason.clone(),
        location: found.location.clone(),
        contact: found.contact.clone(),
        algorithm: String::new(),
        signature: Check::unknown("Not checked."),
        changes: Check::unknown("Not checked."),
        identity: Check::unknown("Not checked."),
        revocation: Check::unknown("Not checked."),
        timestamp: None,
        long_term: false,
        notes: Vec::new(),
    }
}

/// The digest of the signed ranges.
fn digest_of(bytes: &[u8], found: &Found, hash: Hash) -> Vec<u8> {
    let (head, tail) = found.signed(bytes);
    let mut h = hash.hasher();
    h.update(head);
    h.update(tail);
    h.finish()
}

fn check_one(
    doc: &Document,
    bytes: &[u8],
    found: &Found,
    stored: &Stored,
    setup: &Setup<'_>,
) -> SignatureReport {
    let mut report = empty_report(found);
    report.long_term = !stored.evidence.is_empty();

    // Do the ranges describe this file at all?
    let end = match found.reach(bytes) {
        Reach::To(end) => end,
        Reach::Broken(why) => {
            report.signature = Check::fail(format!("The signature does not fit this file: {why}."));
            return report;
        }
    };
    report.changes = changes(doc, bytes, found, end);

    let parsed = match cmsdoc::parse(&found.contents) {
        Ok(p) => p,
        Err(e) => {
            report.signature = Check::fail(format!("The signature data is damaged: {e}."));
            return report;
        }
    };
    if found.kind == Kind::Timestamp {
        check_document_timestamp(&mut report, bytes, found, &parsed, stored, setup);
        return report;
    }
    check_signature(&mut report, bytes, found, parsed, stored, setup);
    report
}

/// How the part of the file after the signature compares with what was signed.
fn changes(doc: &Document, bytes: &[u8], found: &Found, end: usize) -> Check {
    if end >= bytes.len() {
        return Check::pass("The file is exactly as it was signed.");
    }
    let t = sigfind::tail(doc, bytes, end);
    if t.content == 0 && t.forms == 0 {
        if t.validation > 0 {
            return Check::pass(
                "Only signatures, validation data or document properties were added after signing.",
            );
        }
        return Check::unknown(
            "The structure of the file was updated after signing (for example the catalog or the page list), without a signature. The signed content itself is unchanged.",
        );
    }
    if let Kind::Certification(p) = found.kind {
        if p == 1 || t.content > 0 {
            return Check::fail(
                "The file was certified, and it was changed after that in a way the certification does not allow.",
            );
        }
        return Check::unknown(
            "Form fields or comments were added after certifying. The certification allows only some of that: check who made the changes.",
        );
    }
    if t.content > 0 {
        return Check::fail(format!(
            "The content of the file was changed after signing ({} objects). The signature no longer vouches for what you see.",
            t.content + t.forms
        ));
    }
    Check::unknown(
        "Form fields or comments were added or changed after signing. The signed content itself is unchanged.",
    )
}

fn algorithm_name(parsed: &Parsed, signer: &Cert) -> String {
    let hash = parsed.hash().map_or("unknown hash", Hash::name);
    format!("{} with {hash}", signer.key_name())
}

fn check_signature(
    report: &mut SignatureReport,
    bytes: &[u8],
    found: &Found,
    parsed: Parsed,
    stored: &Stored,
    setup: &Setup<'_>,
) {
    let Some(signer) = parsed.signer().cloned() else {
        report.signature = Check::fail(
            "The certificate of the signer is not in the signature, so it cannot be checked.",
        );
        return;
    };
    report.signer = signer.subject();
    report.issuer = signer.issuer();
    report.algorithm = algorithm_name(&parsed, &signer);
    let hash = match parsed.hash() {
        Ok(h) => h,
        Err(e) => {
            report.signature = Check::fail(format!("The signature cannot be checked: {e}."));
            return;
        }
    };
    let digest = digest_of(bytes, found, hash);
    match parsed.verify(&signer, &digest) {
        Ok(()) if parsed.signing_certificate_matches(&signer) => {
            report.signature =
                Check::pass("The signature is valid: the signed part of the file is unchanged.");
        }
        Ok(()) => {
            report.signature = Check::fail(
                "The signature names a different certificate than the one that signed it.",
            );
        }
        Err(e) if e.contains("does not match the signature") => {
            report.signature = Check::fail(
                "The signed part of the file does not match the signature. The file was changed after signing, or the signature is not genuine.",
            );
        }
        Err(e) => {
            report.signature = Check::fail(format!("The signature is not valid: {e}."));
        }
    }
    if hash.is_weak() {
        report
            .notes
            .push("The signature uses SHA-1, which is no longer considered safe.".to_owned());
    }
    if let Ok(kind) = crate::algo::key_kind(&signer.inner.tbs_certificate.subject_public_key_info)
        && kind.is_weak()
    {
        report
            .notes
            .push(format!("The key of the signer is short ({}).", kind.name()));
    }
    if !signer.may_sign_documents() {
        report
            .notes
            .push("The certificate of the signer is not meant for signing documents.".to_owned());
    }

    // Time stamp inside the signature.
    let mut reference = found.claimed.unwrap_or(setup.now);
    let mut proven = false;
    let mut pool = parsed.certs.clone();
    pool.extend(stored.certs.iter().cloned());
    if let Some(token) = parsed.timestamp_token() {
        let stamp = check_token(&token, parsed.signature(), stored, setup);
        if stamp.check.status != Status::Fail
            && let Some(t) = stamp.seconds
        {
            reference = t;
            proven = true;
        }
        report.timestamp = Some(TimestampReport {
            time: stamp.time,
            authority: stamp.authority,
            check: stamp.check,
        });
    }

    // Who the signer is.
    let chain = trust::build(
        &signer,
        &pool,
        setup.trust,
        if proven { reference } else { setup.now },
    );
    report.identity = identity(&chain, &signer, proven, reference, setup, &pool);
    if report.signature.status == Status::Pass && report.timestamp.is_none() {
        report.notes.push(
            "There is no trusted time stamp, so the time the signer claims cannot be proven."
                .to_owned(),
        );
    }
    report.revocation = revocation_check(&chain, stored, setup, reference);
}

fn identity(
    chain: &Chain,
    signer: &Cert,
    proven: bool,
    reference: i64,
    setup: &Setup<'_>,
    pool: &[Cert],
) -> Check {
    let names: Vec<String> = chain.certs.iter().map(Cert::subject).collect();
    match chain.ending {
        Ending::Trusted if chain.faults.is_empty() => Check::pass(format!(
            "Issued through {} to a trusted authority.",
            names.join(" > ")
        )),
        Ending::Trusted => {
            // An expired certificate is a fault only when the time of signing is not proven and
            // the certificate was valid back then.
            if !proven {
                let then = trust::build(signer, pool, setup.trust, reference);
                if then.faults.is_empty() {
                    return Check::unknown(format!(
                        "{} The certificate was valid at the time the signer claims, but nothing proves when it was signed.",
                        chain.faults.join(" ")
                    ));
                }
            }
            Check::fail(chain.faults.join(" "))
        }
        Ending::SelfSigned => Check::unknown(
            "The certificate is self-signed. It is not trusted because it is not in the list of trusted certificates of Windows or in your file of trusted certificates.",
        ),
        Ending::Unknown => {
            let last = chain.certs.last().map_or_else(String::new, Cert::subject);
            Check::unknown(format!(
                "The chain of certificates ends at {last}, which is not trusted."
            ))
        }
    }
}

fn revocation_check(chain: &Chain, stored: &Stored, setup: &Setup<'_>, reference: i64) -> Check {
    // Pairs of a certificate and its issuer: the trusted root itself has no issuer to ask.
    let pairs: Vec<(&Cert, &Cert)> = chain.certs.windows(2).map(|w| (&w[0], &w[1])).collect();
    if pairs.is_empty() {
        return Check::unknown("There is no issuer to ask about this certificate.");
    }
    let mut evidence = stored.evidence.clone();
    let mut freshness = Freshness::Embedded(reference);
    let mut problems: Vec<String> = Vec::new();
    let mut live = false;
    if let Some(web) = setup.revocation {
        let (fetched, issues) = revocation::gather(web, &chain.certs);
        live = !fetched.is_empty();
        problems.extend(issues);
        // Fresh answers go first so that they are used before old stored ones.
        let mut combined = Evidence::default();
        combined.merge(fetched);
        combined.merge(evidence);
        evidence = combined;
        freshness = Freshness::Live(setup.now);
    }
    if evidence.is_empty() && problems.is_empty() {
        return Check::unknown(
            "Not checked. The file holds no revocation answers, and asking the certificate authorities is turned off.",
        );
    }
    let mut unknown: Vec<String> = Vec::new();
    for (cert, issuer) in pairs {
        let outcome = revocation::assess(cert, issuer, &evidence, freshness);
        let outcome = match (&outcome, live) {
            // A fresh question failed, but the file may hold an answer from signing time.
            (Outcome::Unknown(_), true) => revocation::assess(
                cert,
                issuer,
                &stored.evidence,
                Freshness::Embedded(reference),
            ),
            _ => outcome,
        };
        match outcome {
            Outcome::Good => {}
            Outcome::Revoked(at) if at <= reference => {
                return Check::fail(format!(
                    "The certificate of {} was revoked on {}, before the signature was made.",
                    cert.subject(),
                    timefmt::day(at)
                ));
            }
            Outcome::Revoked(at) => {
                unknown.push(format!(
                    "The certificate of {} was revoked on {}, after the signature was made. Whether that matters depends on why it was revoked.",
                    cert.subject(),
                    timefmt::day(at)
                ));
            }
            Outcome::Unknown(why) => {
                unknown.push(format!("No answer for {}: {why}.", cert.subject()));
            }
        }
    }
    if unknown.is_empty() {
        let source = if live {
            "The certificate authorities were asked"
        } else {
            "The answers stored in the file show"
        };
        let verb = if live {
            ": no certificate of the chain is revoked."
        } else {
            " that no certificate of the chain was revoked."
        };
        return Check::pass(format!("{source}{verb}"));
    }
    unknown.extend(problems);
    Check::unknown(unknown.join(" "))
}

struct StampCheck {
    time: String,
    authority: String,
    check: Check,
    seconds: Option<i64>,
}

/// Checks a time stamp token that stamps `data`.
fn check_token(token: &[u8], data: &[u8], stored: &Stored, setup: &Setup<'_>) -> StampCheck {
    let broken = |why: String| StampCheck {
        time: "unknown".to_owned(),
        authority: String::new(),
        check: Check::fail(why),
        seconds: None,
    };
    let (info, parsed, server) = match tsa::read_token(token) {
        Ok(v) => v,
        Err(e) => return broken(format!("The time stamp is not valid: {e}.")),
    };
    if !tsa::stamps(&info, data) {
        return broken("The time stamp is for other data than this signature.".to_owned());
    }
    let mut pool = parsed.certs.clone();
    pool.extend(stored.certs.iter().cloned());
    let chain = trust::build(&server, &pool, setup.trust, info.time);
    let check = match chain.ending {
        Ending::Trusted if chain.faults.is_empty() => Check::pass(
            "The time stamp is valid and its server is trusted: the signature existed at this time.",
        ),
        Ending::Trusted => Check::fail(chain.faults.join(" ")),
        _ => Check::unknown(
            "The time stamp is intact, but its server is not trusted, so the time is not proven.",
        ),
    };
    // A certificate that is not for time stamps must not be taken for one (RFC 3161).
    let check = match server.allows_purpose(TIME_STAMPING) {
        Some(false) => {
            Check::fail("The certificate of the time stamp server is not meant for time stamps.")
        }
        None if check.status == Status::Pass => Check::unknown(
            "The time stamp is valid and its server is trusted, but its certificate is not marked for time stamping.",
        ),
        _ => check,
    };
    StampCheck {
        time: timefmt::utc(info.time),
        authority: server.subject(),
        seconds: (check.status != Status::Fail).then_some(info.time),
        check,
    }
}

fn check_document_timestamp(
    report: &mut SignatureReport,
    bytes: &[u8],
    found: &Found,
    parsed: &Parsed,
    stored: &Stored,
    setup: &Setup<'_>,
) {
    let _ = parsed;
    let (head, tail_part) = found.signed(bytes);
    let mut data = head.to_vec();
    data.extend_from_slice(tail_part);
    let stamp = check_token(&found.contents_trimmed(), &data, stored, setup);
    report.signer = stamp.authority.clone();
    report.issuer = String::new();
    report.signed_at = Some(stamp.time.clone());
    report.signature = stamp.check.clone();
    report.identity = match stamp.check.status {
        Status::Pass => Check::pass("The time stamp server is trusted."),
        Status::Fail => stamp.check.clone(),
        Status::Unknown => Check::unknown("The time stamp server is not trusted."),
    };
    report.revocation = Check::unknown("Not checked for time stamps.");
    report.algorithm = "Time stamp (RFC 3161)".to_owned();
}

impl Found {
    /// The signature value without the zero fill after it.
    fn contents_trimmed(&self) -> Vec<u8> {
        cmsdoc::trim_padding(&self.contents).map_or_else(|_| self.contents.clone(), <[u8]>::to_vec)
    }
}

#[cfg(test)]
mod tests {
    // The checks are exercised end to end in `engine::tests`, where files are signed first.
}
