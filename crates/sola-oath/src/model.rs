//! The package ecosystem as the app shows it: every package here and on
//! the origin, each with where it stands, what it needs and what uses it,
//! what it weighs, and what the audit says. Built from what Oath printed;
//! any part may be missing, and then the app says so instead of guessing.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::audit::{Report, Security, Upstream};
use crate::check::{Check, Move};
use crate::git::Pushed;
use crate::graph::Graph;
use crate::origin::{Listed, Listing};
use crate::standing::Standing;

/// Oath's categories, from the ground up: the order of the map's columns.
pub const CATEGORIES: &[&str] = &[
    "kernel", "base", "toolchain", "security", "network", "text", "graphics", "sound", "x11", "desk", "browser",
    "games", "agents", "machines", "server",
];
pub const UNCATEGORIZED: &str = "uncategorized";

/// Where a build came from (`pkg.source`).
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub repo: String,
    pub commit: String,
    pub branch: String,
    pub dirty: bool,
    pub built_by: String,
    pub built_at: String,
}

impl Source {
    fn parse(v: &Value) -> Option<Self> {
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let src = Source {
            repo: s("repo"),
            commit: s("commit"),
            branch: s("branch"),
            dirty: v.get("dirty").and_then(Value::as_bool).unwrap_or(false),
            built_by: s("built_by"),
            built_at: s("built_at"),
        };
        (!src.repo.is_empty() || !src.commit.is_empty()).then_some(src)
    }

    /// `master@712b942d`.
    pub fn short(&self) -> String {
        let branch = if self.branch.is_empty() { "detached" } else { &self.branch };
        format!("{branch}@{}", self.commit.get(..8).unwrap_or(&self.commit))
    }
}

/// A version in the store, as `oath reap` sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct OnDisk {
    pub hash: String,
    pub arrived: String,
    pub size: Option<u64>,
    /// What keeps it (`live`, `release`, `pin`, `needs:pkg:x`,
    /// `generation:n`, …); none means the next reap removes it.
    pub held: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Service {
    pub id: String,
    pub seat: Option<String>,
    pub state: String,
}

#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub description: String,
    pub home: String,
    pub category: String,
    /// In this machine's catalog (otherwise only on the origin).
    pub here: bool,
    /// Linked into /bin.
    pub present: bool,
    pub hash: String,
    pub removable: bool,
    pub links: Vec<String>,
    pub source: Option<Source>,
    pub verified: Option<(String, bool)>,
    /// What the release this machine runs pins, if it carries it.
    pub release: Option<String>,
    /// The newest release's move, when a check found one.
    pub newest: Option<Move>,
    pub versions: Vec<OnDisk>,
    pub services: Vec<Service>,
    pub security: Security,
    pub upstream: Option<Upstream>,
    pub pushed: Option<Pushed>,
    /// Oath's word for desired against actual (`in-sync`, `drift`, …).
    pub status: String,
    pub listed: Option<Listed>,
    /// `oath get`, whole.
    pub raw: Value,
    pub standing: Standing,
}

impl Package {
    /// The audit lists advisories on the version here. (One only on the
    /// origin shows its findings in its sheet, but is not marked.)
    pub fn advised(&self) -> bool {
        self.here && !self.security.hits().is_empty()
    }

    /// The release's build: no overlay, no addition.
    pub fn is_release_build(&self) -> bool {
        self.release.as_deref() == Some(self.hash.as_str())
    }

    pub fn in_release(&self) -> bool {
        self.release.is_some()
    }

    /// The build in use, in bytes.
    pub fn in_use(&self) -> u64 {
        if !self.present {
            return 0;
        }
        self.versions
            .iter()
            .find(|v| v.hash == self.hash)
            .and_then(|v| v.size)
            .or_else(|| self.listed.as_ref().and_then(|l| l.size))
            .unwrap_or(0)
    }

    /// Every version in the store.
    pub fn on_disk(&self) -> u64 {
        self.versions.iter().filter_map(|v| v.size).sum()
    }

    /// The build is newer in the newest release (not only its record).
    pub fn moves(&self) -> bool {
        self.newest.as_ref().is_some_and(|m| !m.record_only && !m.to_hash.is_empty() && m.to_hash != self.hash)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Release {
    pub output: String,
    pub published: String,
}

/// What Oath printed, before it is made sense of.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    /// `oath --json get pkg:*`.
    pub packages: Vec<Value>,
    /// `oath --json get svc:*`.
    pub services: Vec<Value>,
    pub host: Option<Value>,
    /// `/oath/output.json`.
    pub release: Option<Value>,
    /// `oath --json reap`.
    pub reap: Option<Value>,
    /// Whatever could not be read, in words.
    pub problems: Vec<String>,
    pub read_at: String,
}

impl Inputs {
    pub fn origin(&self) -> String {
        self.host
            .as_ref()
            .and_then(|h| h.pointer("/desired/origins/0").or_else(|| h.pointer("/actual/origins/0")))
            .and_then(Value::as_str)
            .unwrap_or("https://store.oath.moonlight.pm")
            .to_string()
    }

    /// The sources of builds that are not the release's: what git must be
    /// asked about.
    pub fn sources_to_check(&self) -> Vec<Source> {
        let pins = self.release.as_ref().and_then(|r| r.get("pins"));
        let mut out: Vec<Source> = Vec::new();
        for v in &self.packages {
            let name = id_name(v);
            let hash = live_hash(v);
            let pin = pins.and_then(|p| p.get(name)).and_then(Value::as_str);
            if pin == Some(hash.as_str()) {
                continue;
            }
            if let Some(src) = source_of(v)
                && !out.contains(&src)
            {
                out.push(src);
            }
        }
        out
    }
}

pub struct Ecosystem {
    pub host: String,
    pub profile: String,
    pub release: Option<Release>,
    pub pkgs: BTreeMap<String, Package>,
    pub graph: Graph,
    /// How many packages each one needs and is used by, however far.
    pub reach: BTreeMap<String, (usize, usize)>,
    /// The categories that hold a package, in [`CATEGORIES`] order.
    pub categories: Vec<String>,
    pub report: Option<Report>,
    pub listing: Option<(String, String)>,
    pub check: Option<Check>,
    pub problems: Vec<String>,
    pub read_at: String,
}

fn id_name(v: &Value) -> &str {
    let id = v.get("id").and_then(Value::as_str).unwrap_or_default();
    id.strip_prefix("pkg:").unwrap_or(id)
}

fn live_hash(v: &Value) -> String {
    v.pointer("/actual/hash")
        .or_else(|| v.pointer("/desired/hash"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn source_of(v: &Value) -> Option<Source> {
    v.pointer("/desired/source").or_else(|| v.pointer("/actual/source")).and_then(Source::parse)
}

fn strs(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// The version of the component named after the package (`cf` and `oath`
/// list others first), else the first.
fn version_of(name: &str, v: &Value) -> Option<String> {
    let comps = v.pointer("/actual/components").and_then(Value::as_array)?;
    let pick = comps
        .iter()
        .find(|c| c.get("name").and_then(Value::as_str) == Some(name))
        .or_else(|| comps.first())?;
    pick.get("version").and_then(Value::as_str).map(str::to_string)
}

impl Ecosystem {
    pub fn build(
        inputs: &Inputs,
        report: Option<&Report>,
        listing: Option<&Listing>,
        check: Option<&Check>,
        pushed: &BTreeMap<(String, String), Pushed>,
    ) -> Self {
        let host = inputs
            .host
            .as_ref()
            .and_then(|h| h.pointer("/desired/hostname").or_else(|| h.pointer("/actual/hostname")))
            .and_then(Value::as_str)
            .unwrap_or("this machine")
            .to_string();
        let rel = inputs.release.as_ref();
        let profile = inputs
            .host
            .as_ref()
            .and_then(|h| h.pointer("/desired/profile"))
            .or_else(|| rel.and_then(|r| r.get("profile")))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let release = rel.map(|r| {
            let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
            Release { output: s("output"), published: s("published") }
        });
        let pins = rel.and_then(|r| r.get("pins"));
        let packs = rel.and_then(|r| r.get("packs"));

        let mut on_disk: BTreeMap<String, Vec<OnDisk>> = BTreeMap::new();
        for v in inputs.reap.as_ref().and_then(|r| r.get("versions")).and_then(Value::as_array).into_iter().flatten() {
            let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
            let name = id_name(v).to_string();
            on_disk.entry(name).or_default().push(OnDisk {
                hash: s("hash"),
                arrived: s("arrived"),
                size: v.get("size").and_then(Value::as_u64),
                held: strs(v.get("held")),
            });
        }
        for vs in on_disk.values_mut() {
            vs.sort_by(|a, b| b.arrived.cmp(&a.arrived));
        }

        let mut services: BTreeMap<String, Vec<Service>> = BTreeMap::new();
        for v in &inputs.services {
            let Some(pkg) = v.pointer("/actual/pkg/id").and_then(Value::as_str) else {
                continue;
            };
            services.entry(pkg.strip_prefix("pkg:").unwrap_or(pkg).to_string()).or_default().push(Service {
                id: v.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
                seat: v.pointer("/desired/seat").and_then(Value::as_str).map(str::to_string),
                state: v.pointer("/actual/state").and_then(Value::as_str).unwrap_or_default().to_string(),
            });
        }

        let listed: BTreeMap<&str, &Listed> =
            listing.map(|l| l.items.iter().map(|i| (i.name.as_str(), i)).collect()).unwrap_or_default();

        let mut pkgs: BTreeMap<String, Package> = BTreeMap::new();
        let mut needs: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for v in &inputs.packages {
            let name = id_name(v).to_string();
            if name.is_empty() {
                continue;
            }
            let d = |k: &str| v.pointer(&format!("/desired/{k}")).or_else(|| v.pointer(&format!("/actual/{k}")));
            let s = |k: &str| d(k).and_then(Value::as_str).unwrap_or_default().to_string();
            let hash = live_hash(v);
            let listed = listed.get(name.as_str()).map(|l| (*l).clone());
            let version = version_of(&name, v)
                .or_else(|| packs.and_then(|p| p.pointer(&format!("/{name}/version"))).and_then(Value::as_str).map(str::to_string))
                .or_else(|| listed.as_ref().map(|l| l.version.clone()).filter(|s| !s.is_empty()))
                .unwrap_or_default();
            let category = d("category")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| listed.as_ref().and_then(|l| l.category.clone()))
                .unwrap_or_else(|| UNCATEGORIZED.to_string());
            let listed_needs: Vec<String> = v
                .pointer("/desired/needs")
                .or_else(|| v.pointer("/actual/needs"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|n| n.get("id").and_then(Value::as_str).or_else(|| n.as_str()))
                .map(|id| id.strip_prefix("pkg:").unwrap_or(id).to_string())
                .collect();
            needs.insert(name.clone(), listed_needs);
            let verified = v.pointer("/actual/verified").map(|x| {
                (
                    x.get("at").and_then(Value::as_str).unwrap_or_default().to_string(),
                    x.get("ok").and_then(Value::as_bool).unwrap_or(false),
                )
            });
            let security = match report {
                Some(r) => r.security(&name, &version, &hash),
                None => Security::NoReport,
            };
            let source = source_of(v);
            let pkg = Package {
                version,
                description: s("description"),
                home: s("home"),
                category,
                here: true,
                present: d("present").and_then(Value::as_bool).unwrap_or(false),
                removable: v.pointer("/actual/removable").and_then(Value::as_bool).unwrap_or(true),
                links: strs(v.pointer("/actual/links")),
                pushed: source.as_ref().and_then(|s| pushed.get(&(s.repo.clone(), s.commit.clone())).cloned()),
                source,
                verified,
                release: pins.and_then(|p| p.get(&name)).and_then(Value::as_str).map(str::to_string),
                newest: check.and_then(|c| c.moves.get(&name).cloned()),
                versions: on_disk.remove(&name).unwrap_or_default(),
                services: services.remove(&name).unwrap_or_default(),
                upstream: report.and_then(|r| r.upstream(&name)),
                security,
                status: v.pointer("/meta/status").and_then(Value::as_str).unwrap_or_default().to_string(),
                listed,
                raw: v.clone(),
                standing: Standing::Current,
                hash,
                name: name.clone(),
            };
            pkgs.insert(name, pkg);
        }
        // What the origin has that this machine's catalog does not.
        if let Some(l) = listing {
            for item in &l.items {
                if pkgs.contains_key(&item.name) || item.name.ends_with("-src") {
                    continue;
                }
                needs.insert(item.name.clone(), Vec::new());
                pkgs.insert(
                    item.name.clone(),
                    Package {
                        name: item.name.clone(),
                        version: item.version.clone(),
                        description: String::new(),
                        home: String::new(),
                        category: item.category.clone().unwrap_or_else(|| UNCATEGORIZED.to_string()),
                        here: false,
                        present: false,
                        hash: String::new(),
                        removable: true,
                        links: Vec::new(),
                        source: None,
                        verified: None,
                        release: None,
                        newest: None,
                        versions: Vec::new(),
                        services: Vec::new(),
                        security: report.map_or(Security::NoReport, |r| r.security(&item.name, &item.version, "")),
                        upstream: report.and_then(|r| r.upstream(&item.name)),
                        pushed: None,
                        status: String::new(),
                        listed: Some(item.clone()),
                        raw: Value::Null,
                        standing: Standing::OriginOnly,
                    },
                );
            }
        }
        for p in pkgs.values_mut() {
            p.standing = standing_of(p);
        }
        let present: BTreeSet<&str> = pkgs.values().map(|p| p.category.as_str()).collect();
        let mut categories: Vec<String> =
            CATEGORIES.iter().filter(|c| present.contains(**c)).map(|c| c.to_string()).collect();
        categories.extend(present.iter().filter(|c| !CATEGORIES.contains(c)).map(|c| c.to_string()));
        let graph = Graph::new(&needs);
        let reach = pkgs.keys().map(|n| (n.clone(), (graph.below(n).len(), graph.above(n).len()))).collect();
        Ecosystem {
            host,
            profile,
            release,
            graph,
            reach,
            pkgs,
            categories,
            report: report.cloned(),
            listing: listing.map(|l| (l.origin.clone(), l.at.clone())),
            check: check.cloned(),
            problems: inputs.problems.clone(),
            read_at: inputs.read_at.clone(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Package> {
        self.pkgs.get(name)
    }

    /// `(below, above)`: everything it needs, everything that uses it.
    pub fn reach(&self, name: &str) -> (usize, usize) {
        self.reach.get(name).copied().unwrap_or_default()
    }

    pub fn count(&self, f: impl Fn(&Package) -> bool) -> usize {
        self.pkgs.values().filter(|p| f(p)).count()
    }

    /// What the newest release would move here, when a check said.
    pub fn moving(&self) -> usize {
        self.count(|p| p.here && p.moves())
    }
}

fn standing_of(p: &Package) -> Standing {
    if !p.here {
        return Standing::OriginOnly;
    }
    if !p.present {
        return Standing::Off;
    }
    if p.is_release_build() {
        return if p.moves() { Standing::Update } else { Standing::Current };
    }
    match (&p.source, &p.pushed) {
        (Some(s), _) if s.dirty => Standing::NotPushed,
        (Some(_), Some(Pushed::No | Pushed::Dirty)) => Standing::NotPushed,
        (Some(_), _) => Standing::Local,
        // An overlay with no record of where it came from.
        (None, _) if p.in_release() => Standing::Local,
        // Something this machine added that no release carries, made
        // here from a plan or taken from the origin.
        (None, _) => Standing::Current,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn get(name: &str, hash: &str, present: bool, needs: &[&str], source: Option<Value>) -> Value {
        let mut desired = json!({
            "present": present, "hash": hash, "category": "base",
            "needs": needs.iter().map(|n| json!({"id": format!("pkg:{n}"), "hash": "h"})).collect::<Vec<_>>(),
        });
        if let Some(s) = source {
            desired["source"] = s;
        }
        json!({
            "id": format!("pkg:{name}"),
            "desired": desired,
            "actual": {"present": present, "hash": hash, "removable": true,
                       "components": [{"name": "other", "version": "9"}, {"name": name, "version": "1.0"}]},
            "meta": {"status": "in-sync"}
        })
    }

    fn inputs() -> Inputs {
        Inputs {
            packages: vec![
                get("glibc", "g", true, &[], None),
                get("zlib", "z", true, &[], None),
                get("mesa", "m2", true, &["zlib"], Some(json!({"repo": "r", "commit": "c1", "branch": "next", "dirty": false}))),
                get("llama", "l", true, &[], Some(json!({"repo": "r", "commit": "c2", "branch": "master", "dirty": true}))),
                get("codex", "x", false, &[], None),
                get("cf", "cf", true, &[], None),
            ],
            release: Some(json!({"output": "f601", "pins": {"glibc": "g", "zlib": "z", "mesa": "m1", "codex": "x"},
                                 "packs": {}})),
            reap: Some(json!({"versions": [
                {"id": "pkg:mesa", "hash": "m2", "arrived": "2026-11-05T00:00:00Z", "size": 100, "held": ["live"]},
                {"id": "pkg:mesa", "hash": "m1", "arrived": "2026-11-01T00:00:00Z", "size": 90, "held": []}
            ]})),
            ..Inputs::default()
        }
    }

    #[test]
    fn standings() {
        let pushed = BTreeMap::from([(("r".to_string(), "c1".to_string()), Pushed::No)]);
        let e = Ecosystem::build(&inputs(), None, None, None, &pushed);
        assert_eq!(e.pkgs["glibc"].standing, Standing::Current);
        assert_eq!(e.pkgs["mesa"].standing, Standing::NotPushed);
        assert_eq!(e.pkgs["llama"].standing, Standing::NotPushed);
        assert_eq!(e.pkgs["codex"].standing, Standing::Off);
        assert_eq!(e.pkgs["cf"].standing, Standing::Current);
        assert!(!e.pkgs["cf"].in_release());
        let e = Ecosystem::build(&inputs(), None, None, None, &BTreeMap::new());
        assert_eq!(e.pkgs["mesa"].standing, Standing::Local);
    }

    #[test]
    fn sizes_and_versions() {
        let e = Ecosystem::build(&inputs(), None, None, None, &BTreeMap::new());
        let mesa = &e.pkgs["mesa"];
        assert_eq!(mesa.version, "1.0");
        assert_eq!(mesa.in_use(), 100);
        assert_eq!(mesa.on_disk(), 190);
        assert_eq!(e.graph.needs("mesa"), ["glibc", "zlib"]);
        assert_eq!(e.categories, ["base"]);
    }

    #[test]
    fn a_newer_release_and_the_origin() {
        let check = crate::check::parse(
            "t",
            &json!({"release": "05e5", "moves": [
                {"id": "pkg:zlib", "move": "pin", "from": {"hash": "z"}, "to": {"hash": "z2", "version": "1.4"}},
                {"id": "pkg:glibc", "move": "pin", "from": {"hash": "g"}, "to": {"hash": "g"}}
            ]}),
        );
        let listing = crate::origin::parse(
            "o",
            "t",
            &json!({"packages": [{"name": "zlib", "version": "1.0"}, {"name": "qemu", "version": "10", "category": "machines"}]}),
        );
        let e = Ecosystem::build(&inputs(), None, Some(&listing), Some(&check), &BTreeMap::new());
        assert_eq!(e.pkgs["zlib"].standing, Standing::Update);
        assert_eq!(e.pkgs["glibc"].standing, Standing::Current);
        assert_eq!(e.pkgs["qemu"].standing, Standing::OriginOnly);
        assert_eq!(e.moving(), 1);
        assert_eq!(e.categories, ["base", "machines"]);
    }

    #[test]
    fn only_builds_that_are_not_the_release_are_checked() {
        let srcs = inputs().sources_to_check();
        assert_eq!(srcs.iter().map(|s| s.commit.as_str()).collect::<Vec<_>>(), ["c1", "c2"]);
    }
}
