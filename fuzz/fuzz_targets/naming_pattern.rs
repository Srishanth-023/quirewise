#![no_main]

use domain::naming::{DateTimeParts, NameContext, OutputNamingPattern, sanitize_stem};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let stem = sanitize_stem(text);
    assert!(!stem.is_empty());
    assert!(!stem.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']));

    if let Ok(pattern) = OutputNamingPattern::parse(text) {
        let ctx = NameContext {
            filename: text,
            tool: text,
            now: DateTimeParts { year: 2026, month: 1, day: 1, hour: 0, minute: 0, second: 0 },
            sequence: Some(1),
            page: Some(1),
            pages: Some(1),
            bookmark: Some(text),
            text: Some(text),
        };
        if let Ok(name) = pattern.render(&ctx) {
            assert!(!name.is_empty());
            assert!(!name.contains(['/', '\\', ':']));
        }
    }
});
