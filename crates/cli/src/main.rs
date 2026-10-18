//! Quirewise headless CLI for tests and power users.
#![forbid(unsafe_code)]

fn main() {
    println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
}
