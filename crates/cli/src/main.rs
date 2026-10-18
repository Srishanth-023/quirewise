//! `qw` entry point.
#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let (text, code) = quirewise_cli::run(std::env::args_os());
    if code == 0 {
        println!("{text}");
    } else {
        eprintln!("{text}");
    }
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
