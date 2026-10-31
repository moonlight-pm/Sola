//! Runtime `pkg.needs` walk. Catalog apply enforces the same graph.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::catalog::Pkg;

pub fn by_id(pkgs: &[Pkg]) -> BTreeMap<String, &Pkg> {
    pkgs.iter().map(|p| (p.id.clone(), p)).collect()
}

pub fn present_now(pkg: &Pkg, staged: &BTreeMap<String, bool>) -> bool {
    staged.get(&pkg.id).copied().unwrap_or(pkg.desired_present)
}

/// Toggle `id` to `present`. Off also stages present reverse-deps.
/// On also stages missing needs. Does not revive dependents on toggle-on.
pub fn cascade(
    pkgs: &[Pkg],
    staged: &BTreeMap<String, bool>,
    id: &str,
    present: bool,
) -> Result<BTreeMap<String, bool>, String> {
    let map = by_id(pkgs);
    let Some(root) = map.get(id) else {
        return Err(format!("{id} is not in the catalog"));
    };
    if !root.removable && !present {
        return Err(format!("{id} is not removable"));
    }
    let mut out = staged.clone();
    if present {
        let mut q = VecDeque::from([id.to_string()]);
        let mut seen = BTreeSet::new();
        while let Some(cur) = q.pop_front() {
            if !seen.insert(cur.clone()) {
                continue;
            }
            let Some(pkg) = map.get(&cur) else {
                return Err(format!("{id} needs {cur}, which is not in the catalog"));
            };
            out.insert(cur.clone(), true);
            for n in &pkg.needs {
                q.push_back(n.id.clone());
            }
        }
    } else {
        let mut q = VecDeque::from([id.to_string()]);
        let mut seen = BTreeSet::new();
        while let Some(cur) = q.pop_front() {
            if !seen.insert(cur.clone()) {
                continue;
            }
            let Some(pkg) = map.get(&cur) else {
                continue;
            };
            if !pkg.removable {
                return Err(format!(
                    "turning off {id} would disable {cur}, which is not removable"
                ));
            }
            out.insert(cur.clone(), false);
            for (oid, other) in &map {
                if !present_now(other, &out) {
                    continue;
                }
                if other.needs.iter().any(|n| n.id == cur) {
                    q.push_back(oid.clone());
                }
            }
        }
    }
    // Drop entries that match desired — not staged.
    out.retain(|k, v| map.get(k).map(|p| p.desired_present != *v).unwrap_or(true));
    Ok(out)
}

/// Removable packs still present after `staged` that were only needed by
/// packs now staged off. Not auto-staged — the UI offers them.
pub fn unused_after(pkgs: &[Pkg], staged: &BTreeMap<String, bool>) -> Vec<String> {
    let map = by_id(pkgs);
    let remaining: BTreeSet<String> = pkgs
        .iter()
        .filter(|p| present_now(p, staged))
        .map(|p| p.id.clone())
        .collect();
    let mut still_needed = BTreeSet::new();
    for id in &remaining {
        if let Some(p) = map.get(id) {
            for n in &p.needs {
                still_needed.insert(n.id.clone());
            }
        }
    }
    let mut closure = BTreeSet::new();
    for (id, off) in staged {
        if *off {
            continue;
        }
        walk_need_ids(&map, id, &mut closure);
    }
    let mut out: Vec<String> = closure
        .into_iter()
        .filter(|id| {
            remaining.contains(id)
                && !still_needed.contains(id)
                && map.get(id).is_some_and(|p| p.removable)
        })
        .collect();
    out.sort();
    out
}

/// Packs that become unused if `extra_off` is also staged off.
pub fn would_free_if(
    pkgs: &[Pkg],
    staged: &BTreeMap<String, bool>,
    extra_off: &str,
) -> Vec<String> {
    let before = unused_after(pkgs, staged);
    let next = match cascade(pkgs, staged, extra_off, false) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    unused_after(pkgs, &next)
        .into_iter()
        .filter(|id| id != extra_off && !before.contains(id))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeRole {
    /// Operator staged this pack off.
    RootOff,
    /// Operator staged this pack on.
    RootOn,
    /// Used-by: will go off with the root.
    MustOff,
    /// Unused need: operator may also stage off.
    CanOff,
    /// Need that another remaining pack still requires.
    Stays,
    /// Need pulled in because the root is staging on.
    MustOn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeNode {
    pub id: String,
    pub role: ChangeRole,
    pub children: Vec<ChangeNode>,
}

/// Roots the operator started. Extra staged packs stay nested; they do not steal the tree.
pub fn change_forest(
    pkgs: &[Pkg],
    staged: &BTreeMap<String, bool>,
    anchors: &BTreeSet<String>,
) -> Vec<ChangeNode> {
    let mut roots: Vec<String> = anchors.iter().cloned().collect();
    if roots.is_empty() {
        roots = staged.keys().cloned().collect();
    }
    roots.sort();
    let mut out = Vec::new();
    for id in roots {
        let Some(&want) = staged.get(&id) else {
            continue;
        };
        if want {
            let mut children = Vec::new();
            if let Some(p) = pkgs.iter().find(|p| p.id == id) {
                for n in &p.needs {
                    let on = pkgs
                        .iter()
                        .find(|q| q.id == n.id)
                        .map(|q| present_now(q, staged))
                        .unwrap_or(false);
                    if !on {
                        children.push(ChangeNode {
                            id: n.id.clone(),
                            role: ChangeRole::MustOn,
                            children: Vec::new(),
                        });
                    }
                }
            }
            out.push(ChangeNode {
                id,
                role: ChangeRole::RootOn,
                children,
            });
            continue;
        }
        let mut children = Vec::new();
        let used = used_by_tree(pkgs, &id);
        let mut seen = BTreeSet::new();
        for (_, dep, _) in used {
            if !seen.insert(dep.clone()) {
                continue;
            }
            children.push(ChangeNode {
                id: dep,
                role: ChangeRole::MustOff,
                children: Vec::new(),
            });
        }
        let can = unused_after(pkgs, staged);
        let needs: Vec<String> = pkgs
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.needs.iter().map(|n| n.id.clone()).collect())
            .unwrap_or_default();
        for n in &needs {
            let optional = can.contains(n) || staged.get(n) == Some(&false);
            if optional {
                let nested = would_free_if(pkgs, staged, n);
                children.push(ChangeNode {
                    id: n.clone(),
                    role: ChangeRole::CanOff,
                    children: nested
                        .into_iter()
                        .map(|c| ChangeNode {
                            id: c,
                            role: ChangeRole::CanOff,
                            children: Vec::new(),
                        })
                        .collect(),
                });
            } else {
                children.push(ChangeNode {
                    id: n.clone(),
                    role: ChangeRole::Stays,
                    children: Vec::new(),
                });
            }
        }
        out.push(ChangeNode {
            id,
            role: ChangeRole::RootOff,
            children,
        });
    }
    out
}

fn walk_need_ids(map: &BTreeMap<String, &Pkg>, id: &str, out: &mut BTreeSet<String>) {
    let Some(pkg) = map.get(id) else {
        return;
    };
    for n in &pkg.needs {
        if out.insert(n.id.clone()) {
            walk_need_ids(map, &n.id, out);
        }
    }
}

pub fn needs_tree(pkgs: &[Pkg], id: &str) -> Vec<(u32, String, String)> {
    let map = by_id(pkgs);
    let mut rows = Vec::new();
    walk_needs(&map, id, 0, &mut BTreeSet::new(), &mut rows);
    rows
}

pub fn used_by_tree(pkgs: &[Pkg], id: &str) -> Vec<(u32, String, String)> {
    let map = by_id(pkgs);
    let mut rows = Vec::new();
    walk_used(&map, id, 0, &mut BTreeSet::new(), &mut rows);
    rows
}

fn walk_needs(
    map: &BTreeMap<String, &Pkg>,
    id: &str,
    depth: u32,
    path: &mut BTreeSet<String>,
    rows: &mut Vec<(u32, String, String)>,
) {
    let Some(pkg) = map.get(id) else {
        return;
    };
    if !path.insert(id.to_string()) {
        return;
    }
    for n in &pkg.needs {
        rows.push((depth, n.id.clone(), n.hash.clone()));
        walk_needs(map, &n.id, depth + 1, path, rows);
    }
    path.remove(id);
}

fn walk_used(
    map: &BTreeMap<String, &Pkg>,
    id: &str,
    depth: u32,
    path: &mut BTreeSet<String>,
    rows: &mut Vec<(u32, String, String)>,
) {
    if !path.insert(id.to_string()) {
        return;
    }
    let mut kids: Vec<String> = map
        .iter()
        .filter(|(_, p)| p.needs.iter().any(|n| n.id == id))
        .map(|(k, _)| k.clone())
        .collect();
    kids.sort();
    for k in kids {
        let hash = map
            .get(&k)
            .and_then(|p| p.needs.iter().find(|n| n.id == id))
            .map(|n| n.hash.clone())
            .unwrap_or_default();
        rows.push((depth, k.clone(), hash));
        walk_used(map, &k, depth + 1, path, rows);
    }
    path.remove(id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Pkg;

    fn pkg(id: &str, present: bool, removable: bool, needs: &[&str]) -> Pkg {
        Pkg {
            id: id.into(),
            desired_present: present,
            actual_present: present,
            removable,
            hash: String::new(),
            links: Vec::new(),
            status: String::new(),
            needs: needs
                .iter()
                .map(|s| crate::catalog::Need {
                    id: (*s).into(),
                    hash: String::new(),
                })
                .collect(),
            description: String::new(),
            home: String::new(),
            components: Vec::new(),
            advisories: None,
        }
    }

    #[test]
    fn off_mesa_stages_steam() {
        let pkgs = vec![
            pkg("pkg:glibc", true, false, &[]),
            pkg("pkg:mesa", true, true, &["pkg:glibc"]),
            pkg("pkg:steam", true, true, &["pkg:mesa"]),
        ];
        let staged = cascade(&pkgs, &BTreeMap::new(), "pkg:mesa", false).unwrap();
        assert_eq!(staged.get("pkg:mesa"), Some(&false));
        assert_eq!(staged.get("pkg:steam"), Some(&false));
        assert!(!staged.contains_key("pkg:glibc"));
    }

    #[test]
    fn on_steam_stages_mesa() {
        let pkgs = vec![
            pkg("pkg:glibc", true, false, &[]),
            pkg("pkg:mesa", false, true, &["pkg:glibc"]),
            pkg("pkg:steam", false, true, &["pkg:mesa", "pkg:glibc"]),
        ];
        let staged = cascade(&pkgs, &BTreeMap::new(), "pkg:steam", true).unwrap();
        assert_eq!(staged.get("pkg:steam"), Some(&true));
        assert_eq!(staged.get("pkg:mesa"), Some(&true));
        assert!(!staged.contains_key("pkg:glibc"));
    }

    #[test]
    fn off_does_not_disable_locked() {
        let pkgs = vec![
            pkg("pkg:glibc", true, false, &[]),
            pkg("pkg:sola", true, true, &["pkg:glibc"]),
        ];
        let err = cascade(&pkgs, &BTreeMap::new(), "pkg:glibc", false).unwrap_err();
        assert!(err.contains("not removable"), "{err}");
    }

    #[test]
    fn trees() {
        let pkgs = vec![
            pkg("pkg:glibc", true, false, &[]),
            pkg("pkg:mesa", true, true, &["pkg:glibc"]),
            pkg("pkg:steam", true, true, &["pkg:mesa"]),
        ];
        assert_eq!(
            needs_tree(&pkgs, "pkg:steam"),
            vec![
                (0, "pkg:mesa".into(), String::new()),
                (1, "pkg:glibc".into(), String::new())
            ]
        );
        assert_eq!(
            used_by_tree(&pkgs, "pkg:glibc"),
            vec![
                (0, "pkg:mesa".into(), String::new()),
                (1, "pkg:steam".into(), String::new())
            ]
        );
    }

    #[test]
    fn unused_after_steam_off_offers_mesa_not_glibc() {
        let pkgs = vec![
            pkg("pkg:glibc", true, false, &[]),
            pkg("pkg:mesa", true, true, &["pkg:glibc"]),
            pkg("pkg:steam", true, true, &["pkg:mesa"]),
        ];
        let staged = cascade(&pkgs, &BTreeMap::new(), "pkg:steam", false).unwrap();
        let unused = unused_after(&pkgs, &staged);
        assert_eq!(unused, vec!["pkg:mesa".to_string()]);
    }

    #[test]
    fn unused_skips_mesa_if_gamescope_still_on() {
        let pkgs = vec![
            pkg("pkg:glibc", true, false, &[]),
            pkg("pkg:mesa", true, true, &["pkg:glibc"]),
            pkg("pkg:steam", true, true, &["pkg:mesa"]),
            pkg("pkg:gamescope", true, true, &["pkg:mesa"]),
        ];
        let staged = cascade(&pkgs, &BTreeMap::new(), "pkg:steam", false).unwrap();
        let unused = unused_after(&pkgs, &staged);
        assert!(unused.is_empty(), "{unused:?}");
    }

    #[test]
    fn would_free_nested_need() {
        let pkgs = vec![
            pkg("pkg:glibc", true, false, &[]),
            pkg("pkg:foo", true, true, &["pkg:glibc"]),
            pkg("pkg:mesa", true, true, &["pkg:glibc", "pkg:foo"]),
            pkg("pkg:steam", true, true, &["pkg:mesa"]),
        ];
        let staged = cascade(&pkgs, &BTreeMap::new(), "pkg:steam", false).unwrap();
        let unused = unused_after(&pkgs, &staged);
        assert!(unused.contains(&"pkg:mesa".to_string()), "{unused:?}");
        let nested = would_free_if(&pkgs, &staged, "pkg:mesa");
        assert_eq!(nested, vec!["pkg:foo".to_string()]);
    }

    #[test]
    fn change_forest_steam_off() {
        let pkgs = vec![
            pkg("pkg:glibc", true, false, &[]),
            pkg("pkg:mesa", true, true, &["pkg:glibc"]),
            pkg("pkg:steam", true, true, &["pkg:mesa"]),
        ];
        let staged = cascade(&pkgs, &BTreeMap::new(), "pkg:steam", false).unwrap();
        let anchors = BTreeSet::from(["pkg:steam".to_string()]);
        let forest = change_forest(&pkgs, &staged, &anchors);
        assert_eq!(forest.len(), 1);
        assert_eq!(forest[0].role, ChangeRole::RootOff);
        let can: Vec<_> = forest[0]
            .children
            .iter()
            .filter(|c| c.role == ChangeRole::CanOff)
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(can, vec!["pkg:mesa"]);
        let stays: Vec<_> = forest[0]
            .children
            .iter()
            .filter(|c| c.role == ChangeRole::Stays)
            .map(|c| c.id.as_str())
            .collect();
        assert!(stays.is_empty(), "{stays:?}");
    }

    #[test]
    fn forest_keeps_steam_root_when_mesa_also_off() {
        let pkgs = vec![
            pkg("pkg:glibc", true, false, &[]),
            pkg("pkg:mesa", true, true, &["pkg:glibc"]),
            pkg("pkg:steam", true, true, &["pkg:mesa"]),
        ];
        let mut staged = cascade(&pkgs, &BTreeMap::new(), "pkg:steam", false).unwrap();
        staged = cascade(&pkgs, &staged, "pkg:mesa", false).unwrap();
        let anchors = BTreeSet::from(["pkg:steam".to_string()]);
        let forest = change_forest(&pkgs, &staged, &anchors);
        assert_eq!(forest.len(), 1);
        assert_eq!(forest[0].id, "pkg:steam");
        let can: Vec<_> = forest[0]
            .children
            .iter()
            .filter(|c| c.role == ChangeRole::CanOff)
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(can, vec!["pkg:mesa"]);
    }
}
