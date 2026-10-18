#![no_main]

use domain::{Duplicates, PageRange};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(range) = PageRange::parse(text) {
        // Display must re-parse to the same range.
        let again = PageRange::parse(&range.to_string());
        assert_eq!(again.as_ref(), Ok(&range));
        for count in [0, 1, 7, 1000] {
            if let Ok(selection) = range.resolve(count, Duplicates::Allow) {
                assert!(selection.pages().iter().all(|p| p.get() < count));
            }
        }
    }
});
