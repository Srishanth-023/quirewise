//! End-to-end tests for the digital signature tools: the real RunTool, the worker engines, the
//! file store, and for the long-term levels a real HTTP server on the loopback address.
// Test helpers outside `#[test]` functions may unwrap: a failure should stop the test at once.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::fs;
use std::path::PathBuf;

use application::{InputDoc, Password};
use common::{Lab, name};
use domain::secret::Secret;
use domain::tools::signing::{Certify, DigitalSignOptions, SignLevel, VerifySignaturesOptions};
use domain::{ErrorCode, ToolOptions};
use quirewise_engine_sign_cms::testkit::{Pki, Spec};
use quirewise_engine_sign_cms::testweb::{TestServer, Tsa};
use test_support::{encrypt_pdf, pdfs};

fn pfx(lab: &Lab, pki: &Pki) -> PathBuf {
    lab.put("me.pfx", &pki.pfx("pw"))
}

fn sign_options(pfx: PathBuf, f: impl FnOnce(&mut DigitalSignOptions)) -> ToolOptions {
    let mut o = DigitalSignOptions {
        pfx_path: Some(pfx),
        pfx_password: Some(Secret::new("pw")),
        ..DigitalSignOptions::default()
    };
    f(&mut o);
    ToolOptions::DigitalSign(o)
}

fn verify_options(f: impl FnOnce(&mut VerifySignaturesOptions)) -> ToolOptions {
    let mut o = VerifySignaturesOptions::default();
    f(&mut o);
    ToolOptions::VerifySignatures(o)
}

fn sample(lab: &Lab) -> InputDoc {
    lab.doc(
        "contract.pdf",
        &pdfs::pdf_with_texts(&["Page one", "Page two"]),
    )
}

/// The text of the report written by Verify signatures.
fn report(lab: &Lab, signed: &InputDoc, options: ToolOptions) -> String {
    let out = lab.run(options, signed.clone()).unwrap();
    assert!(
        name(&out[0]).ends_with("(signature report).pdf"),
        "{}",
        name(&out[0])
    );
    // Long sentences are wrapped over several lines of the report.
    lab.texts(&out[0].path, None)
        .join("\n")
        .replace("\r\n", " ")
        .replace('\n', " ")
}

fn signed_doc(
    lab: &Lab,
    input: InputDoc,
    pki: &Pki,
    f: impl FnOnce(&mut DigitalSignOptions),
) -> InputDoc {
    let out = lab.run(sign_options(pfx(lab, pki), f), input).unwrap();
    InputDoc::new(&out[0].path)
}

#[test]
fn a_file_is_signed_and_the_report_calls_it_trusted() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let input = sample(&lab);
    let before = fs::read(&input.path).unwrap();
    let out = lab
        .run(sign_options(pfx(&lab, &pki), |_| {}), input.clone())
        .unwrap();
    assert_eq!(name(&out[0]), "contract (digitally signed).pdf");
    assert_eq!(out[0].pages, Some(2));
    assert_eq!(
        fs::read(&input.path).unwrap(),
        before,
        "the input is not touched"
    );
    let signed = fs::read(&out[0].path).unwrap();
    assert_eq!(
        &signed[..before.len()],
        &before[..],
        "the original bytes come first"
    );
    assert_eq!(lab.texts(&out[0].path, None), ["Page one", "Page two"]);

    let trust = lab.put("trust.pem", pki.root_pem().as_bytes());
    let text = report(
        &lab,
        &InputDoc::new(&out[0].path),
        verify_options(|o| o.trusted_certificates = Some(trust.clone())),
    );
    for expected in [
        "Signature report",
        "contract (digitally signed).pdf",
        "1 signatures: 1 valid and trusted",
        "1. Test Signer",
        "Valid and trusted",
        "Passed. The signature is valid",
        "Passed. The file is exactly as it was signed",
        "Test Root",
    ] {
        assert!(text.contains(expected), "{expected:?} missing:\n{text}");
    }
    lab.tidy();
}

#[test]
fn an_unknown_signer_leaves_open_questions() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let signed = signed_doc(&lab, sample(&lab), &pki, |_| {});
    let text = report(&lab, &signed, verify_options(|_| {}));
    assert!(
        text.contains("Valid, but not everything could be confirmed"),
        "{text}"
    );
    assert!(text.contains("not trusted"), "{text}");
    assert!(text.contains("Revocation: not asked"), "{text}");
    lab.tidy();
}

#[test]
fn a_changed_file_is_not_valid() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let signed = signed_doc(&lab, sample(&lab), &pki, |_| {});
    let mut bytes = fs::read(&signed.path).unwrap();
    let at = bytes.windows(8).position(|w| w == b"Page one").unwrap();
    bytes[at + 5] = b'1';
    let tampered = lab.doc("tampered.pdf", &bytes);
    let text = report(&lab, &tampered, verify_options(|_| {}));
    assert!(text.contains("Not valid"), "{text}");
    assert!(
        text.contains("Failed. The signed part of the file does not match"),
        "{text}"
    );
    lab.tidy();
}

#[test]
fn a_file_without_signatures_gets_a_plain_report() {
    let lab = Lab::new();
    let text = report(&lab, &sample(&lab), verify_options(|_| {}));
    assert!(text.contains("no digital signatures"), "{text}");
    lab.tidy();
}

#[test]
fn a_visible_signature_is_drawn_on_the_page() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let input = sample(&lab);
    let plain = lab.look(&input.path, 0, None);
    let signed = signed_doc(&lab, input, &pki, |o| {
        o.visible = true;
        o.page = 1;
        o.reason = Some("I approve".into());
    });
    let drawn = lab.look(&signed.path, 0, None);
    assert!(
        drawn.inked > plain.inked + 100,
        "the signature adds ink: {plain:?} then {drawn:?}"
    );
    // The second page is untouched.
    assert_eq!(
        lab.look(&signed.path, 1, None).inked,
        lab.look(&sample(&lab).path, 1, None).inked
    );
    lab.tidy();
}

#[test]
fn certifying_and_countersigning_are_reported() {
    let lab = Lab::new();
    let one = Pki::ec();
    let two = Pki::ec();
    let first = signed_doc(&lab, sample(&lab), &one, |o| o.certify = Certify::FormFill);
    let out = lab
        .run(
            sign_options(lab.put("two.pfx", &two.pfx("pw")), |_| {}),
            first,
        )
        .unwrap();
    let trust = lab.put(
        "trust.pem",
        format!("{}{}", one.root_pem(), two.root_pem()).as_bytes(),
    );
    let text = report(
        &lab,
        &InputDoc::new(&out[0].path),
        verify_options(|o| o.trusted_certificates = Some(trust.clone())),
    );
    assert!(text.contains("2 signatures: 2 valid and trusted"), "{text}");
    assert!(
        text.contains("Certifying signature: filling in forms allowed"),
        "{text}"
    );
    assert!(
        text.contains("Only signatures, validation data or document properties were added"),
        "{text}"
    );
    lab.tidy();
}

#[test]
fn a_wrong_password_writes_nothing() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let mut options = sign_options(pfx(&lab, &pki), |_| {});
    if let ToolOptions::DigitalSign(o) = &mut options {
        o.pfx_password = Some(Secret::new("wrong"));
    }
    let err = lab.run(options, sample(&lab)).unwrap_err();
    assert_eq!(err.code, ErrorCode::OptionInvalid);
    assert!(err.message.contains("password"), "{}", err.message);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}

#[test]
fn a_protected_file_cannot_be_signed() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let plain = lab.put("plain.pdf", &pdfs::pdf_with_texts(&["secret"]));
    let locked = lab.dir.path().join("locked.pdf");
    encrypt_pdf(&plain, &locked, "open");
    let mut input = InputDoc::new(&locked);
    input.password = Some(Password::new("open"));
    let err = lab
        .run(sign_options(pfx(&lab, &pki), |_| {}), input)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PdfEncrypted);
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}

#[test]
fn the_internet_is_not_used_without_consent() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let options = sign_options(pfx(&lab, &pki), |o| {
        o.level = SignLevel::BT;
        o.timestamp_url = Some("http://127.0.0.1:9/ts".into());
    });
    let err = lab.run(options, sample(&lab)).unwrap_err();
    assert_eq!(err.code, ErrorCode::NetworkConsentRequired);
    let revoke = verify_options(|o| o.check_revocation = true);
    assert_eq!(
        lab.run(revoke, sample(&lab)).unwrap_err().code,
        ErrorCode::NetworkConsentRequired
    );
    assert_eq!(fs::read_dir(lab.out()).unwrap().count(), 0);
    lab.tidy();
}

#[test]
fn a_server_that_is_down_fails_cleanly() {
    let lab = Lab::new();
    let pki = Pki::ec();
    let options = sign_options(pfx(&lab, &pki), |o| {
        o.level = SignLevel::BT;
        o.allow_network = true;
        o.timestamp_url = Some("http://127.0.0.1:9/ts".into());
    });
    let err = lab.run(options, sample(&lab)).unwrap_err();
    assert_eq!(err.code, ErrorCode::NetworkUnavailable);
    assert_eq!(
        fs::read_dir(lab.out()).unwrap().count(),
        0,
        "no partial file"
    );
    lab.tidy();
}

#[test]
fn an_archival_signature_is_made_and_checked_over_http() {
    let lab = Lab::new();
    let server = TestServer::bind();
    let base = server.base();
    let pki = Pki::build(&Spec {
        addresses: true,
        base_url: Some(base.clone()),
        ..Spec::default()
    });
    let now = i64::try_from(quirewise_engine_sign_cms::testkit::now()).unwrap();
    let ocsp = pki.ocsp(&pki.leaf_der, None, now - 10, Some(now + 86_400));
    let tsa_state = std::sync::Arc::new(Tsa::new());
    let tsa_for_server = std::sync::Arc::clone(&tsa_state);
    let _guard = server.start(move |path, body| match path {
        "/ts" => tsa_for_server.answer(body).ok(),
        "/ocsp" => Some(ocsp.clone()),
        _ => None,
    });

    let options = sign_options(pfx(&lab, &pki), |o| {
        o.level = SignLevel::BLta;
        o.allow_network = true;
        o.timestamp_url = Some(format!("{base}/ts"));
        o.reason = Some("Archive".into());
    });
    let out = lab.run(options, sample(&lab)).unwrap();
    assert_eq!(out[0].pages, Some(2));

    // Checked later with no connection at all: everything needed is in the file.
    let trust = lab.put(
        "trust.pem",
        format!("{}{}", pki.root_pem(), tsa_state.pki.root_pem()).as_bytes(),
    );
    let text = report(
        &lab,
        &InputDoc::new(&out[0].path),
        verify_options(|o| o.trusted_certificates = Some(trust.clone())),
    );
    for expected in [
        "2 signatures",
        "Time stamp over the document",
        "Valid and trusted",
        "The file stores revocation answers",
        "The answers stored in the file show that no certificate of the chain was revoked",
        "Test TSA",
    ] {
        assert!(text.contains(expected), "{expected:?} missing:\n{text}");
    }
    assert!(!text.contains("Not valid"), "{text}");
    lab.tidy();
}

#[test]
fn a_revocation_check_over_http_finds_a_revoked_certificate() {
    let lab = Lab::new();
    let server = TestServer::bind();
    let base = server.base();
    let pki = Pki::build(&Spec {
        addresses: true,
        base_url: Some(base.clone()),
        ..Spec::default()
    });
    let signed = signed_doc(&lab, sample(&lab), &pki, |_| {});
    let now = i64::try_from(quirewise_engine_sign_cms::testkit::now()).unwrap();
    let revoked = pki.ocsp(&pki.leaf_der, Some(now - 3600), now - 10, Some(now + 3600));
    let _guard = server.start(move |path, _| (path == "/ocsp").then(|| revoked.clone()));
    let trust = lab.put("trust.pem", pki.root_pem().as_bytes());
    let text = report(
        &lab,
        &signed,
        verify_options(|o| {
            o.check_revocation = true;
            o.allow_network = true;
            o.trusted_certificates = Some(trust.clone());
        }),
    );
    assert!(text.contains("Not valid"), "{text}");
    assert!(text.contains("was revoked on"), "{text}");
    assert!(
        text.contains("the certificate authorities were asked"),
        "{text}"
    );
    lab.tidy();
}

#[test]
fn a_visible_signature_lands_where_it_is_seen_on_a_rotated_page() {
    // The box is given on the page as displayed, so it must end up in the top left whatever the
    // rotation of the page.
    for rotate in [0u32, 90, 180, 270] {
        let lab = Lab::new();
        let pki = Pki::ec();
        let input = lab.doc("rotated.pdf", &pdfs::pdf_from_content("", rotate));
        assert_eq!(
            lab.look(&input.path, 0, None).inked,
            0,
            "the page starts blank"
        );
        let signed = signed_doc(&lab, input, &pki, |o| {
            o.visible = true;
            o.left = 0.05;
            o.top = 0.05;
            o.width = 0.35;
            o.height = 0.1;
            o.reason = Some("I approve".into());
        });
        let look = lab.look(&signed.path, 0, None);
        assert!(
            look.inked > 100,
            "{rotate}: the signature is drawn: {look:?}"
        );
        assert!(
            look.center_x < 0.35 && look.center_y < 0.2,
            "{rotate}: the signature is in the top left corner of the displayed page: {look:?}"
        );
        lab.tidy();
    }
}

#[test]
fn a_linearized_or_packed_file_is_signed_and_published() {
    // The result is checked by qpdf before it is published, so a file that qpdf dislikes would
    // not come out at all.
    for flags in [["--linearize"], ["--object-streams=generate"]] {
        let lab = Lab::new();
        let pki = Pki::ec();
        let source = lab.put("plain.pdf", &pdfs::pdf_with_texts(&["one", "two"]));
        let changed = lab.dir.path().join("changed.pdf");
        let status = std::process::Command::new(test_support::qpdf_exe().unwrap())
            .args(flags)
            .arg(&source)
            .arg(&changed)
            .status()
            .unwrap();
        assert!(status.success());
        let out = lab
            .run(
                sign_options(pfx(&lab, &pki), |_| {}),
                InputDoc::new(&changed),
            )
            .unwrap();
        assert_eq!(out[0].pages, Some(2), "{flags:?}");
        lab.tidy();
    }
}
