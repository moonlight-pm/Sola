//! The opener's command line, as `xdg-open` and as `x-www-browser`.
//!
//! `xdg-open { file | URL }`: opens it with the handler Sola keeps for its
//! scheme or type ([`crate::open`]). Exit status as xdg-open's: 0 opened,
//! 1 usage, 2 no such file, 3 nothing opens it, 4 the handler failed.

use std::process::ExitCode;

const USAGE: &str = "usage: xdg-open { file | URL }";

pub fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--help" || flag == "--manual" => {
            println!("{USAGE}\n\nOpens a file or a URL with the app Sola keeps for its type or scheme.");
            ExitCode::SUCCESS
        }
        [flag] if flag == "--version" => {
            println!("xdg-open (Sola) {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        [target] if !target.starts_with("--") => {
            sola_core::log::init_quiet("xdg-open");
            match crate::open(target) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::warn!(%target, error = %e, "xdg-open failed");
                    eprintln!("xdg-open: {e}");
                    ExitCode::from(e.exit_code() as u8)
                }
            }
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(1)
        }
    }
}
