//! Component use cases: parsing the manifest and listing installed components.

use domain::ErrorCode;
use domain::components::{ComponentEntry, ComponentManifest};
use std::sync::Arc;

use crate::error::AppError;
use crate::ports::ComponentStore;

/// Largest accepted manifest, 1 MiB.
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

/// Parses and validates a `components.json` document. Signature checks happen before this,
/// in infrastructure; this only guards the shape of the data.
pub fn parse_component_manifest(bytes: &[u8]) -> Result<ComponentManifest, AppError> {
    let invalid = |why: &str| {
        AppError::new(
            ErrorCode::ComponentVerifyFailed,
            format!(
                "The component list is not valid ({why}). Do not install from it; check for an app update."
            ),
        )
    };
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(invalid("too large"));
    }
    let manifest: ComponentManifest =
        serde_json::from_slice(bytes).map_err(|_| invalid("unreadable"))?;
    manifest.validate().map_err(|e| invalid(&e.to_string()))?;
    Ok(manifest)
}

/// The `ListComponents` use case.
pub struct ListComponents {
    store: Arc<dyn ComponentStore>,
}

impl ListComponents {
    /// Creates the use case over a component store.
    pub fn new(store: Arc<dyn ComponentStore>) -> Self {
        Self { store }
    }

    /// Installed components, sorted by id then version.
    pub fn run(&self) -> Result<Vec<ComponentEntry>, AppError> {
        let mut list = self.store.installed()?;
        list.sort_by(|a, b| (&a.id, &a.version).cmp(&(&b.id, &b.version)));
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::Bytes;
    use domain::components::Arch;

    fn entry(id: &str) -> ComponentEntry {
        ComponentEntry {
            id: id.to_owned(),
            version: "1.0".to_owned(),
            arch: Arch::Any,
            url: "https://example.invalid/x.zip".to_owned(),
            size: Bytes::from_mb(1),
            sha256: "b".repeat(64),
        }
    }

    #[test]
    fn parses_a_valid_manifest() {
        let manifest = ComponentManifest {
            schema_version: 1,
            components: vec![entry("tessdata-eng")],
        };
        let bytes = serde_json::to_vec(&manifest).unwrap();
        assert_eq!(parse_component_manifest(&bytes).unwrap(), manifest);
    }

    #[test]
    fn rejects_bad_manifests_with_the_verify_code() {
        for bytes in [
            b"not json".to_vec(),
            br#"{"schema_version":1,"components":[],"extra":1}"#.to_vec(),
            br#"{"schema_version":9,"components":[]}"#.to_vec(),
            vec![b' '; MAX_MANIFEST_BYTES + 1],
        ] {
            let err = parse_component_manifest(&bytes).unwrap_err();
            assert_eq!(err.code, ErrorCode::ComponentVerifyFailed);
        }
    }

    struct Store(Vec<ComponentEntry>);
    impl ComponentStore for Store {
        fn installed(&self) -> Result<Vec<ComponentEntry>, AppError> {
            Ok(self.0.clone())
        }
        fn locate(&self, _: &str) -> Option<std::path::PathBuf> {
            None
        }
    }

    #[test]
    fn lists_sorted_by_id() {
        let uc = ListComponents::new(Arc::new(Store(vec![entry("zeta"), entry("alpha")])));
        let ids: Vec<String> = uc.run().unwrap().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["alpha", "zeta"]);
    }
}
