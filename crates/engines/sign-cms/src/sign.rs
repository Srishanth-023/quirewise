//! Signing a PDF: the signature, an optional time stamp, long-term validation data and an
//! optional time stamp over the whole file.

use std::path::Path;

use application::AppError;
use application::runtime::CancellationToken;
use application::signing::{KeyChoice, SignOutcome, SignRequest, WebClient};
use der::Decode;
use domain::ErrorCode;

use crate::algo::Hash;
use crate::certs::Cert;
use crate::cmsdoc::{self, Content, ID_DATA};
use crate::dss;
use crate::keys::{Identity, load_pfx};
use crate::pdfsign::{self, Placement, SigPlan};
use crate::revocation::{self, Evidence, Freshness, Outcome};
use crate::trust::{self, TrustStore};
use crate::tsa;

/// Space kept for the signature besides the certificates, and for a time stamp token.
const BASE_RESERVE: usize = 3_000;
const STAMP_RESERVE: usize = 14_000;
/// Space for the time stamp over the whole file, which carries the certificates of its server.
const DOCUMENT_STAMP_RESERVE: usize = 16_000;
/// Largest input read into memory.
const MAX_INPUT: u64 = 1 << 30;

fn needs_network() -> AppError {
    AppError::new(
        ErrorCode::NetworkConsentRequired,
        "This level of signature asks servers on the internet, and the connection was not agreed to. Allow the connection, or choose level b-b.",
    )
}

fn read_input(path: &Path) -> Result<Vec<u8>, AppError> {
    let meta = std::fs::metadata(path).map_err(|e| AppError::from_io("read the PDF", &e))?;
    if meta.len() > MAX_INPUT {
        return Err(AppError::new(
            ErrorCode::PdfLimitExceeded,
            "The PDF is too large to sign. Split it or compress it first.",
        ));
    }
    std::fs::read(path).map_err(|e| AppError::from_io("read the PDF", &e))
}

fn identity(key: &KeyChoice) -> Result<Identity, AppError> {
    match key {
        KeyChoice::Pfx { path, password } => {
            let bytes = std::fs::read(path)
                .map_err(|e| AppError::from_io("read the certificate file", &e))?;
            load_pfx(&bytes, password.expose())
        }
        KeyChoice::Store { thumbprint } => crate::store::identity(thumbprint),
    }
}

/// A file with a signature added.
struct Added {
    /// The whole new file.
    bytes: Vec<u8>,
    /// The `/Contents` of the signature as stored, zero fill included.
    value: Vec<u8>,
    /// The signature structure without the fill.
    cms: Vec<u8>,
    /// Name of the signature field.
    field: String,
}

/// Makes one signature of `bytes` and returns the new file and what names the signature.
fn add_signature(
    bytes: &[u8],
    id: &Identity,
    request: &SignRequest,
    web: Option<&dyn WebClient>,
    cancel: &CancellationToken,
) -> Result<Added, AppError> {
    let signer = id.signer();
    let name = request
        .signer_name
        .clone()
        .unwrap_or_else(|| signer.subject());
    let chain_bytes: usize = id.chain.iter().map(|c| c.der.len()).sum();
    let mut reserve = chain_bytes
        + BASE_RESERVE
        + if request.level.timestamped() {
            STAMP_RESERVE
        } else {
            0
        };
    for _attempt in 0..2 {
        cancel.check()?;
        let plan = SigPlan {
            field: request.field.clone(),
            visible: request.visible.map(|b| Placement {
                page: b.page,
                left: b.left,
                top: b.top,
                width: b.width,
                height: b.height,
            }),
            lines: pdfsign::default_lines(
                &name,
                request.signed_at,
                request.reason.as_deref(),
                request.location.as_deref(),
                request.contact.as_deref(),
            ),
            name: Some(name.clone()),
            reason: request.reason.clone(),
            location: request.location.clone(),
            contact: request.contact.clone(),
            signed_at: request.signed_at,
            certify: request.certify.permission(),
            document_timestamp: false,
            reserve,
        };
        let prepared = pdfsign::prepare(bytes, &plan)?;
        let hash = id.key.hash();
        let digest = prepared.digest(hash);
        let built = cmsdoc::sign(
            id,
            &Content {
                oid: ID_DATA,
                attached: None,
                digest,
            },
        )
        .map_err(|e| {
            AppError::new(
                ErrorCode::OutputVerifyFailed,
                format!("The signature could not be made: {e}."),
            )
        })?;
        let mut unsigned = Vec::new();
        if request.level.timestamped() {
            let web = web.ok_or_else(needs_network)?;
            let url = request.timestamp_url.as_deref().ok_or_else(|| {
                AppError::new(
                    ErrorCode::OptionInvalid,
                    "A time stamp server address is needed for this level.",
                )
            })?;
            let token = tsa::stamp(web, url, hash, &hash.digest(&built.signature))?;
            let any = der::Any::from_der(&token)
                .map_err(|_| AppError::internal("reading the time stamp"))?;
            unsigned.push(x509_cert::attr::Attribute {
                oid: cmsdoc::ATTR_TIMESTAMP_TOKEN,
                values: der::asn1::SetOfVec::try_from(vec![any])
                    .map_err(|_| AppError::internal("storing the time stamp"))?,
            });
        }
        let cms = built.to_der(unsigned).map_err(|e| {
            AppError::new(
                ErrorCode::OutputVerifyFailed,
                format!("The signature could not be made: {e}."),
            )
        })?;
        if cms.len() > prepared.capacity() {
            reserve = cms.len() + BASE_RESERVE;
            continue;
        }
        let value = {
            let mut padded = cms.clone();
            padded.resize(prepared.capacity(), 0);
            padded
        };
        let field = prepared.field_name.clone();
        let out = prepared.embed(&cms).map_err(|e| {
            AppError::new(
                ErrorCode::OutputVerifyFailed,
                format!("The signature does not fit: {e}."),
            )
        })?;
        return Ok(Added {
            bytes: out,
            value,
            cms,
            field,
        });
    }
    Err(AppError::internal("making room for the signature"))
}

/// Signs `input` as `request` says and writes the result to `dest`.
pub fn sign_file(
    input: &Path,
    dest: &Path,
    request: &SignRequest,
    web: Option<&dyn WebClient>,
    cancel: &CancellationToken,
) -> Result<SignOutcome, AppError> {
    if request.level.timestamped() && web.is_none() {
        return Err(needs_network());
    }
    let id = identity(&request.key)?;
    sign_with(&id, input, dest, request, web, cancel)
}

/// Signs `input` with an identity that is already loaded.
pub(crate) fn sign_with(
    id: &Identity,
    input: &Path,
    dest: &Path,
    request: &SignRequest,
    web: Option<&dyn WebClient>,
    cancel: &CancellationToken,
) -> Result<SignOutcome, AppError> {
    if request.level.timestamped() && web.is_none() {
        return Err(needs_network());
    }
    check_certificate(id, request.signed_at)?;
    let original = read_input(input)?;
    cancel.check()?;
    let Added {
        mut bytes,
        value,
        cms,
        field,
    } = add_signature(&original, id, request, web, cancel)?;
    let mut warnings: Vec<String> = Vec::new();

    if request.level.long_term() {
        cancel.check()?;
        let web = web.ok_or_else(needs_network)?;
        let (certs, evidence, issues) = collect_validation_data(id, &cms, web, request.signed_at)?;
        warnings.extend(issues);
        bytes = dss::append(&bytes, &value, &certs, &evidence, request.signed_at)?;
    }
    if request.level.archival() {
        cancel.check()?;
        let web = web.ok_or_else(needs_network)?;
        let url = request.timestamp_url.as_deref().ok_or_else(|| {
            AppError::new(
                ErrorCode::OptionInvalid,
                "A time stamp server address is needed for this level.",
            )
        })?;
        bytes = add_document_timestamp(&bytes, web, url, request.signed_at)?;
    }

    cancel.check()?;
    std::fs::write(dest, &bytes).map_err(|e| AppError::from_io("write the signed PDF", &e))?;
    Ok(SignOutcome {
        field,
        signer: id.signer().subject(),
        warnings,
    })
}

/// Refuses to sign with a certificate that cannot be used now.
fn check_certificate(id: &Identity, at: i64) -> Result<(), AppError> {
    let cert = id.signer();
    if at < cert.not_before() || at > cert.not_after() {
        return Err(AppError::new(
            ErrorCode::OptionInvalid,
            format!(
                "The certificate of {} is not valid today (valid from {} to {}). Choose another certificate.",
                cert.subject(),
                crate::timefmt::day(cert.not_before()),
                crate::timefmt::day(cert.not_after())
            ),
        ));
    }
    if !cert.may_sign_documents() {
        return Err(AppError::new(
            ErrorCode::OptionInvalid,
            format!(
                "The certificate of {} is not meant for signing documents. Choose another certificate.",
                cert.subject()
            ),
        ));
    }
    Ok(())
}

/// The certificates and revocation answers that let the signature be checked later.
fn collect_validation_data(
    id: &Identity,
    cms: &[u8],
    web: &dyn WebClient,
    signed_at: i64,
) -> Result<(Vec<Cert>, Evidence, Vec<String>), AppError> {
    let mut certs: Vec<Cert> = id.chain.clone();
    let mut chains: Vec<Vec<Cert>> = vec![id.chain.clone()];
    // The server that made the time stamp has a chain of its own.
    if let Ok(parsed) = cmsdoc::parse(cms)
        && let Some(token) = parsed.timestamp_token()
        && let Ok((_, token_parsed, server)) = tsa::read_token(&token)
    {
        let chain = trust::build(
            &server,
            &token_parsed.certs,
            &TrustStore::empty(),
            signed_at,
        );
        for c in &chain.certs {
            if !certs.contains(c) {
                certs.push(c.clone());
            }
        }
        chains.push(chain.certs);
    }
    let mut evidence = Evidence::default();
    let mut problems: Vec<String> = Vec::new();
    for chain in &chains {
        let (e, p) = revocation::gather(web, chain);
        evidence.merge(e);
        problems.extend(p);
    }
    if evidence.is_empty() {
        return Err(AppError::new(
            ErrorCode::NetworkUnavailable,
            format!(
                "The revocation answers for the long-term signature could not be fetched ({}). Check that the certificates name revocation servers and that you are online, or choose level b-t.",
                problems.join("; ")
            ),
        ));
    }
    // A signer whose certificate is revoked must not sign.
    if let [signer, issuer, ..] = id.chain.as_slice() {
        let now_ish = signed_at;
        if let Outcome::Revoked(at) =
            revocation::assess(signer, issuer, &evidence, Freshness::Live(now_ish))
        {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                format!(
                    "The certificate of {} was revoked on {}. Choose another certificate.",
                    signer.subject(),
                    crate::timefmt::day(at)
                ),
            ));
        }
    }
    Ok((certs, evidence, problems))
}

/// Appends a time stamp over everything so far.
fn add_document_timestamp(
    bytes: &[u8],
    web: &dyn WebClient,
    url: &str,
    at: i64,
) -> Result<Vec<u8>, AppError> {
    let plan = SigPlan {
        signed_at: at,
        document_timestamp: true,
        reserve: DOCUMENT_STAMP_RESERVE,
        ..SigPlan::default()
    };
    let prepared = pdfsign::prepare(bytes, &plan)?;
    let hash = Hash::Sha256;
    let token = tsa::stamp(web, url, hash, &prepared.digest(hash))?;
    prepared.embed(&token).map_err(|e| {
        AppError::new(
            ErrorCode::OutputVerifyFailed,
            format!("The time stamp over the file does not fit: {e}."),
        )
    })
}
