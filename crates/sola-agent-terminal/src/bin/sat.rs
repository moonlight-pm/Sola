//! `sat` — nickname for `solactl at …`. The Workspaces app must be running.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let solactl = neighbor_solactl().unwrap_or_else(|| PathBuf::from("solactl"));
    let mut cmd = Command::new(&solactl);
    cmd.arg("at");
    cmd.args(std::env::args().skip(1));
    let err = cmd.exec();
    eprintln!("sat: failed to exec {}: {err}", solactl.display());
    std::process::exit(3);
}

fn neighbor_solactl() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join("solactl");
    candidate.exists().then_some(candidate)
}
