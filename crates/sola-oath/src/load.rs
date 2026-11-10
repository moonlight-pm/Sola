//! Reading everything, off the UI thread. The catalog and the store come
//! first (local and quick); git, the audit and the origin's listing follow
//! as each answers, so a slow network never holds the window.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::audit::Report;
use crate::git::{self, Pushed};
use crate::model::{Inputs, Source};
use crate::origin::{self, Listing};
use crate::{heads, oath, when};

/// The catalog (`oath get` for every package and service, and the host),
/// the release record, the heads taken and the store (`oath reap`).
pub fn local() -> Inputs {
    let mut problems = Vec::new();
    let ids: Vec<String> = match oath::json(&["ls"]) {
        Ok(v) => v.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect(),
        Err(e) => {
            problems.push(e);
            Vec::new()
        }
    };
    let mut get = |id: &str| match oath::json(&["get", id]) {
        Ok(v) => Some(v),
        Err(e) => {
            problems.push(e);
            None
        }
    };
    let packages = ids.iter().filter(|i| i.starts_with("pkg:")).filter_map(|i| get(i)).collect();
    let services = ids.iter().filter(|i| i.starts_with("svc:")).filter_map(|i| get(i)).collect();
    let host = get("host:local");
    let file = oath::release_file();
    let release = match std::fs::read_to_string(&file) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| problems.push(format!("{}: {e}", file.display()))).ok(),
        Err(e) => {
            problems.push(format!("{}: {e}", file.display()));
            None
        }
    };
    let heads = heads::read().unwrap_or_else(|e| {
        problems.push(e);
        Default::default()
    });
    let reap = oath::json(&["reap"]).map_err(|e| problems.push(e)).ok();
    Inputs {
        packages,
        services,
        host,
        release,
        heads,
        last_apply: oath::last_apply(),
        reap,
        problems,
        read_at: when::format(when::now()),
    }
}

pub fn pushed(sources: Vec<Source>) -> BTreeMap<(String, String), Pushed> {
    sources
        .into_iter()
        .map(|s| {
            let p = git::pushed(&s.repo, &s.commit, s.dirty);
            ((s.repo, s.commit), p)
        })
        .collect()
}

/// The audit, its signature checked by Oath against `host:local.publishers`.
pub fn audit() -> Result<Report, String> {
    oath::json(&["audit"]).map(|v| Report::parse(&v))
}

pub fn listing(origin: &str) -> Result<Listing, String> {
    origin::fetch(origin).map(|v| origin::parse(origin, &when::format(when::now()), &v))
}
