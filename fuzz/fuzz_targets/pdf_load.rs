#![no_main]

// Phase 1 covers the code that touches untrusted bytes before any parser: magic-byte
// detection and the worker IPC framing. The PDFium load path joins this target in Phase 2.
use application::input::sniff;
use application::ipc::{Request, Response, read_frame};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = sniff(data);
    let _ = read_frame::<Request>(&mut &data[..]);
    let _ = read_frame::<Response>(&mut &data[..]);
});
