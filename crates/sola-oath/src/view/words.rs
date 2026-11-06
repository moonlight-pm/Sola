//! What the app says about a package, in sentences, and how it writes
//! sizes and hashes.

use crate::audit::{Security, Upstream, rank};
use crate::git::Pushed;
use crate::model::{Ecosystem, Package};
use crate::standing::Standing;
use crate::when;

pub fn size(b: u64) -> String {
    match b {
        0 => "–".into(),
        _ if b >= 1_000_000_000 => format!("{:.1} GB", b as f64 / 1e9),
        _ if b >= 1_000_000 => format!("{} MB", (b as f64 / 1e6).round()),
        _ if b >= 1_000 => format!("{} KB", (b as f64 / 1e3).round()),
        _ => format!("{b} B"),
    }
}

/// The first 8 hex digits of `sha256-…`.
pub fn short(hash: &str) -> &str {
    let hex = hash.strip_prefix("sha256-").unwrap_or(hash);
    hex.get(..8).unwrap_or(hex)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The worst advisory's severity, in words.
fn worst(p: &Package) -> String {
    let hits = p.security.hits();
    let Some(h) = hits.iter().min_by_key(|h| rank(h)) else {
        return String::new();
    };
    if h.kev {
        return "one known to be exploited".into();
    }
    match h.severity.as_deref() {
        Some(s) => format!("the worst {}", s.to_lowercase()),
        None => "severity unknown".into(),
    }
}

/// Whether an update touches it, said plainly.
fn on_update(p: &Package) -> &'static str {
    if p.in_release() {
        "An update replaces it with the release's build"
    } else {
        "No release carries it, so an update leaves it alone"
    }
}

/// `(tone, sentence)`: the verdict, first thing in the sheet.
pub fn verdict(e: &Ecosystem, p: &Package) -> (Tone, String) {
    let built = p.source.as_ref().map(|s| s.short()).unwrap_or_default();
    match p.standing {
        Standing::NotPushed => {
            let what = match (&p.source, &p.pushed) {
                (Some(s), _) if s.dirty => format!(
                    "A local build from {built} with uncommitted changes: that work is in no commit, and this is its only copy."
                ),
                _ => format!("A local build from {built}, and no remote has that commit: this is the only copy of that work."),
            };
            return (Tone::Warn, format!("{what} {}.", on_update(p)));
        }
        _ if p.advised() => {
            let n = p.security.hits().len();
            let above = e.graph.above(&p.name).len();
            let mut s = format!(
                "{} on this build, {}, reaching {} above it.",
                plural(n, "advisory", "advisories"),
                worst(p),
                plural(above, "package", "packages"),
            );
            if let Some(Upstream::Newer(v)) = &p.upstream {
                s.push_str(&format!(" Upstream is at {v}."));
            }
            return (Tone::Danger, s);
        }
        _ => {}
    }
    let s = match p.standing {
        Standing::Local => match (&p.source, &p.pushed) {
            (Some(_), Some(Pushed::Yes(branch))) => {
                format!("A local build from {built}, which {branch} has. {}, and nothing is lost.", on_update(p))
            }
            (Some(_), Some(Pushed::Unknown(why))) => {
                format!("A local build from {built}. Whether it is pushed is unknown: {why}. {}.", on_update(p))
            }
            (Some(_), _) => format!("A local build from {built}. {}.", on_update(p)),
            (None, _) => format!(
                "Not the release's build: {} here where the release pins {}, and Oath has no record of where it came from.",
                short(&p.hash),
                short(p.release.as_deref().unwrap_or_default()),
            ),
        },
        Standing::Update => {
            let to = p.newest.as_ref().map(|m| {
                let v = if m.to_version.is_empty() || m.to_version == p.version {
                    String::new()
                } else {
                    format!(" ({})", m.to_version)
                };
                format!("{}{v}", short(&m.to_hash))
            });
            format!(
                "The release this machine runs pins {}. The newest release moves it to {}.",
                short(p.release.as_deref().unwrap_or_default()),
                to.unwrap_or_default()
            )
        }
        Standing::Off => {
            if p.versions.is_empty() {
                "In the catalog but not linked into /bin. No tree on disk: turning it on builds or fetches it first.".into()
            } else {
                "In the catalog but not linked into /bin. Its tree is on disk, so turning it on needs no network.".into()
            }
        }
        Standing::OriginOnly => {
            if p.listed.as_ref().is_some_and(|l| l.recipe) {
                "A recipe on the origin: a plan that fetches the vendor's own build. Not in this machine's catalog.".into()
            } else {
                "On the origin, and not in this machine's catalog.".into()
            }
        }
        _ if !p.in_release() => "Added on this machine. No release carries it, so an update leaves it alone.".into(),
        _ => {
            let mut s = match &e.check {
                Some(c) if c.error.is_none() && !p.moves() => {
                    format!("The release's build, and no newer release is out (checked {}).", when::ago(&c.at))
                }
                _ => "The release's build. A newer release has not been checked for.".into(),
            };
            if let Some(Upstream::Newer(v)) = &p.upstream {
                s.push_str(&format!(" Upstream is at {v}, which no release has taken yet."));
            }
            s
        }
    };
    (Tone::Plain, s)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Warn,
    Danger,
}

/// The Security section's one-word summary.
pub fn security_short(p: &Package) -> String {
    match &p.security {
        Security::Assessed { hits, .. } if !hits.is_empty() => plural(hits.len(), "advisory", "advisories"),
        Security::Assessed { .. } => "none listed".into(),
        Security::OtherVersion(_) => "not assessed".into(),
        Security::Skipped(_) => "not audited".into(),
        Security::Source => "a source tree".into(),
        Security::Absent => "not in the report".into(),
        Security::NoReport => "no report read".into(),
    }
}

/// The opening line of the summary: what this machine runs, and what is
/// worth a look.
pub fn summary(e: &Ecosystem) -> Vec<(Part, String)> {
    let mut out = Vec::new();
    let local = e.count(|p| p.standing == Standing::Local);
    let unpushed = e.count(|p| p.standing == Standing::NotPushed);
    match &e.release {
        Some(r) => {
            out.push((Part::Quiet, "Runs release ".into()));
            out.push((Part::Mono, r.output.clone()));
        }
        None => out.push((Part::Quiet, "No release record".into())),
    }
    match (local, unpushed) {
        (0, 0) => out.push((Part::Quiet, " as published, with no local builds".into())),
        (l, 0) => {
            out.push((Part::Quiet, " with ".into()));
            out.push((Part::Warn, plural(l, "local build", "local builds")));
        }
        (l, u) => {
            out.push((Part::Quiet, " with ".into()));
            if l > 0 {
                out.push((Part::Warn, plural(l, "local build", "local builds")));
                out.push((Part::Quiet, ", and ".into()));
            }
            out.push((Part::Warn, format!("{} not pushed", plural(u, "build", "builds"))));
        }
    }
    out.push((Part::Quiet, ". ".into()));
    match &e.check {
        None => out.push((Part::Quiet, "Not checked for a newer release.".into())),
        Some(c) if c.error.is_some() => {
            out.push((Part::Quiet, format!("The check for a newer release, {}, could not say.", when::ago(&c.at))))
        }
        Some(c) => {
            let n = e.moving();
            match (&c.newest, n) {
                (Some(id), n) if n > 0 && Some(id) != e.release.as_ref().map(|r| &r.output) => {
                    out.push((Part::Accent, format!("Release {id} is out and moves {}", plural(n, "package", "packages"))));
                    let mut more = Vec::new();
                    if !c.made.is_empty() {
                        more.push(format!("brings {} new", c.made.len()));
                    }
                    if !c.gone.is_empty() {
                        more.push(format!("takes {} away", plural(c.gone.len(), "package", "packages")));
                    }
                    if c.reboot {
                        more.push("needs a reboot".to_string());
                    }
                    if !more.is_empty() {
                        out.push((Part::Accent, format!(", {}", more.join(", "))));
                    }
                    out.push((Part::Quiet, format!(" (checked {}).", when::ago(&c.at))));
                }
                _ => out.push((Part::Quiet, format!("No newer release is out (checked {}).", when::ago(&c.at)))),
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Quiet,
    Mono,
    Warn,
    Accent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_like_the_design() {
        assert_eq!(size(0), "–");
        assert_eq!(size(1_888_569), "2 MB");
        assert_eq!(size(10_327_252_269), "10.3 GB");
        assert_eq!(size(512), "512 B");
    }

    #[test]
    fn short_hashes() {
        assert_eq!(short("sha256-d6e18a444abb488b"), "d6e18a44");
        assert_eq!(short("abc"), "abc");
    }
}
