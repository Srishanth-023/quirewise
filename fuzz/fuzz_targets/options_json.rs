#![no_main]

use domain::ToolOptions;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(options) = serde_json::from_slice::<ToolOptions>(data) {
        // Anything that parses must serialize and parse back identically.
        let json = serde_json::to_vec(&options).expect("serializable");
        let back: ToolOptions = serde_json::from_slice(&json).expect("round trip");
        assert_eq!(back, options);
    }
});
