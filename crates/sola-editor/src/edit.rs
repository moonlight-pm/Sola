//! `edit <file>…`: open files in Sola's editor from anywhere, and return.
//! A file that doesn't exist yet opens empty and is made when saved. With no
//! file, the editor comes up.

use std::path::Path;
use std::process::ExitCode;

const APP_ID: &str = "sola-editor";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.is_empty()).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("usage: edit [file]…");
        return ExitCode::SUCCESS;
    }
    let targets: Vec<String> = if args.is_empty() {
        vec![String::new()]
    } else {
        args.iter().map(|a| absolute(Path::new(a))).collect()
    };
    let mut failed = false;
    for target in &targets {
        if let Err(e) = sola_open::to_app(APP_ID, target) {
            eprintln!("edit: {target}: {e}");
            failed = true;
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

/// The running editor has its own working directory: send it the whole path.
fn absolute(path: &Path) -> String {
    std::fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}
