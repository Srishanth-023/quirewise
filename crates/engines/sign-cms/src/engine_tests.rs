//! The engine end to end: files are signed, and the signatures are checked and attacked.

use std::path::{Path, PathBuf};

use application::runtime::CancellationToken;
use application::signing::{
    KeyChoice, PdfSigner, SignRequest, SignatureBox, SignatureChecker, SignatureKind,
    SignatureReport, Status, VerifyRequest, WebClient,
};
use domain::ErrorCode;
use domain::secret::Secret;
use domain::tools::signing::{Certify, SignLevel};
use lopdf::dictionary;

use crate::CmsEngine;
use crate::testkit::{LeafKind, Pki, Spec, now};
use crate::testweb::{MockWeb, TSA_URL, Tsa};

fn t() -> i64 {
    i64::try_from(now()).unwrap()
}

struct Lab {
    dir: tempfile::TempDir,
}

impl Lab {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn pdf(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.path(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn pfx(&self, pki: &Pki) -> PathBuf {
        self.pdf("me.pfx", &pki.pfx("pw"))
    }

    fn trust(&self, roots: &[&[u8]]) -> PathBuf {
        use der::{Decode, EncodePem};
        let mut pem = String::new();
        for der in roots {
            pem.push_str(
                &x509_cert::Certificate::from_der(der)
                    .unwrap()
                    .to_pem(der::pem::LineEnding::LF)
                    .unwrap(),
            );
        }
        let p = self.path("trust.pem");
        std::fs::write(&p, pem).unwrap();
        p
    }
}

fn request(pfx: &Path) -> SignRequest {
    SignRequest {
        key: KeyChoice::Pfx {
            path: pfx.to_path_buf(),
            password: Secret::new("pw"),
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
        signed_at: t(),
    }
}

fn sign(
    lab: &Lab,
    input: &Path,
    out: &str,
    req: &SignRequest,
    web: Option<&dyn WebClient>,
) -> Result<PathBuf, application::AppError> {
    let dest = lab.path(out);
    CmsEngine::isolated().sign(input, &dest, req, web, &CancellationToken::new())?;
    Ok(dest)
}

fn check(
    file: &Path,
    trust: Option<&Path>,
    revocation: bool,
    web: Option<&dyn WebClient>,
) -> Vec<SignatureReport> {
    CmsEngine::isolated()
        .check(
            file,
            &VerifyRequest {
                check_revocation: revocation,
                trusted_certificates: trust.map(Path::to_path_buf),
                now: t(),
            },
            web,
            &CancellationToken::new(),
        )
        .unwrap()
}

fn plain(lab: &Lab) -> PathBuf {
    lab.pdf(
        "in.pdf",
        &test_support::pdfs::pdf_with_texts(&["one", "two"]),
    )
}

#[test]
fn a_signed_file_verifies_and_is_trusted_when_its_root_is() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let input = plain(&lab);
    let before = std::fs::read(&input).unwrap();
    let out = sign(&lab, &input, "out.pdf", &request(&lab.pfx(&pki)), None).unwrap();
    assert_eq!(
        std::fs::read(&input).unwrap(),
        before,
        "the input is never touched"
    );
    let signed = std::fs::read(&out).unwrap();
    assert_eq!(
        &signed[..before.len()],
        &before[..],
        "the original bytes come first"
    );

    let trust = lab.trust(&[&pki.root_der]);
    let reports = check(&out, Some(&trust), false, None);
    assert_eq!(reports.len(), 1);
    let r = &reports[0];
    assert_eq!(r.signer, "Test Signer");
    assert_eq!(r.issuer, "Test Root");
    assert_eq!(r.field, "Signature1");
    assert_eq!(r.signature.status, Status::Pass, "{r:?}");
    assert_eq!(r.changes.status, Status::Pass, "{r:?}");
    assert_eq!(r.identity.status, Status::Pass, "{r:?}");
    assert_eq!(r.revocation.status, Status::Unknown);
    assert!(
        r.algorithm.contains("ECDSA P-256") && r.algorithm.contains("SHA-256"),
        "{}",
        r.algorithm
    );
    assert!(r.timestamp.is_none());
    assert!(!r.has_failure());
    assert!(!r.is_trusted() || r.revocation.status != Status::Fail);
}

#[test]
fn without_the_root_the_identity_is_unknown_but_the_signature_is_valid() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let out = sign(
        &lab,
        &plain(&lab),
        "out.pdf",
        &request(&lab.pfx(&pki)),
        None,
    )
    .unwrap();
    let r = &check(&out, None, false, None)[0];
    assert_eq!(r.signature.status, Status::Pass);
    assert_eq!(r.identity.status, Status::Unknown, "{r:?}");
    assert!(!r.is_trusted());
}

#[test]
fn every_kind_of_key_signs_files_that_verify() {
    for kind in [LeafKind::P256, LeafKind::P384, LeafKind::Rsa] {
        let lab = Lab::new();
        let pki = Pki::build(&Spec {
            kind,
            ..Spec::default()
        });
        let out = sign(
            &lab,
            &plain(&lab),
            "out.pdf",
            &request(&lab.pfx(&pki)),
            None,
        )
        .unwrap();
        let trust = lab.trust(&[&pki.root_der]);
        let r = &check(&out, Some(&trust), false, None)[0];
        assert_eq!(r.signature.status, Status::Pass, "{kind:?}: {r:?}");
        assert_eq!(r.identity.status, Status::Pass, "{kind:?}");
    }
}

#[test]
fn changing_the_signed_content_breaks_the_signature() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let out = sign(
        &lab,
        &plain(&lab),
        "out.pdf",
        &request(&lab.pfx(&pki)),
        None,
    )
    .unwrap();
    let mut bytes = std::fs::read(&out).unwrap();
    let at = bytes
        .windows(3)
        .position(|w| w == b"one")
        .expect("page text");
    bytes[at] = b'0';
    let tampered = lab.pdf("tampered.pdf", &bytes);
    let r = &check(&tampered, None, false, None)[0];
    assert_eq!(r.signature.status, Status::Fail, "{r:?}");
    assert!(
        r.signature.detail.contains("does not match"),
        "{}",
        r.signature.detail
    );
    assert!(r.has_failure());
}

#[test]
fn a_flipped_bit_in_the_signature_value_is_caught() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let out = sign(
        &lab,
        &plain(&lab),
        "out.pdf",
        &request(&lab.pfx(&pki)),
        None,
    )
    .unwrap();
    let mut bytes = std::fs::read(&out).unwrap();
    let open = bytes.windows(11).position(|w| w == b"/Contents <").unwrap() + 11;
    // The hex of the signature starts with the CMS header; change a digit well inside it.
    bytes[open + 600] = if bytes[open + 600] == b'0' {
        b'1'
    } else {
        b'0'
    };
    let tampered = lab.pdf("tampered.pdf", &bytes);
    let r = &check(&tampered, None, false, None)[0];
    assert_eq!(r.signature.status, Status::Fail, "{r:?}");
}

#[test]
fn content_added_after_signing_is_reported() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let out = sign(
        &lab,
        &plain(&lab),
        "out.pdf",
        &request(&lab.pfx(&pki)),
        None,
    )
    .unwrap();
    let signed = std::fs::read(&out).unwrap();
    let mut rev = crate::pdfrev::Revision::open(&signed).unwrap();
    rev.add(lopdf::Object::Stream(lopdf::Stream::new(
        lopdf::Dictionary::new(),
        b"BT /F1 12 Tf 10 10 Td (added) Tj ET".to_vec(),
    )));
    let edited = lab.pdf("edited.pdf", &rev.finish().unwrap().bytes);
    let r = &check(&edited, None, false, None)[0];
    assert_eq!(
        r.signature.status,
        Status::Pass,
        "the signed bytes are still intact"
    );
    assert_eq!(r.changes.status, Status::Fail, "{r:?}");
}

#[test]
fn a_second_signature_leaves_the_first_valid() {
    let lab = Lab::new();
    let one = Pki::ec();
    let two = Pki::ec();
    let first = sign(
        &lab,
        &plain(&lab),
        "one.pdf",
        &request(&lab.pfx(&one)),
        None,
    )
    .unwrap();
    let pfx2 = lab.pdf("two.pfx", &two.pfx("pw"));
    let second = sign(&lab, &first, "two.pdf", &request(&pfx2), None).unwrap();
    let trust = lab.trust(&[&one.root_der, &two.root_der]);
    let reports = check(&second, Some(&trust), false, None);
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].field, "Signature1");
    assert_eq!(reports[1].field, "Signature2");
    for r in &reports {
        assert_eq!(r.signature.status, Status::Pass, "{r:?}");
        assert_eq!(r.identity.status, Status::Pass, "{r:?}");
    }
    assert_eq!(
        reports[0].changes.status,
        Status::Pass,
        "only a signature was added: {:?}",
        reports[0].changes
    );
    assert_eq!(reports[1].changes.status, Status::Pass);
}

#[test]
fn a_visible_signature_carries_the_details_the_signer_gave() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let mut req = request(&lab.pfx(&pki));
    req.visible = Some(SignatureBox {
        page: 0,
        left: 0.5,
        top: 0.85,
        width: 0.4,
        height: 0.1,
    });
    req.reason = Some("I approve".into());
    req.location = Some("Chennai".into());
    req.contact = Some("ada@example.test".into());
    req.signer_name = Some("Ada Lovelace".into());
    let out = sign(&lab, &plain(&lab), "out.pdf", &req, None).unwrap();
    let r = &check(&out, None, false, None)[0];
    assert_eq!(r.reason.as_deref(), Some("I approve"));
    assert_eq!(r.location.as_deref(), Some("Chennai"));
    assert_eq!(r.contact.as_deref(), Some("ada@example.test"));
    let doc = lopdf::Document::load_mem(&std::fs::read(&out).unwrap()).unwrap();
    let page1 = doc.get_pages()[&1];
    assert_eq!(doc.get_page_annotations(page1).unwrap().len(), 1);
}

#[test]
fn certifying_records_the_permission() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let mut req = request(&lab.pfx(&pki));
    req.certify = Certify::FormFill;
    let out = sign(&lab, &plain(&lab), "out.pdf", &req, None).unwrap();
    let r = &check(&out, None, false, None)[0];
    assert_eq!(r.kind, SignatureKind::Certification(2));
    assert_eq!(r.signature.status, Status::Pass);
}

#[test]
fn a_wrong_password_and_an_expired_certificate_are_refused_before_anything_is_written() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let mut req = request(&lab.pfx(&pki));
    req.key = KeyChoice::Pfx {
        path: lab.path("me.pfx"),
        password: Secret::new("wrong"),
    };
    let err = sign(&lab, &plain(&lab), "out.pdf", &req, None).unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(!lab.path("out.pdf").exists());

    let old = Pki::build(&Spec {
        expired: true,
        ..Spec::default()
    });
    let err = sign(
        &lab,
        &plain(&lab),
        "out2.pdf",
        &request(&lab.pdf("old.pfx", &old.pfx("pw"))),
        None,
    )
    .unwrap_err();
    assert!(err.message.contains("not valid today"), "{}", err.message);
}

#[test]
fn a_protected_file_is_refused_with_advice() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let plain_pdf = plain(&lab);
    let locked = lab.path("locked.pdf");
    test_support::encrypt_pdf(&plain_pdf, &locked, "pw");
    let err = sign(&lab, &locked, "out.pdf", &request(&lab.pfx(&pki)), None).unwrap_err();
    assert_eq!(err.code, ErrorCode::PdfEncrypted);
}

#[test]
fn a_time_stamped_signature_proves_its_time() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let tsa = Tsa::new();
    let web = tsa.client();
    let mut req = request(&lab.pfx(&pki));
    req.level = SignLevel::BT;
    req.timestamp_url = Some(TSA_URL.into());
    let out = sign(&lab, &plain(&lab), "out.pdf", &req, Some(&web)).unwrap();
    assert_eq!(web.asked(), vec![TSA_URL]);

    let both = lab.trust(&[&pki.root_der, &tsa.root_der()]);
    let r = &check(&out, Some(&both), false, None)[0];
    let stamp = r.timestamp.as_ref().expect("a time stamp");
    assert_eq!(stamp.check.status, Status::Pass, "{stamp:?}");
    assert_eq!(stamp.authority, "Test TSA");
    assert_eq!(r.signature.status, Status::Pass);

    // A server that is not trusted gives a stamp that is intact but proves nothing.
    let only_signer = lab.trust(&[&pki.root_der]);
    let r = &check(&out, Some(&only_signer), false, None)[0];
    assert_eq!(r.timestamp.as_ref().unwrap().check.status, Status::Unknown);
}

#[test]
fn a_server_certificate_that_is_not_for_time_stamps_proves_nothing() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let tsa = Tsa::unmarked();
    let web = tsa.client();
    let mut req = request(&lab.pfx(&pki));
    req.level = SignLevel::BT;
    req.timestamp_url = Some(TSA_URL.into());
    let out = sign(&lab, &plain(&lab), "out.pdf", &req, Some(&web)).unwrap();
    let trust = lab.trust(&[&pki.root_der, &tsa.root_der()]);
    let r = &check(&out, Some(&trust), false, None)[0];
    let stamp = r.timestamp.as_ref().expect("a time stamp");
    assert_eq!(stamp.check.status, Status::Unknown, "{stamp:?}");
    assert!(
        stamp.check.detail.contains("not marked for time stamping"),
        "{}",
        stamp.check.detail
    );
    assert_eq!(r.signature.status, Status::Pass);
}

#[test]
fn a_time_stamped_signature_stays_valid_after_the_certificate_expires() {
    // The certificate is valid when the signature is made; the check runs far in the future.
    let lab = Lab::new();
    let pki = Pki::ec();
    let tsa = Tsa::new();
    let web = tsa.client();
    let mut req = request(&lab.pfx(&pki));
    req.level = SignLevel::BT;
    req.timestamp_url = Some(TSA_URL.into());
    let out = sign(&lab, &plain(&lab), "out.pdf", &req, Some(&web)).unwrap();
    let trust = lab.trust(&[&pki.root_der, &tsa.root_der()]);
    let far = t() + 30 * 365 * 86_400;
    let reports = CmsEngine::isolated()
        .check(
            &out,
            &VerifyRequest {
                check_revocation: false,
                trusted_certificates: Some(trust),
                now: far,
            },
            None,
            &CancellationToken::new(),
        )
        .unwrap();
    // The time stamp fixes the time of signing, so the later expiry does not matter.
    assert_eq!(
        reports[0].identity.status,
        Status::Pass,
        "{:?}",
        reports[0].identity
    );
}

#[test]
fn a_signature_without_a_stamp_is_unknown_once_the_certificate_expires() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let out = sign(
        &lab,
        &plain(&lab),
        "out.pdf",
        &request(&lab.pfx(&pki)),
        None,
    )
    .unwrap();
    let trust = lab.trust(&[&pki.root_der]);
    let far = t() + 30 * 365 * 86_400;
    let reports = CmsEngine::isolated()
        .check(
            &out,
            &VerifyRequest {
                check_revocation: false,
                trusted_certificates: Some(trust),
                now: far,
            },
            None,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(
        reports[0].identity.status,
        Status::Unknown,
        "{:?}",
        reports[0].identity
    );
    assert!(reports[0].identity.detail.contains("expired"));
}

#[test]
fn levels_that_use_the_internet_need_the_connection() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let mut req = request(&lab.pfx(&pki));
    req.level = SignLevel::BT;
    req.timestamp_url = Some(TSA_URL.into());
    let err = sign(&lab, &plain(&lab), "out.pdf", &req, None).unwrap_err();
    assert_eq!(err.code, ErrorCode::NetworkConsentRequired);
    assert!(!lab.path("out.pdf").exists());
}

#[test]
fn a_server_that_cannot_be_reached_fails_the_signature_cleanly() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let mut req = request(&lab.pfx(&pki));
    req.level = SignLevel::BT;
    req.timestamp_url = Some("http://nowhere.test/ts".into());
    let web = MockWeb::new();
    let err = sign(&lab, &plain(&lab), "out.pdf", &req, Some(&web)).unwrap_err();
    assert_eq!(err.code, ErrorCode::NetworkUnavailable);
    assert!(!lab.path("out.pdf").exists());
}

fn long_term_web(pki: &Pki, tsa: &Tsa, revoked: bool) -> MockWeb {
    let leaf = pki.leaf_der.clone();
    let answer = pki.ocsp(
        &leaf,
        revoked.then(|| t() - 100),
        t() - 10,
        Some(t() + 86_400),
    );
    tsa.route_into(MockWeb::new().route("http://ocsp.test/ocsp", move |_| Ok(answer.clone())))
}

#[test]
fn a_long_term_signature_can_be_checked_offline() {
    let lab = Lab::new();
    let pki = Pki::ec_with_addresses();
    let tsa = Tsa::new();
    let web = long_term_web(&pki, &tsa, false);
    let mut req = request(&lab.pfx(&pki));
    req.level = SignLevel::BLt;
    req.timestamp_url = Some(TSA_URL.into());
    let out = sign(&lab, &plain(&lab), "out.pdf", &req, Some(&web)).unwrap();

    // Checked without any connection: the answers are in the file.
    let trust = lab.trust(&[&pki.root_der, &tsa.root_der()]);
    let r = &check(&out, Some(&trust), false, None)[0];
    assert!(r.long_term, "the file holds revocation answers");
    assert_eq!(r.revocation.status, Status::Pass, "{:?}", r.revocation);
    assert_eq!(r.signature.status, Status::Pass);
    assert_eq!(r.changes.status, Status::Pass, "{:?}", r.changes);
    assert_eq!(r.timestamp.as_ref().unwrap().check.status, Status::Pass);
}

#[test]
fn a_revoked_certificate_is_not_used_for_a_long_term_signature() {
    let lab = Lab::new();
    let pki = Pki::ec_with_addresses();
    let tsa = Tsa::new();
    let web = long_term_web(&pki, &tsa, true);
    let mut req = request(&lab.pfx(&pki));
    req.level = SignLevel::BLt;
    req.timestamp_url = Some(TSA_URL.into());
    let err = sign(&lab, &plain(&lab), "out.pdf", &req, Some(&web)).unwrap_err();
    assert!(err.message.contains("revoked"), "{}", err.message);
    assert!(!lab.path("out.pdf").exists());
}

#[test]
fn an_archival_signature_ends_with_a_time_stamp_over_the_file() {
    let lab = Lab::new();
    let pki = Pki::ec_with_addresses();
    let tsa = Tsa::new();
    let web = long_term_web(&pki, &tsa, false);
    let mut req = request(&lab.pfx(&pki));
    req.level = SignLevel::BLta;
    req.timestamp_url = Some(TSA_URL.into());
    let out = sign(&lab, &plain(&lab), "out.pdf", &req, Some(&web)).unwrap();
    let trust = lab.trust(&[&pki.root_der, &tsa.root_der()]);
    let reports = check(&out, Some(&trust), false, None);
    assert_eq!(
        reports.len(),
        2,
        "the signature and the time stamp over the file"
    );
    assert_eq!(reports[0].kind, SignatureKind::Approval);
    assert_eq!(reports[1].kind, SignatureKind::DocumentTimestamp);
    assert_eq!(
        reports[1].signature.status,
        Status::Pass,
        "{:?}",
        reports[1]
    );
    assert_eq!(
        reports[0].changes.status,
        Status::Pass,
        "the later revisions only add validation data"
    );

    // Changing the file after the time stamp is caught by the stamp.
    let mut bytes = std::fs::read(&out).unwrap();
    let at = bytes.windows(3).position(|w| w == b"one").unwrap();
    bytes[at] = b'0';
    let tampered = lab.pdf("t.pdf", &bytes);
    let after = check(&tampered, Some(&trust), false, None);
    assert!(
        after.iter().all(|r| r.signature.status == Status::Fail),
        "{after:?}"
    );
}

#[test]
fn a_live_revocation_check_finds_a_revoked_certificate() {
    let lab = Lab::new();
    let pki = Pki::ec_with_addresses();
    let out = sign(
        &lab,
        &plain(&lab),
        "out.pdf",
        &request(&lab.pfx(&pki)),
        None,
    )
    .unwrap();
    let trust = lab.trust(&[&pki.root_der]);

    let leaf = pki.leaf_der.clone();
    let good = pki.ocsp(&leaf, None, t() - 10, Some(t() + 3600));
    let web = MockWeb::new().route("http://ocsp.test/ocsp", move |_| Ok(good.clone()));
    let r = &check(&out, Some(&trust), true, Some(&web))[0];
    assert_eq!(r.revocation.status, Status::Pass, "{:?}", r.revocation);

    let revoked = pki.ocsp(&leaf, Some(t() - 5000), t() - 10, Some(t() + 3600));
    let web = MockWeb::new().route("http://ocsp.test/ocsp", move |_| Ok(revoked.clone()));
    let r = &check(&out, Some(&trust), true, Some(&web))[0];
    // The signer's claimed time is just now, which is after the revocation.
    assert_eq!(r.revocation.status, Status::Fail, "{:?}", r.revocation);
    assert!(r.has_failure());
}

#[test]
fn an_unreachable_revocation_server_leaves_the_answer_unknown() {
    let lab = Lab::new();
    let pki = Pki::ec_with_addresses();
    let out = sign(
        &lab,
        &plain(&lab),
        "out.pdf",
        &request(&lab.pfx(&pki)),
        None,
    )
    .unwrap();
    let trust = lab.trust(&[&pki.root_der]);
    let web = MockWeb::new();
    let r = &check(&out, Some(&trust), true, Some(&web))[0];
    assert_eq!(r.revocation.status, Status::Unknown, "{:?}", r.revocation);
}

#[test]
fn a_file_without_signatures_gives_no_reports() {
    let lab = Lab::new();
    assert!(check(&plain(&lab), None, false, None).is_empty());
}

#[test]
fn a_file_that_is_not_a_pdf_is_a_clear_error() {
    let lab = Lab::new();
    let junk = lab.pdf("junk.pdf", b"not a pdf at all");
    let err = CmsEngine::isolated()
        .check(
            &junk,
            &VerifyRequest {
                check_revocation: false,
                trusted_certificates: None,
                now: t(),
            },
            None,
            &CancellationToken::new(),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PdfDamaged);
}

#[test]
fn a_signature_can_fill_an_empty_field_that_was_prepared() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let base = test_support::pdfs::pdf_with_texts(&["x"]);
    let mut rev = crate::pdfrev::Revision::open(&base).unwrap();
    let page = crate::acro::pages(rev.doc())[0];
    let widget = rev.add(lopdf::Object::Dictionary(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Sig",
        "T" => crate::pdfsign::pdf_text("Approve"), "F" => 4,
        "Rect" => vec![100.into(), 100.into(), 300.into(), 160.into()],
        "P" => lopdf::Object::Reference(page),
    }));
    let mut page_dict = rev.dict(&lopdf::Object::Reference(page)).unwrap().clone();
    page_dict.set(
        "Annots",
        lopdf::Object::Array(vec![lopdf::Object::Reference(widget)]),
    );
    rev.replace(page, lopdf::Object::Dictionary(page_dict));
    let root = rev.root().unwrap();
    let mut catalog = rev.dict(&lopdf::Object::Reference(root)).unwrap().clone();
    catalog.set(
        "AcroForm",
        lopdf::Object::Dictionary(
            dictionary! { "Fields" => vec![lopdf::Object::Reference(widget)] },
        ),
    );
    rev.replace(root, lopdf::Object::Dictionary(catalog));
    let prepared = lab.pdf("form.pdf", &rev.finish().unwrap().bytes);

    let mut req = request(&lab.pfx(&pki));
    req.field = Some("Approve".into());
    let out = sign(&lab, &prepared, "out.pdf", &req, None).unwrap();
    let r = &check(&out, None, false, None)[0];
    assert_eq!(r.field, "Approve");
    assert_eq!(r.signature.status, Status::Pass, "{r:?}");
}

/// Runs qpdf on a file, for making fixtures and for checking results.
fn qpdf(args: &[&str]) -> std::process::ExitStatus {
    std::process::Command::new(test_support::qpdf_exe().expect("run scripts/fetch/fetch-qpdf.ps1 first"))
        .args(args)
        .status()
        .expect("qpdf starts")
}

#[test]
fn a_file_with_a_cross_reference_stream_is_signed_and_stays_valid() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let source = plain(&lab);
    let packed = lab.path("packed.pdf");
    let made = qpdf(&[
        "--object-streams=generate",
        "--compress-streams=y",
        source.to_str().unwrap(),
        packed.to_str().unwrap(),
    ]);
    assert!(made.success());
    let before = std::fs::read(&packed).unwrap();
    assert!(
        before.windows(5).any(|w| w == b"/XRef"),
        "the fixture uses a cross-reference stream"
    );

    let out = sign(&lab, &packed, "out.pdf", &request(&lab.pfx(&pki)), None).unwrap();
    let signed = std::fs::read(&out).unwrap();
    assert_eq!(&signed[..before.len()], &before[..]);
    let trust = lab.trust(&[&pki.root_der]);
    let r = &check(&out, Some(&trust), false, None)[0];
    assert_eq!(r.signature.status, Status::Pass, "{r:?}");
    assert_eq!(r.changes.status, Status::Pass, "{r:?}");
    // A second signature on top of a file that uses object streams works too.
    let again = sign(&lab, &out, "again.pdf", &request(&lab.pfx(&pki)), None).unwrap();
    let reports = check(&again, Some(&trust), false, None);
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|r| r.signature.status == Status::Pass));
    // qpdf accepts the structure of what was written.
    assert!(qpdf(&["--check", again.to_str().unwrap()]).success());
}

#[test]
fn a_linearized_file_is_signed_and_still_passes_qpdf() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let source = plain(&lab);
    let linear = lab.path("linear.pdf");
    assert!(
        qpdf(&[
            "--linearize",
            source.to_str().unwrap(),
            linear.to_str().unwrap()
        ])
        .success()
    );
    let out = sign(&lab, &linear, "out.pdf", &request(&lab.pfx(&pki)), None).unwrap();
    assert_eq!(
        check(&out, None, false, None)[0].signature.status,
        Status::Pass
    );
    assert!(qpdf(&["--check", out.to_str().unwrap()]).success());
}

#[test]
fn a_file_with_a_form_keeps_its_fields_when_signed() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let input = lab.pdf(
        "form.pdf",
        &test_support::pdfs::pdf_with_form_field("name", "Ada"),
    );
    let out = sign(&lab, &input, "out.pdf", &request(&lab.pfx(&pki)), None).unwrap();
    let doc = lopdf::Document::load_mem(&std::fs::read(&out).unwrap()).unwrap();
    let names: Vec<String> = crate::acro::fields(&doc)
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert!(names.contains(&"name".to_owned()), "{names:?}");
    assert!(names.contains(&"Signature1".to_owned()), "{names:?}");
    assert_eq!(
        check(&out, None, false, None)[0].signature.status,
        Status::Pass
    );
    assert!(qpdf(&["--check", out.to_str().unwrap()]).success());
}
