//! `cargo make oath …` — Sola's packages for Oath, made and applied
//! with the `oath` CLI on an Oath machine.
//!
//! - `stage` builds the binaries and assembles each package's tree under
//!   `target/oath/<name>/` (no root, no catalog change).
//! - `apply` stages, then `sudo oath store`s each tree, pins it with
//!   `sudo oath set pkg:<name> hash=… needs=[…]` (needs at the hashes
//!   this machine has live) and `sudo oath apply`s them together. PID 1
//!   restarts the services that run from a package that moved. Like
//!   `cargo make install`, apply only with the owner's go-ahead.

mod catalog;
mod packages;
mod stage;

use std::process::exit;

use packages::PACKAGES;
use stage::Staged;

pub enum Action {
    Stage,
    Apply,
}

pub fn run(action: Action, release: bool) {
    if !catalog::on_oath() {
        eprintln!("cargo make oath: not an Oath machine (no /oath/INDEX.md)");
        exit(1);
    }
    let staged = stage::stage(release);
    if let Action::Apply = action
        && let Err(e) = apply(&staged)
    {
        eprintln!("cargo make oath apply: {e}");
        exit(1);
    }
}

fn apply(staged: &[Staged]) -> Result<(), String> {
    let mut stored: Vec<(&str, String)> = Vec::new();
    for s in staged {
        let hash = catalog::store(&s.pkg.id(), &s.dir)?;
        stored.push((s.pkg.name, hash));
    }
    for (s, (_, hash)) in staged.iter().zip(&stored) {
        let id = s.pkg.id();
        let was = catalog::live_hash(&id).unwrap_or_else(|_| "none".into());
        let mut needs = Vec::new();
        for need in s.pkg.needs {
            let need_hash = match stored.iter().find(|(n, _)| n == need) {
                Some((_, h)) => h.clone(),
                None => catalog::live_hash(&format!("pkg:{need}"))?,
            };
            needs.push((format!("pkg:{need}"), need_hash));
        }
        catalog::set(&id, hash, &needs)?;
        println!("[oath] {id}: {} -> {}", short(&was), short(hash));
    }
    let ids: Vec<String> = PACKAGES.iter().map(|p| p.id()).collect();
    catalog::apply(&ids)?;
    println!("[oath] applied {}", ids.join(" "));
    Ok(())
}

fn short(hash: &str) -> &str {
    let hex = hash.strip_prefix("sha256-").unwrap_or(hash);
    &hex[..hex.len().min(12)]
}
