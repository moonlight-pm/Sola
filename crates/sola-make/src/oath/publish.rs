//! `cargo make oath publish`: build Sola's packages from plans in `oath
//! build`'s sandbox and put them, with their plans and sources, on Oath's
//! default origin, as Oath publishes its own (R13: a pack's source beside
//! it).
//!
//! 1. `pkg:sola-src`: the committed tree (`git archive HEAD`, so a clean
//!    tree only) at `src/sola`, the icon and cursor packs at `src/share`.
//! 2. `pkg:sola-crates`: `cargo vendor` of the lock.
//! 3. A plan per package, built against this machine's live packs (the
//!    base set and each build pack's run-time needs) and the two above.
//!    `pkg:sola-browser` and `pkg:sola-design` need the `pkg:sola` just
//!    built.
//! 4. Upload `pkg/<name>/<hash>.tar` and `plan/<name>/<hash>.plan` for
//!    the sources and products (skipping what the origin has), then print
//!    the desk profile entries that pin the products.
//!
//! The machine's own pins do not change: `oath build` leaves an existing
//! `pkg:<name>` as it is.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::catalog;
use super::origin::{self, Origin};
use super::packages::{BASE, PACKAGES, Package};
use super::plan::{self, Inputs};
use super::vendor;

const WORK: &str = "target/oath/publish";

pub fn publish() -> Result<(), String> {
    let origin = Origin::from_env()?;
    let (rev, dirty) = git_rev()?;
    if dirty {
        return Err("commit first: a published source is the committed tree".into());
    }
    let version = format!("{}+g{rev}", env!("CARGO_PKG_VERSION"));
    let work = Path::new(WORK);
    let _ = fs::remove_dir_all(work);

    let src_dir = work.join("sola-src");
    source_tree(&src_dir)?;
    let src_hash = catalog::store("pkg:sola-src", &src_dir)?;
    println!("[publish] pkg:sola-src {src_hash}");

    let crates_dir = work.join("sola-crates");
    fs::create_dir_all(&crates_dir).map_err(|e| format!("{}: {e}", crates_dir.display()))?;
    vendor::vendor(&crates_dir)?;
    let crates_hash = catalog::store("pkg:sola-crates", &crates_dir)?;
    println!("[publish] pkg:sola-crates {crates_hash}");

    let mut products: BTreeMap<&str, String> = BTreeMap::new();
    let mut plans: Vec<(&str, String)> = Vec::new();
    let mut run_needs_of: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
    for pkg in PACKAGES {
        let mut build_needs = closure(BASE.iter().chain(pkg.build))?;
        build_needs.push(("pkg:sola-src".into(), src_hash.clone()));
        build_needs.push(("pkg:sola-crates".into(), crates_hash.clone()));
        let run_needs = run_needs(pkg, &products)?;
        let text = plan::text(&Inputs {
            pkg,
            build_needs,
            run_needs: run_needs.clone(),
            src_hash: &src_hash,
            crates_hash: &crates_hash,
            version: &version,
        });
        let file = work.join(format!("{}.plan", pkg.name));
        fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
        catalog::fmt(&file)?;
        let plan_hash = catalog::set_plan(pkg.name, &file)?;
        println!("[publish] plan:{} {plan_hash}; building (oath logs plan:{})", pkg.name, pkg.name);
        let product = catalog::build(pkg.name)?;
        println!("[publish] pkg:{} {product}", pkg.name);
        products.insert(pkg.name, product);
        plans.push((pkg.name, plan_hash));
        run_needs_of.insert(pkg.name, run_needs);
    }

    let mut packs = vec![("sola-src", src_hash), ("sola-crates", crates_hash)];
    packs.extend(products.iter().map(|(n, h)| (*n, h.clone())));
    for (name, hash) in &packs {
        upload_pack(&origin, work, name, hash)?;
    }
    for (name, hash) in &plans {
        let key = format!("plan/{name}/{hash}.plan");
        let file = PathBuf::from(format!("/oath/store/plan/{name}/{hash}/plan.plan"));
        upload(&origin, &key, &file)?;
    }

    println!("\n[publish] desk profile entries (Oath's crates/oath-core/seed/desk.json):");
    for pkg in PACKAGES {
        let entry = serde_json::json!({
            "present": true,
            "hash": products[pkg.name],
            "needs": run_needs_of[pkg.name]
                .iter()
                .map(|(id, hash)| serde_json::json!({ "id": id, "hash": hash }))
                .collect::<Vec<_>>(),
        });
        println!("\"{}\": {entry},", pkg.id());
    }
    Ok(())
}

/// `src/sola` (the committed tree) and `src/share` (icons, cursors).
fn source_tree(dir: &Path) -> Result<(), String> {
    let sola = dir.join("src/sola");
    fs::create_dir_all(&sola).map_err(|e| format!("{}: {e}", sola.display()))?;
    sh(&format!(
        "git archive --format=tar HEAD | tar -x -C '{}'",
        sola.display()
    ))?;
    let share = dir.join("src/share");
    let cache = Path::new("target/oath/share-cache");
    crate::assets::sync_into(cache, false);
    fs::create_dir_all(share.join("icons")).map_err(|e| format!("{}: {e}", share.display()))?;
    sh(&format!("cp -a '{}/.' '{}'", cache.display(), share.display()))?;
    sh(&format!(
        "cp -a crates/sola-assets/icons/. '{}'",
        share.join("icons").display()
    ))
}

/// The ids and live hashes of `names` and everything they need at run
/// time, transitively.
fn closure<'a>(names: impl Iterator<Item = &'a &'a str>) -> Result<Vec<(String, String)>, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut queue: VecDeque<String> = names.map(|n| format!("pkg:{n}")).collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id.clone()) {
            continue;
        }
        out.insert(id.clone(), catalog::live_hash(&id)?);
        for (need, _) in catalog::live_needs(&id)? {
            queue.push_back(need);
        }
    }
    Ok(out.into_iter().collect())
}

/// `pkg.needs` at the hashes just built (Sola's own) or live here.
fn run_needs(pkg: &Package, built: &BTreeMap<&str, String>) -> Result<Vec<(String, String)>, String> {
    pkg.needs
        .iter()
        .map(|n| {
            let hash = match built.get(n) {
                Some(h) => h.clone(),
                None => catalog::live_hash(&format!("pkg:{n}"))?,
            };
            Ok((format!("pkg:{n}"), hash))
        })
        .collect()
}

/// A tar of the tree's contents, as Oath's origin serves packs.
fn upload_pack(origin: &Origin, work: &Path, name: &str, hash: &str) -> Result<(), String> {
    let key = format!("pkg/{name}/{hash}.tar");
    if origin::published(&key) {
        println!("[publish] {key} (already there)");
        return Ok(());
    }
    let tar = work.join(format!("{name}-{hash}.tar"));
    sh(&format!(
        "tar -C '/oath/store/pkg/{name}/{hash}' -cf '{}' .",
        tar.display()
    ))?;
    upload(origin, &key, &tar)?;
    let _ = fs::remove_file(&tar);
    Ok(())
}

fn upload(origin: &Origin, key: &str, file: &Path) -> Result<(), String> {
    if origin::published(key) {
        println!("[publish] {key} (already there)");
        return Ok(());
    }
    origin.put(key, file)?;
    if !origin::published(key) {
        return Err(format!("{key}: uploaded, but {} does not serve it", origin::PUBLIC));
    }
    println!("[publish] {key}");
    Ok(())
}

fn git_rev() -> Result<(String, bool), String> {
    let out = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .map_err(|e| format!("git: {e}"))
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let rev = out(&["rev-parse", "--short", "HEAD"])?;
    let dirty = !out(&["status", "--porcelain"])?.is_empty();
    Ok((rev, dirty))
}

fn sh(script: &str) -> Result<(), String> {
    let status = Command::new("sh")
        .args(["-c", script])
        .status()
        .map_err(|e| format!("sh: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("failed: {script}"))
    }
}
