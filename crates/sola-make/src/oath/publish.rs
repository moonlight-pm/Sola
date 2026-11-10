//! `cargo make oath publish`: build Sola's packages from plans in `oath
//! build`'s sandbox, put their plans and sources on Oath's default origin
//! (R13: a pack's source beside it), run them here, and publish each as
//! its head (Oath R72), signed with this machine's key. What goes out is
//! what this machine runs.
//!
//! 1. `pkg:sola-src`: the committed tree (`git archive HEAD`, so a clean
//!    tree only) at `src/sola`, the icon and cursor packs at `src/share`.
//! 2. `pkg:sola-crates`: `cargo vendor` of the lock.
//! 3. `plan:sola-build` compiles every package's programs once, against
//!    this machine's live packs (the base set and the build packs, with
//!    their run-time needs) and the two above.
//! 4. A plan per package copies its programs out of `pkg:sola-build`.
//!    `pkg:sola-browser` and `pkg:sola-design` need the `pkg:sola` just
//!    built.
//! 5. Upload `pkg/<name>/<hash>.tar` for the two sources and
//!    `plan/<name>/<hash>.plan` for every plan (skipping what the origin
//!    has). `pkg:sola-build` stays here, with no `pkg` object (`oath build
//!    --no-pkg`): it is only the other plans' input, and a rebuild runs
//!    its plan.
//! 6. Pin and apply the products here, as `apply` does, with its guard: a
//!    head is a pin its machine runs. A package that moves restarts its
//!    services (`pkg:sola`: the desk).
//! 7. `oath publish --confirm pkg:<name> --committed <its commit's time>`
//!    for each: the pack (if the origin lacks it), then `pkg/<name>.json`,
//!    signed with `~/.config/oath/sign_ed25519`. One whose head is that
//!    hash already stays as it is. A machine whose `host:local.publishers`
//!    trusts that key for `sola,sola-*` takes the heads at its next `oath
//!    update`; a head wins over the desk profile's pin.
//!
//! A package no machine has yet, or a new or changed service, still needs
//! Oath's desk profile: a machine reads heads only for the packages it
//! has. Tell the Oath session.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::catalog;
use super::origin::{self, Origin};
use super::packages::{BASE, PACKAGES, Package};
use super::plan::{self, BUILD, Plan};
use super::provenance;
use super::vendor;

const WORK: &str = "target/oath/publish";

/// What a build reads of the repo. A commit that touches nothing else (the
/// docs) leaves `pkg:sola-src` and every plan as they were: nothing rebuilds.
const SOURCES: &[&str] = &[".cargo", "Cargo.lock", "Cargo.toml", "cef-version", "crates"];

/// `local`: build the plans here; upload, pin and publish nothing.
/// `force`: pin over live work this tree lacks (as `apply --force`).
pub fn publish(local: bool, force: bool) -> Result<(), String> {
    let origin = if local { None } else { Some(Origin::from_env()?) };
    let client = crate::google_client::vars()?;
    if client.is_empty() && !local {
        return Err(format!(
            "no Google client in {}: a release without it cannot sign in to Google",
            crate::google_client::file().display()
        ));
    }
    if git_dirty()? {
        return Err("commit first: a published source is the committed tree".into());
    }
    let source = provenance::here()?;
    if !local {
        released_from_master()?;
        can_sign()?;
        // Before the build, not after it.
        let live: Vec<(String, String)> = PACKAGES
            .iter()
            .filter_map(|p| Some((p.id(), catalog::live_hash(&p.id()).ok()?)))
            .collect();
        super::refuse_dropped(&live, &source, force)?;
    }
    // Not the commit (see `stage`): an untouched package builds to the
    // hash it had.
    let version = env!("CARGO_PKG_VERSION");
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

    let mut plans: Vec<(&str, String)> = Vec::new();

    let mut build_packs: Vec<&str> = BASE.to_vec();
    build_packs.extend(PACKAGES.iter().flat_map(|p| p.build.iter().copied()));
    let mut build_needs = closure(build_packs.iter())?;
    build_needs.push(("pkg:sola-src".into(), src_hash.clone()));
    build_needs.push(("pkg:sola-crates".into(), crates_hash.clone()));
    let bins: Vec<&str> = PACKAGES.iter().flat_map(|p| p.bins.iter().copied()).collect();
    let build = Plan {
        name: BUILD.into(),
        build_needs,
        run_needs: Vec::new(),
        script: plan::build_script(&bins, &src_hash, &crates_hash, &version, &client),
    };
    let (plan_hash, build_hash) = file_and_build(work, &build, true)?;
    plans.push((BUILD, plan_hash));

    let mut products: BTreeMap<&str, String> = BTreeMap::new();
    let mut run_needs_of: BTreeMap<&str, Vec<(String, String)>> = BTreeMap::new();
    for pkg in PACKAGES {
        let mut build_needs = closure(BASE.iter())?;
        build_needs.push((format!("pkg:{BUILD}"), build_hash.clone()));
        if pkg.share {
            build_needs.push(("pkg:sola-src".into(), src_hash.clone()));
        }
        let run_needs = run_needs(pkg, &products)?;
        let plan = Plan {
            name: pkg.name.into(),
            build_needs,
            run_needs: run_needs.clone(),
            script: plan::package_script(pkg, &build_hash, &src_hash, &version),
        };
        let (plan_hash, product) = file_and_build(work, &plan, false)?;
        products.insert(pkg.name, product);
        plans.push((pkg.name, plan_hash));
        run_needs_of.insert(pkg.name, run_needs);
    }

    let Some(origin) = &origin else {
        println!("[publish] --local: nothing uploaded, applied or published");
        return Ok(());
    };
    let packs = [("sola-src", src_hash), ("sola-crates", crates_hash)];
    upload_all(origin, work, &packs, &plans)?;

    let new: Vec<String> = PACKAGES.iter().map(|p| p.id()).filter(|id| !catalog::exists(id)).collect();
    let wants: Vec<super::Want> = PACKAGES
        .iter()
        .map(|p| super::Want {
            id: p.id(),
            hash: products[p.name].clone(),
            needs: run_needs_of[p.name].clone(),
        })
        .collect();
    let solactl = Path::new("/oath/store/pkg/sola").join(&products["sola"]).join("bin/solactl");
    super::pin(&wants, &source, force, &solactl)?;
    heads(&products)?;
    if !new.is_empty() {
        println!(
            "\n[publish] new here: {}. A machine reads heads only for the packages it has: \
             ask the Oath session to add them to the desk profile",
            new.join(" ")
        );
    }
    Ok(())
}

/// `oath publish --confirm` each package's pin as its head, ordered by the
/// time its source commit was made. One whose head is that hash already
/// stays as it is. Then, when Oath asks, record the heads this machine
/// runs as the ones it took (Oath 0995609), once for them all.
fn heads(products: &BTreeMap<&str, String>) -> Result<(), String> {
    let mut published = Vec::new();
    let mut record = false;
    for pkg in PACKAGES {
        let id = pkg.id();
        let hash = &products[pkg.name];
        // The pin's own commit: a package that did not move keeps the
        // source it was pinned with.
        let commit = provenance::live(&id, hash).map(|s| s.commit).unwrap_or_else(|| "HEAD".into());
        let committed = commit_time(&commit)
            .or_else(|| commit_time("HEAD"))
            .ok_or("git: no commit time for HEAD")?;
        match catalog::publish_head(&id, &committed) {
            Ok(p) => {
                record |= p.record;
                if p.new {
                    println!("[publish] {id} {}: its head now", super::short(hash));
                    published.push(id);
                } else {
                    println!("[publish] {id} {}: its head already", super::short(hash));
                }
            }
            Err(e) if published.is_empty() => return Err(e),
            Err(e) => return Err(format!("{e}\n(heads published before it: {})", published.join(" "))),
        }
    }
    if record {
        let said = catalog::record_heads()?;
        println!("[publish] heads this machine runs recorded as taken: {said}");
    }
    Ok(())
}

/// When `commit` was made, in UTC with a `Z` (`oath publish --committed`:
/// heads are ordered by that text).
fn commit_time(commit: &str) -> Option<String> {
    let out = Command::new("git")
        .env("TZ", "UTC")
        .args(["show", "-s", "--date=format-local:%Y-%m-%dT%H:%M:%SZ", "--format=%cd", commit])
        .output()
        .ok()?;
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !t.is_empty()).then_some(t)
}

/// This machine can sign heads: its `oath` has `publish` (Oath R72) and
/// the signing key is there.
fn can_sign() -> Result<(), String> {
    let has_publish = Command::new("oath")
        .args(["publish", "--help"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !has_publish {
        return Err(
            "this machine's oath has no `publish` (Oath R72, in desk release bda181db1fc6): \
             it needs that update first, the owner's call"
                .into(),
        );
    }
    let key = origin::sign_key();
    if !key.is_file() {
        return Err(format!(
            "no signing key at {0}: ssh-keygen -t ed25519 -N '' -C oath@<host> -f {0}, \
             and the owner adds its .pub to each machine's host:local.publishers \
             as names=\"sola,sola-*\" ssh-ed25519 …",
            key.display()
        ));
    }
    Ok(())
}

/// Packs (`pkg/<name>/<hash>.tar`) and plans (`plan/<name>/<hash>.plan`).
fn upload_all(
    origin: &Origin,
    work: &Path,
    packs: &[(&str, String)],
    plans: &[(&str, String)],
) -> Result<(), String> {
    for (name, hash) in packs {
        upload_pack(origin, work, name, hash)?;
    }
    for (name, hash) in plans {
        let key = format!("plan/{name}/{hash}.plan");
        let file = PathBuf::from(format!("/oath/store/plan/{name}/{hash}/plan.plan"));
        upload(origin, &key, &file)?;
    }
    Ok(())
}

/// Write, `oath fmt`, file and build `plan`; returns `(plan hash, product)`.
/// `input`: the product is only other plans' build input, so no `pkg`
/// object is made for it.
fn file_and_build(work: &Path, plan: &Plan, input: bool) -> Result<(String, String), String> {
    let file = work.join(format!("{}.plan", plan.name));
    fs::write(&file, plan::text(plan)).map_err(|e| format!("{}: {e}", file.display()))?;
    catalog::fmt(&file)?;
    let plan_hash = catalog::set_plan(&plan.name, &file)?;
    println!(
        "[publish] plan:{} {plan_hash}; building (oath logs plan:{})",
        plan.name, plan.name
    );
    let product = catalog::build(&plan.name, input)?;
    println!("[publish] pkg:{} {product}", plan.name);
    Ok((plan_hash, product))
}

/// `src/sola` (the committed tree) and `src/share` (icons, cursors).
fn source_tree(dir: &Path) -> Result<(), String> {
    let sola = dir.join("src/sola");
    fs::create_dir_all(&sola).map_err(|e| format!("{}: {e}", sola.display()))?;
    sh(&format!(
        "git archive --format=tar HEAD {} | tar -x -C '{}'",
        SOURCES.join(" "),
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

/// `pkg`'s run-time needs at the hashes just built (Sola's own) or live
/// here.
fn run_needs(pkg: &Package, built: &BTreeMap<&str, String>) -> Result<Vec<(String, String)>, String> {
    pkg.runtime_needs()
        .into_iter()
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

/// A release comes from master only: this checkout on master, and master
/// pushed (`HEAD == origin/master`), so every machine's packages are work
/// everyone has (the `oath-packages` skill).
fn released_from_master() -> Result<(), String> {
    let out = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let branch = out(&["branch", "--show-current"]).unwrap_or_default();
    if branch != "master" {
        return Err(format!(
            "publish from master (this is {}): merge the work to master and push it first",
            if branch.is_empty() { "a detached HEAD" } else { &branch }
        ));
    }
    let head = out(&["rev-parse", "HEAD"]).unwrap_or_default();
    let pushed = out(&["rev-parse", "origin/master"]).unwrap_or_default();
    if head != pushed {
        return Err("push master first: a release is what origin/master has".into());
    }
    Ok(())
}

fn git_dirty() -> Result<bool, String> {
    let out = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .map_err(|e| format!("git: {e}"))
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    Ok(!out(&["status", "--porcelain"])?.is_empty())
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
