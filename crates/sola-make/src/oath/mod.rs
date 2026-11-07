//! `cargo make oath …` — Sola's packages for Oath, made and applied
//! with the `oath` CLI on an Oath machine.
//!
//! - `stage` builds the binaries and assembles each package's tree under
//!   `target/oath/<name>/` (no root, no catalog change).
//! - `apply` stages, then `sudo oath store`s each tree and pins the ones
//!   that changed (tree or needs) with `sudo oath set pkg:<name> hash=…
//!   needs=[…]` (needs at the hashes this machine has live), and `sudo
//!   oath apply`s them together. PID 1 restarts the services that run from
//!   a package that moved. It keeps what is live: a live pin made from this
//!   repo at a commit this tree lacks refuses it ([`provenance`]; `--force`
//!   drops that work). Named packages (`apply sola`) stage and move alone.
//!   Only with the owner's go-ahead (`oath-packages`).
//! - `publish` builds each package from a plan in `oath build`'s sandbox,
//!   uploads the plans and sources to Oath's origin, pins and applies the
//!   products here as `apply` does, then `oath publish`es each as its
//!   signed head (Oath R72), which every machine trusting novus's key for
//!   `sola-*` takes at its next `oath update` ([`publish`]).
//!   Outward-facing: only with the owner's go-ahead.

mod catalog;
mod cpus;
mod origin;
mod packages;
mod plan;
mod provenance;
mod prune;
mod publish;
mod stage;
mod vendor;

use std::path::Path;
use std::process::exit;

use stage::Staged;

pub enum Action {
    Stage,
    /// `force`: pin over live work this tree lacks. `only`: these
    /// packages (by name), or every one when empty.
    Apply { force: bool, only: Vec<String> },
    /// `local`: build the plans here; upload, pin and publish nothing.
    /// `force`: as for `Apply`.
    Publish { local: bool, force: bool },
}

pub fn run(action: Action, release: bool) {
    if !catalog::on_oath() {
        eprintln!("cargo make oath: not an Oath machine (no /oath/INDEX.md)");
        exit(1);
    }
    if let Action::Publish { local, force } = action {
        if let Err(e) = publish::publish(local, force) {
            eprintln!("cargo make oath publish: {e}");
            exit(1);
        }
        return;
    }
    let only = match &action {
        Action::Apply { only, .. } => only.as_slice(),
        _ => &[],
    };
    let staged = stage::stage(release, only);
    if let Action::Apply { force, .. } = action
        && let Err(e) = apply(&staged, force)
    {
        eprintln!("cargo make oath apply: {e}");
        exit(1);
    }
}

fn apply(staged: &[Staged], force: bool) -> Result<(), String> {
    let me = provenance::here()?;
    if me.dirty {
        eprintln!("[oath] the tree is dirty: anything applied after this replaces it");
    }
    if !matches!(me.branch.as_str(), "master" | "next") {
        eprintln!(
            "[oath] applying from {}: whoever applies after you must merge it; \
             with other sessions working, apply from next (the oath-packages skill)",
            me.short()
        );
    }
    let mut stored: Vec<(&str, String)> = Vec::new();
    for s in staged {
        let hash = catalog::store(&s.pkg.id(), &s.dir)?;
        stored.push((s.pkg.name, hash));
    }
    let mut wants = Vec::new();
    for (s, (_, hash)) in staged.iter().zip(&stored) {
        let mut needs = Vec::new();
        for need in s.pkg.runtime_needs() {
            let need_hash = match stored.iter().find(|(n, _)| *n == need) {
                Some((_, h)) => h.clone(),
                None => catalog::live_hash(&format!("pkg:{need}"))?,
            };
            needs.push((format!("pkg:{need}"), need_hash));
        }
        wants.push(Want { id: s.pkg.id(), hash: hash.clone(), needs });
    }
    let solactl = Path::new(stage::STAGE_ROOT).join("sola/bin/solactl");
    pin(&wants, &me, force, &solactl)
}

/// A package at the tree and needs it is to be pinned to.
struct Want {
    id: String,
    hash: String,
    /// `(id, hash)`.
    needs: Vec<(String, String)>,
}

/// Pin each of `wants` whose tree or needs differ from what is live, with
/// `me` as its source, and `oath apply` them together; then tell the desk
/// through `solactl`. PID 1 restarts the services that run from a package
/// that moved. Refuses to take off this machine work this tree lacks,
/// unless `force`.
fn pin(wants: &[Want], me: &provenance::Source, force: bool, solactl: &Path) -> Result<(), String> {
    // What moves: a package whose tree or needs differ from what is live.
    let mut moves: Vec<(&Want, String)> = Vec::new();
    for w in wants {
        let was = catalog::live_hash(&w.id).unwrap_or_else(|_| "none".into());
        let mut live_needs = catalog::live_needs(&w.id).unwrap_or_default();
        live_needs.sort();
        let mut sorted = w.needs.clone();
        sorted.sort();
        if w.hash == was && sorted == live_needs {
            println!("[oath] {}: {} (unchanged)", w.id, short(&w.hash));
            continue;
        }
        moves.push((w, was));
    }
    if moves.is_empty() {
        println!("[oath] nothing changed; nothing applied");
        return Ok(());
    }
    let live: Vec<(String, String)> = moves.iter().map(|(w, was)| (w.id.clone(), was.clone())).collect();
    refuse_dropped(&live, me, force)?;
    for (w, was) in &moves {
        // A package this machine does not have yet (an app split out of
        // the core): made, and its /bin names pass to it in this apply.
        if was == "none" && !catalog::exists(&w.id) {
            catalog::create(&w.id, &w.hash, &w.needs, me)?;
        } else {
            catalog::set(&w.id, &w.hash, &w.needs, me)?;
        }
        println!("[oath] {}: {} -> {}", w.id, short(was), short(&w.hash));
    }
    let ids: Vec<String> = moves.iter().map(|(w, _)| w.id.clone()).collect();
    catalog::apply(&ids)?;
    println!("[oath] applied {} from {}", ids.join(" "), me.short());
    tell_desk(&ids, solactl);
    Ok(())
}

/// An error naming the live work among `live` (`(id, its live hash)`)
/// that this tree lacks, unless there is none, or `force` (then only
/// said).
fn refuse_dropped(live: &[(String, String)], me: &provenance::Source, force: bool) -> Result<(), String> {
    let dropped = dropped_work(live, me, provenance::live, provenance::has_commit);
    if dropped.is_empty() {
        return Ok(());
    }
    let msg = format!(
        "this tree lacks work that is live here:\n{}\n\
         merge it (git merge <commit>), or apply from next (the oath-packages skill); \
         --force drops it",
        dropped.join("\n")
    );
    if !force {
        return Err(msg);
    }
    eprintln!("[oath] --force: {msg}");
    Ok(())
}

/// Tell the running desk which packages moved (`Topic::PackagesMoved`):
/// the session manager starts again each app on an old program. Not when
/// `pkg:sola` moved — the bus and the session manager restart, and it
/// checks as it starts. Through the `solactl` just built (staged, or the
/// plan's), which knows the topic whatever the desk runs.
fn tell_desk(ids: &[String], solactl: &Path) {
    if ids.iter().any(|id| id == "pkg:sola") {
        return;
    }
    let payload = serde_json::to_string(ids).unwrap_or_else(|_| "[]".into());
    match std::process::Command::new(solactl)
        .args(["emit", "PackagesMoved", &payload])
        .status()
    {
        Ok(s) if s.success() => println!("[oath] told the desk: {payload}"),
        Ok(s) => eprintln!("[oath] telling the desk failed ({s}): reopen moved apps by hand"),
        Err(e) => eprintln!("[oath] {}: {e}: reopen moved apps by hand", solactl.display()),
    }
}

/// The packages about to move whose live pin this repo made at a commit
/// this tree lacks — work an apply would take off this machine. `live`:
/// `(id, its live hash)`; `source_of` knows where a live pin came from;
/// `has` says this tree's history holds a commit.
fn dropped_work(
    live: &[(String, String)],
    me: &provenance::Source,
    source_of: impl Fn(&str, &str) -> Option<provenance::Source>,
    has: impl Fn(&str) -> bool,
) -> Vec<String> {
    live.iter()
        .filter_map(|(id, was)| {
            let src = source_of(id, was)?;
            (provenance::same_repo(&src.repo, &me.repo) && !has(&src.commit))
                .then(|| format!("  {id} runs {} (built on {} at {})", src.short(), src.built_by, src.built_at))
        })
        .collect()
}

fn short(hash: &str) -> &str {
    let hex = hash.strip_prefix("sha256-").unwrap_or(hash);
    &hex[..hex.len().min(12)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use provenance::Source;

    fn src(repo: &str, commit: &str) -> Source {
        Source {
            repo: repo.into(),
            commit: commit.into(),
            branch: "zone-layout".into(),
            ..Source::default()
        }
    }

    #[test]
    fn an_apply_keeps_what_is_live() {
        let me = src("https://github.com/moonlight-pm/Sola", "mine");
        let live = vec![
            ("pkg:sola".to_string(), "h1".to_string()),
            ("pkg:sola-browser".to_string(), "h2".to_string()),
            ("pkg:sola-design".to_string(), "h3".to_string()),
        ];
        let source_of = |id: &str, _: &str| match id {
            // Another branch's work, not in this tree: refused.
            "pkg:sola" => Some(src("git@github.com:moonlight-pm/Sola.git", "theirs")),
            // In this tree's history: fine.
            "pkg:sola-browser" => Some(src("https://github.com/moonlight-pm/Sola", "old")),
            // No source (a release): fine.
            _ => None,
        };
        let has = |c: &str| c == "old" || c == "mine";
        let dropped = dropped_work(&live, &me, source_of, has);
        assert_eq!(dropped.len(), 1);
        assert!(dropped[0].contains("pkg:sola runs zone-layout@theirs"), "{dropped:?}");
    }

    #[test]
    fn another_repos_pin_is_not_ours_to_keep() {
        let me = src("https://github.com/moonlight-pm/Sola", "mine");
        let live = vec![("pkg:oath".to_string(), "h".to_string())];
        let dropped = dropped_work(
            &live,
            &me,
            |_, _| Some(src("https://github.com/moonlight-pm/Oath", "x")),
            |_| false,
        );
        assert!(dropped.is_empty());
    }
}
