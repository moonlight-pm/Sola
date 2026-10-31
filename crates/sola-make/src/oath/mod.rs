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
//!   drops that work). Only with the owner's go-ahead (`oath-packages`).
//! - `publish` builds each package from a plan in `oath build`'s sandbox
//!   and uploads packs, plans and sources to Oath's origin
//!   ([`publish`]). Outward-facing: only with the owner's go-ahead.

mod catalog;
mod origin;
mod packages;
mod plan;
mod provenance;
mod publish;
mod stage;
mod vendor;

use std::process::exit;

use stage::Staged;

pub enum Action {
    Stage,
    /// `true`: pin over live work this tree lacks.
    Apply { force: bool },
    /// `true`: build the plans here, upload nothing.
    Publish { local: bool },
}

pub fn run(action: Action, release: bool) {
    if !catalog::on_oath() {
        eprintln!("cargo make oath: not an Oath machine (no /oath/INDEX.md)");
        exit(1);
    }
    if let Action::Publish { local } = action {
        if let Err(e) = publish::publish(local) {
            eprintln!("cargo make oath publish: {e}");
            exit(1);
        }
        return;
    }
    let staged = stage::stage(release);
    if let Action::Apply { force } = action
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
    // What moves: a package whose tree or needs differ from what is live.
    let mut moves: Vec<(String, String, Vec<(String, String)>, String)> = Vec::new();
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
        let mut live_needs = catalog::live_needs(&id).unwrap_or_default();
        live_needs.sort();
        let mut sorted = needs.clone();
        sorted.sort();
        if *hash == was && sorted == live_needs {
            println!("[oath] {id}: {} (unchanged)", short(hash));
            continue;
        }
        moves.push((id, hash.clone(), needs, was));
    }
    if moves.is_empty() {
        println!("[oath] nothing changed; nothing applied");
        return Ok(());
    }
    let live: Vec<(String, String)> = moves.iter().map(|(id, _, _, was)| (id.clone(), was.clone())).collect();
    let dropped = dropped_work(&live, &me, provenance::live, provenance::has_commit);
    if !dropped.is_empty() {
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
    }
    for (id, hash, needs, was) in &moves {
        catalog::set(id, hash, needs)?;
        println!("[oath] {id}: {} -> {}", short(was), short(hash));
    }
    let ids: Vec<String> = moves.iter().map(|(id, ..)| id.clone()).collect();
    catalog::apply(&ids)?;
    let pins: Vec<(String, String)> = moves.iter().map(|(id, hash, ..)| (id.clone(), hash.clone())).collect();
    provenance::record(&pins, &me)?;
    println!("[oath] applied {} from {}", ids.join(" "), me.short());
    Ok(())
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
