//! The report Verify signatures writes: what the checks found for every signature, in words a
//! person can act on. Laid out with the shared report kit.

use super::reportkit::{Builder, Face, INK, MARGIN, MUTED, PAGE, Report, wrap};
use crate::signing::{Check, SignatureKind, SignatureReport, Status};

const OK_TEXT: &str = "#1B5E20";
const BAD_TEXT: &str = "#9A1B1B";
/// Characters that fit on a line at the detail size, with room to spare.
const DETAIL_CHARS: usize = 100;
const LABEL_WIDTH: f32 = 120.0;

/// What the report is about.
pub struct Subject<'a> {
    /// Name of the file that was checked.
    pub file_name: &'a str,
    /// When the check ran, as text.
    pub checked: &'a str,
    /// Whether the certificate authorities were asked about revocation.
    pub asked_revocation: bool,
}

/// How a signature is summed up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Valid, intact and from a trusted identity.
    Trusted,
    /// Valid, but something could not be confirmed.
    Open,
    /// Something failed.
    Failed,
}

/// The verdict of one signature.
pub fn verdict(report: &SignatureReport) -> Verdict {
    if report.has_failure() {
        Verdict::Failed
    } else if report.is_trusted() {
        Verdict::Trusted
    } else {
        Verdict::Open
    }
}

fn verdict_text(v: Verdict) -> (&'static str, &'static str) {
    match v {
        Verdict::Trusted => ("Valid and trusted", OK_TEXT),
        Verdict::Open => ("Valid, but not everything could be confirmed", INK),
        Verdict::Failed => ("Not valid", BAD_TEXT),
    }
}

fn kind_text(kind: SignatureKind) -> String {
    match kind {
        SignatureKind::Approval => "Approval signature".to_owned(),
        SignatureKind::Certification(1) => "Certifying signature: no changes allowed".to_owned(),
        SignatureKind::Certification(2) => {
            "Certifying signature: filling in forms allowed".to_owned()
        }
        SignatureKind::Certification(_) => {
            "Certifying signature: forms and comments allowed".to_owned()
        }
        SignatureKind::DocumentTimestamp => "Time stamp over the document".to_owned(),
    }
}

fn status_word(status: Status) -> &'static str {
    match status {
        Status::Pass => "Passed",
        Status::Fail => "Failed",
        Status::Unknown => "Open",
    }
}

fn status_color(status: Status) -> &'static str {
    match status {
        Status::Pass => OK_TEXT,
        Status::Fail => BAD_TEXT,
        Status::Unknown => INK,
    }
}

const DETAIL: f32 = 9.0;
const LEADING: f32 = 11.5;

fn ensure_room(b: &mut Builder, height: f32) {
    if !b.room(height) {
        b.new_page();
    }
}

fn fact(b: &mut Builder, label: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    let lines = wrap(value, DETAIL_CHARS - 20);
    ensure_room(b, LEADING * lines.len() as f32);
    let y = b.y;
    b.text(
        (MARGIN, LABEL_WIDTH, y),
        label,
        Face {
            size: DETAIL,
            bold: false,
            color: MUTED,
        },
    );
    for (i, line) in lines.iter().enumerate() {
        b.text(
            (
                MARGIN + LABEL_WIDTH,
                PAGE.0 - 2.0 * MARGIN - LABEL_WIDTH,
                y + i as f32 * LEADING,
            ),
            line,
            Face {
                size: DETAIL,
                bold: false,
                color: INK,
            },
        );
    }
    b.y += lines.len() as f32 * LEADING;
}

fn check_row(b: &mut Builder, label: &str, check: &Check) {
    let text = format!("{}. {}", status_word(check.status), check.detail);
    let lines = wrap(&text, DETAIL_CHARS - 20);
    ensure_room(b, LEADING * lines.len() as f32);
    let y = b.y;
    b.text(
        (MARGIN, LABEL_WIDTH, y),
        label,
        Face {
            size: DETAIL,
            bold: true,
            color: INK,
        },
    );
    for (i, line) in lines.iter().enumerate() {
        // The first line starts with the answer, which carries the color.
        let color = if i == 0 {
            status_color(check.status)
        } else {
            INK
        };
        b.text(
            (
                MARGIN + LABEL_WIDTH,
                PAGE.0 - 2.0 * MARGIN - LABEL_WIDTH,
                y + i as f32 * LEADING,
            ),
            line,
            Face {
                size: DETAIL,
                bold: false,
                color,
            },
        );
    }
    b.y += lines.len() as f32 * LEADING + 1.0;
}

/// Lays out the report.
pub fn layout(subject: &Subject<'_>, reports: &[SignatureReport]) -> Report {
    let mut b = Builder::new();
    b.line("Signature report", 20.0, true, INK);
    b.gap(6.0);
    b.line(&format!("File: {}", subject.file_name), 10.0, false, INK);
    b.line(&format!("Checked: {}", subject.checked), 10.0, false, INK);
    let revocation = if subject.asked_revocation {
        "Revocation: the certificate authorities were asked."
    } else {
        "Revocation: not asked. Only answers stored in the file were used."
    };
    b.line(revocation, 10.0, false, INK);
    b.gap(6.0);
    if reports.is_empty() {
        b.line("This file has no digital signatures.", 11.0, true, INK);
    } else {
        let count = |v: Verdict| reports.iter().filter(|r| verdict(r) == v).count();
        b.line(
            &format!(
                "{} signatures: {} valid and trusted, {} with open questions, {} not valid.",
                reports.len(),
                count(Verdict::Trusted),
                count(Verdict::Open),
                count(Verdict::Failed)
            ),
            11.0,
            true,
            INK,
        );
    }
    for (i, r) in reports.iter().enumerate() {
        b.gap(12.0);
        ensure_room(&mut b, 120.0);
        b.line(
            &format!(
                "{}. {}",
                i + 1,
                if r.signer.is_empty() {
                    "Unknown signer"
                } else {
                    &r.signer
                }
            ),
            12.0,
            true,
            INK,
        );
        let (text, color) = verdict_text(verdict(r));
        b.line(text, 10.0, true, color);
        b.gap(2.0);
        fact(&mut b, "Kind", &kind_text(r.kind));
        fact(&mut b, "Field", &r.field);
        fact(&mut b, "Signed", r.signed_at.as_deref().unwrap_or(""));
        fact(&mut b, "Reason", r.reason.as_deref().unwrap_or(""));
        fact(&mut b, "Location", r.location.as_deref().unwrap_or(""));
        fact(&mut b, "Contact", r.contact.as_deref().unwrap_or(""));
        fact(&mut b, "Issued by", &r.issuer);
        fact(&mut b, "Algorithm", &r.algorithm);
        if r.long_term {
            fact(
                &mut b,
                "Long term",
                "The file stores revocation answers for later checks.",
            );
        }
        b.gap(3.0);
        check_row(&mut b, "Signature", &r.signature);
        check_row(&mut b, "Later changes", &r.changes);
        check_row(&mut b, "Signer identity", &r.identity);
        check_row(&mut b, "Revocation", &r.revocation);
        if let Some(t) = &r.timestamp {
            let detail = Check {
                status: t.check.status,
                detail: format!("{} by {}. {}", t.time, t.authority, t.check.detail),
            };
            check_row(&mut b, "Time stamp", &detail);
        }
        for note in &r.notes {
            ensure_room(&mut b, LEADING);
            b.line(&format!("Note: {note}"), DETAIL, false, MUTED);
        }
    }
    b.gap(14.0);
    ensure_room(&mut b, 3.0 * LEADING);
    for line in wrap(
        "A valid signature shows that the signed part of the file is unchanged and who holds the key. It does not show that the content is right, or that you know the signer. Windows decides which authorities are trusted.",
        DETAIL_CHARS + 10,
    ) {
        b.line(&line, 8.0, false, MUTED);
    }
    Report {
        pages: b.page,
        items: b.items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::TimestampReport;

    fn report(signer: &str) -> SignatureReport {
        SignatureReport {
            field: "Signature1".into(),
            kind: SignatureKind::Approval,
            signer: signer.into(),
            issuer: "Test Root".into(),
            signed_at: Some("2027-01-15 08:00:00 UTC".into()),
            reason: Some("I approve".into()),
            location: None,
            contact: None,
            algorithm: "ECDSA P-256 with SHA-256".into(),
            signature: Check::pass("The signature is valid."),
            changes: Check::pass("The file is exactly as signed."),
            identity: Check::pass("Issued to a trusted authority."),
            revocation: Check::unknown("Not checked."),
            timestamp: None,
            long_term: false,
            notes: vec!["No trusted time stamp.".into()],
        }
    }

    fn texts(report: &Report) -> String {
        report
            .items
            .iter()
            .filter_map(|i| match i {
                domain::tools::editor::EditItem::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn subject() -> Subject<'static> {
        Subject {
            file_name: "contract.pdf",
            checked: "2027-01-16 09:00",
            asked_revocation: false,
        }
    }

    #[test]
    fn the_verdict_follows_the_checks() {
        let mut r = report("A");
        assert_eq!(verdict(&r), Verdict::Trusted);
        r.identity = Check::unknown("self-signed");
        assert_eq!(verdict(&r), Verdict::Open);
        r.signature = Check::fail("changed");
        assert_eq!(verdict(&r), Verdict::Failed);
    }

    #[test]
    fn a_file_without_signatures_says_so() {
        let out = layout(&subject(), &[]);
        assert_eq!(out.pages, 1);
        assert!(texts(&out).contains("no digital signatures"));
    }

    #[test]
    fn every_signature_gets_its_facts_and_checks() {
        let mut second = report("Grace");
        second.timestamp = Some(TimestampReport {
            time: "2027-01-15 08:00:01 UTC".into(),
            authority: "Test TSA".into(),
            check: Check::pass("Valid."),
        });
        second.signature = Check::fail("The file was changed.");
        let out = layout(&subject(), &[report("Ada"), second]);
        let text = texts(&out);
        for expected in [
            "Signature report",
            "contract.pdf",
            "2 signatures: 1 valid and trusted, 0 with open questions, 1 not valid.",
            "1. Ada",
            "2. Grace",
            "Valid and trusted",
            "Not valid",
            "I approve",
            "Test Root",
            "Passed. The signature is valid.",
            "Failed. The file was changed.",
            "Open. Not checked.",
            "Test TSA",
            "Note: No trusted time stamp.",
            "Revocation: not asked",
        ] {
            assert!(text.contains(expected), "{expected:?} missing:\n{text}");
        }
    }

    #[test]
    fn many_signatures_flow_onto_more_pages() {
        let many: Vec<SignatureReport> = (0..40).map(|i| report(&format!("Signer {i}"))).collect();
        let out = layout(&subject(), &many);
        assert!(out.pages > 2, "{}", out.pages);
        let max_page = out
            .items
            .iter()
            .map(|i| match i {
                domain::tools::editor::EditItem::Text(t) => t.page,
                domain::tools::editor::EditItem::Whiteout(w) => w.page,
                _ => 1,
            })
            .max()
            .unwrap();
        assert_eq!(max_page, out.pages);
    }

    #[test]
    fn certifications_and_time_stamps_are_named() {
        assert!(kind_text(SignatureKind::Certification(1)).contains("no changes"));
        assert!(kind_text(SignatureKind::Certification(3)).contains("comments"));
        assert!(kind_text(SignatureKind::DocumentTimestamp).contains("Time stamp"));
    }
}
