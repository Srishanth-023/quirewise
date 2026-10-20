//! Who is trusted: the root certificates of Windows plus any the person adds, and building the
//! chain from a signer certificate up to one of them.

use std::sync::OnceLock;

use crate::certs::Cert;
use crate::timefmt;

/// Root certificates that are trusted.
#[derive(Debug, Clone, Default)]
pub struct TrustStore {
    anchors: Vec<Cert>,
}

fn system_roots() -> &'static [Cert] {
    static ROOTS: OnceLock<Vec<Cert>> = OnceLock::new();
    ROOTS.get_or_init(|| {
        let found = rustls_native_certs::load_native_certs();
        found
            .certs
            .iter()
            .filter_map(|c| Cert::parse(c.as_ref()).ok())
            .collect()
    })
}

impl TrustStore {
    /// A store with nothing in it.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The root certificates of Windows.
    pub fn system() -> Self {
        Self {
            anchors: system_roots().to_vec(),
        }
    }

    /// Adds certificates.
    pub fn add(&mut self, certs: impl IntoIterator<Item = Cert>) {
        self.anchors.extend(certs);
    }

    /// True when this exact certificate is trusted.
    pub fn trusts(&self, cert: &Cert) -> bool {
        self.anchors.iter().any(|a| a == cert)
    }

    /// The trusted certificate that issued `cert`, when there is one.
    pub fn issuer_of(&self, cert: &Cert) -> Option<&Cert> {
        self.anchors
            .iter()
            .find(|a| cert.check_issued_by(a).is_ok())
    }
}

/// How far a chain got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// The chain ends at a trusted root.
    Trusted,
    /// The signer certificate is its own issuer and is not trusted.
    SelfSigned,
    /// The chain ends at a certificate that is not trusted (its issuer may be missing).
    Unknown,
}

/// A chain from a certificate up to where it stops.
#[derive(Debug, Clone)]
pub struct Chain {
    /// The certificates, the signer first. A trusted root, when reached, is the last.
    pub certs: Vec<Cert>,
    /// Where it ends.
    pub ending: Ending,
    /// Things that are wrong with the chain, in plain words. Empty when it is sound.
    pub faults: Vec<String>,
}

/// Builds the chain of `leaf` from `pool` and the trust store, and checks it at time `at`.
pub fn build(leaf: &Cert, pool: &[Cert], store: &TrustStore, at: i64) -> Chain {
    let mut certs = vec![leaf.clone()];
    let mut ending = Ending::Unknown;
    for _ in 0..10 {
        let Some(current) = certs.last() else { break };
        if store.trusts(current) {
            ending = Ending::Trusted;
            break;
        }
        if let Some(anchor) = store.issuer_of(current) {
            certs.push(anchor.clone());
            ending = Ending::Trusted;
            break;
        }
        if current.is_self_issued() {
            if certs.len() == 1 {
                ending = Ending::SelfSigned;
            }
            break;
        }
        let next = pool
            .iter()
            .find(|c| !certs.contains(c) && current.check_issued_by(c).is_ok() && c.is_ca());
        match next {
            Some(c) => certs.push(c.clone()),
            None => break,
        }
    }
    let faults = check(&certs, at);
    Chain {
        certs,
        ending,
        faults,
    }
}

/// Problems with the certificates of a chain at time `at`.
fn check(certs: &[Cert], at: i64) -> Vec<String> {
    let mut faults = Vec::new();
    for (i, cert) in certs.iter().enumerate() {
        if at < cert.not_before() {
            faults.push(format!(
                "The certificate of {} was not valid yet on {}.",
                cert.subject(),
                timefmt::day(at)
            ));
        } else if at > cert.not_after() {
            faults.push(format!(
                "The certificate of {} expired on {}.",
                cert.subject(),
                timefmt::day(cert.not_after())
            ));
        }
        if i > 0 {
            if !cert.is_ca() {
                faults.push(format!(
                    "{} is not allowed to issue certificates.",
                    cert.subject()
                ));
            }
            if !cert.may_sign_certificates() {
                faults.push(format!(
                    "The key of {} is not allowed to sign certificates.",
                    cert.subject()
                ));
            }
            if let Some(limit) = cert.path_length()
                && i - 1 > usize::from(limit)
            {
                faults.push(format!(
                    "The chain below {} is longer than it allows.",
                    cert.subject()
                ));
            }
        }
    }
    faults
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{Pki, Spec, now};

    fn store_with(root: &[u8]) -> TrustStore {
        let mut s = TrustStore::empty();
        s.add([Cert::parse(root).unwrap()]);
        s
    }

    fn t() -> i64 {
        i64::try_from(now()).unwrap()
    }

    #[test]
    fn a_chain_reaches_a_trusted_root_through_an_intermediate() {
        let pki = Pki::build(&Spec {
            intermediate: true,
            ..Spec::default()
        });
        let leaf = Cert::parse(&pki.leaf_der).unwrap();
        let pool: Vec<Cert> = pki
            .chain()
            .iter()
            .map(|d| Cert::parse(d).unwrap())
            .collect();
        let chain = build(&leaf, &pool[..2], &store_with(&pki.root_der), t());
        assert_eq!(chain.ending, Ending::Trusted);
        let names: Vec<String> = chain.certs.iter().map(Cert::subject).collect();
        assert_eq!(names, ["Test Signer", "Test Intermediate", "Test Root"]);
        assert!(chain.faults.is_empty(), "{:?}", chain.faults);
    }

    #[test]
    fn an_unknown_root_is_not_trusted() {
        let pki = Pki::ec();
        let leaf = Cert::parse(&pki.leaf_der).unwrap();
        let pool: Vec<Cert> = pki
            .chain()
            .iter()
            .map(|d| Cert::parse(d).unwrap())
            .collect();
        let chain = build(&leaf, &pool, &TrustStore::empty(), t());
        assert_eq!(chain.ending, Ending::Unknown);
        assert_eq!(chain.certs.len(), 2, "the leaf and the root it came with");
    }

    #[test]
    fn a_self_signed_certificate_is_called_that() {
        let pki = Pki::ec();
        let root = Cert::parse(&pki.root_der).unwrap();
        let chain = build(&root, &[], &TrustStore::empty(), t());
        assert_eq!(chain.ending, Ending::SelfSigned);
        let trusted = build(&root, &[], &store_with(&pki.root_der), t());
        assert_eq!(trusted.ending, Ending::Trusted);
    }

    #[test]
    fn a_missing_issuer_leaves_the_chain_open() {
        let pki = Pki::ec();
        let leaf = Cert::parse(&pki.leaf_der).unwrap();
        let chain = build(&leaf, &[], &TrustStore::empty(), t());
        assert_eq!(chain.ending, Ending::Unknown);
        assert_eq!(chain.certs.len(), 1);
    }

    #[test]
    fn an_expired_certificate_is_a_fault() {
        let pki = Pki::build(&Spec {
            expired: true,
            ..Spec::default()
        });
        let leaf = Cert::parse(&pki.leaf_der).unwrap();
        let chain = build(&leaf, &[], &store_with(&pki.root_der), t());
        assert_eq!(chain.ending, Ending::Trusted);
        assert!(
            chain.faults.iter().any(|f| f.contains("expired")),
            "{:?}",
            chain.faults
        );
        // Checked at a time inside its period, the same certificate is fine.
        let then = leaf.not_before() + 3600;
        assert!(
            build(&leaf, &[], &store_with(&pki.root_der), then)
                .faults
                .is_empty()
        );
        let before = build(
            &leaf,
            &[],
            &store_with(&pki.root_der),
            leaf.not_before() - 10,
        );
        assert!(before.faults.iter().any(|f| f.contains("not valid yet")));
    }

    #[test]
    fn a_leaf_cannot_stand_in_for_an_authority() {
        // A signer certificate issued by another signer certificate is not a chain.
        let pki = Pki::ec();
        let leaf = Cert::parse(&pki.leaf_der).unwrap();
        let other = Cert::parse(&Pki::ec().leaf_der).unwrap();
        let chain = build(&leaf, &[other], &TrustStore::empty(), t());
        assert_eq!(chain.certs.len(), 1);
    }

    #[test]
    fn the_system_store_loads() {
        // Not asserting its size: a build machine may have an empty store.
        let _ = TrustStore::system();
    }
}
