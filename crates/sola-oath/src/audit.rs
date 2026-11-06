//! The daily security audit (`oath --json audit`): Oath's report, signed
//! on the build host, of what the advisory feeds say about each package of
//! the tree it scanned. Its findings hold for the version it read; a build
//! at another version was not assessed, which is not the same as clean.

use serde_json::Value;

/// What the report says about one package here.
#[derive(Debug, Clone, PartialEq)]
pub enum Security {
    /// The report read this version. `same_build`: the very hash, not
    /// only the version.
    Assessed {
        hits: Vec<Hit>,
        /// Advisories Oath looked at and set aside, each with why.
        suppressed: Vec<(String, String)>,
        same_build: bool,
    },
    /// The report read another version.
    OtherVersion(String),
    /// The audit leaves it out on purpose, with why (fonts, firmware).
    Skipped(String),
    /// A source tree: nothing runs from it.
    Source,
    /// Not in the report.
    Absent,
    /// No report was read.
    NoReport,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub id: String,
    pub severity: Option<String>,
    /// `affected`, or `unknown` when the feeds cannot tell.
    pub status: String,
    /// In CISA's list of advisories known to be exploited.
    pub kev: bool,
    /// The chance of exploitation in the next 30 days (EPSS).
    pub epss: Option<f64>,
    pub sources: Vec<String>,
    pub summary: String,
    pub url: String,
}

impl Security {
    pub fn hits(&self) -> &[Hit] {
        match self {
            Security::Assessed { hits, .. } => hits,
            _ => &[],
        }
    }

    pub fn assessed(&self) -> bool {
        matches!(self, Security::Assessed { .. })
    }
}

/// The report's own facts: when, which tree, who signed, how each feed did.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub generated: String,
    pub commit: String,
    pub signer: String,
    pub origin: String,
    /// `(feed, "ok …" or what went wrong)`.
    pub feeds: Vec<(String, String)>,
    entries: Vec<Value>,
}

/// One package's upstream: a newer release than the report read, or why it
/// could not tell.
#[derive(Debug, Clone, PartialEq)]
pub enum Upstream {
    Newer(String),
    Unread(String),
    Current,
}

impl Report {
    pub fn parse(v: &Value) -> Self {
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let feeds = v
            .get("sources")
            .and_then(Value::as_object)
            .map(|o| {
                o.iter()
                    .map(|(k, x)| (k.clone(), x.as_str().unwrap_or("?").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Report {
            generated: s("generated"),
            commit: s("commit"),
            signer: s("signer"),
            origin: s("origin"),
            feeds,
            entries: v.get("packages").and_then(Value::as_array).cloned().unwrap_or_default(),
        }
    }

    /// Feeds that did not answer `ok`: their findings are missing.
    pub fn failed_feeds(&self) -> Vec<&(String, String)> {
        self.feeds.iter().filter(|(_, s)| !s.starts_with("ok")).collect()
    }

    fn entry(&self, name: &str) -> Option<&Value> {
        let find = |n: &str| self.entries.iter().find(|e| e.get("name").and_then(Value::as_str) == Some(n));
        find(name).or_else(|| {
            // An -i386 build follows its sibling, but for the C library,
            // which has its own entry.
            let sibling = name.strip_suffix("-i386").filter(|_| name != "glibc-i386")?;
            find(sibling)
        })
    }

    /// What the report says of `name` at `version` / `hash`.
    pub fn security(&self, name: &str, version: &str, hash: &str) -> Security {
        if name.ends_with("-src") {
            return Security::Source;
        }
        let Some(e) = self.entry(name) else {
            return Security::Absent;
        };
        if let Some(why) = e.get("skip").and_then(Value::as_str) {
            return Security::Skipped(why.to_string());
        }
        let e_version = e.get("version").and_then(Value::as_str).unwrap_or_default();
        let e_hash = e.get("hash").and_then(Value::as_str).unwrap_or_default();
        // The sibling's hash is never ours; its version is.
        let own = e.get("name").and_then(Value::as_str) == Some(name);
        let same_build = own && !hash.is_empty() && e_hash == hash;
        if !same_build && e_version != version {
            return Security::OtherVersion(e_version.to_string());
        }
        let suppressed: Vec<(String, String)> = e
            .get("suppressed")
            .and_then(Value::as_object)
            .map(|o| o.iter().map(|(k, x)| (k.clone(), x.as_str().unwrap_or_default().to_string())).collect())
            .unwrap_or_default();
        let hits = e
            .get("hits")
            .and_then(Value::as_array)
            .map(|hs| hs.iter().map(parse_hit).filter(|h| !suppressed.iter().any(|(id, _)| *id == h.id)).collect())
            .unwrap_or_default();
        Security::Assessed { hits, suppressed, same_build }
    }

    /// A newer upstream release than the version the report read.
    pub fn upstream(&self, name: &str) -> Option<Upstream> {
        let e = self.entry(name)?;
        if let Some(v) = e.get("newer").and_then(Value::as_str) {
            return Some(Upstream::Newer(v.to_string()));
        }
        if let Some(why) = e.get("upstream_unread").and_then(Value::as_str) {
            return Some(Upstream::Unread(why.to_string()));
        }
        if e.get("skip").is_some() {
            return None;
        }
        Some(Upstream::Current)
    }
}

fn parse_hit(h: &Value) -> Hit {
    let s = |k: &str| h.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    Hit {
        id: s("id"),
        severity: h.get("severity").and_then(Value::as_str).map(str::to_string),
        status: s("status"),
        kev: h.get("kev").and_then(Value::as_bool).unwrap_or(false),
        epss: h.get("epss").and_then(Value::as_f64),
        sources: h
            .get("sources")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default(),
        summary: s("summary"),
        url: s("url"),
    }
}

/// Worst first: known exploited, then by severity.
pub fn rank(h: &Hit) -> u8 {
    if h.kev {
        return 0;
    }
    match h.severity.as_deref() {
        Some("CRITICAL") => 1,
        Some("HIGH") => 2,
        Some("MEDIUM") => 3,
        Some("LOW") => 4,
        _ => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report() -> Report {
        Report::parse(&json!({
            "generated": "2026-11-05T11:45:28Z",
            "commit": "ffa89c65c5",
            "signer": "oath@buccina",
            "sources": {"NVD": "ok, 138 CPEs", "OSV": "timed out"},
            "packages": [
                {"name": "bison", "version": "3.8.2", "hash": "sha256-b",
                 "hits": [{"id": "CVE-1", "severity": "MEDIUM", "status": "affected", "kev": false,
                           "sources": ["NVD"], "summary": "s", "url": "u"},
                          {"id": "CVE-2", "status": "affected", "kev": false, "sources": [], "summary": "", "url": ""}],
                 "suppressed": {"CVE-2": "patched"}},
                {"name": "mesa", "version": "26.2.5", "hash": "sha256-m", "hits": [], "newer": "26.2.6"},
                {"name": "font-noto", "version": "1", "hash": "x", "hits": [], "skip": "fonts"},
                {"name": "glibc-i386", "version": "2.42", "hash": "g", "hits": []}
            ]
        }))
    }

    #[test]
    fn suppressed_advisories_are_set_aside() {
        let s = report().security("bison", "3.8.2", "sha256-b");
        let Security::Assessed { hits, suppressed, same_build } = s else { panic!() };
        assert_eq!(hits.len(), 1);
        assert_eq!(suppressed, [("CVE-2".to_string(), "patched".to_string())]);
        assert!(same_build);
    }

    #[test]
    fn another_version_is_not_assessed() {
        assert_eq!(report().security("bison", "3.8.3", "sha256-z"), Security::OtherVersion("3.8.2".into()));
    }

    #[test]
    fn same_version_another_build_still_counts() {
        let s = report().security("bison", "3.8.2", "sha256-other");
        assert!(matches!(s, Security::Assessed { same_build: false, .. }));
    }

    #[test]
    fn i386_follows_its_sibling_but_glibc() {
        assert!(report().security("mesa-i386", "26.2.5", "sha256-q").assessed());
        assert!(report().security("glibc-i386", "2.42", "g").assessed());
        assert_eq!(report().security("zlib-i386", "1", "z"), Security::Absent);
    }

    #[test]
    fn sources_and_skips() {
        assert_eq!(report().security("codex-src", "", ""), Security::Source);
        assert_eq!(report().security("font-noto", "1", "x"), Security::Skipped("fonts".into()));
    }

    #[test]
    fn upstream_and_failed_feeds() {
        let r = report();
        assert_eq!(r.upstream("mesa"), Some(Upstream::Newer("26.2.6".into())));
        assert_eq!(r.upstream("bison"), Some(Upstream::Current));
        assert_eq!(r.upstream("font-noto"), None);
        assert_eq!(r.failed_feeds().len(), 1);
    }
}
