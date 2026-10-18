#![no_main]

use application::components::parse_component_manifest;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(manifest) = parse_component_manifest(data) {
        assert!(manifest.validate().is_ok());
    }
});
