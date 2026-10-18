//! Quirewise worker process: the only process that parses untrusted PDF bytes, run inside a Job Object.
#![forbid(unsafe_code)]

fn main() {
    println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
}
