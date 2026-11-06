//! Who needs whom, by package name. Oath lists a package's needs; it does
//! not list the C library every program stands on, so the graph adds that
//! one by rule (`glibc`, `glibc-i386` for an `-i386` package), as Oath
//! asked: sources (`-src`), fonts and the C library itself stand on
//! nothing.

use std::collections::{BTreeMap, BTreeSet};

pub struct Graph {
    needs: BTreeMap<String, Vec<String>>,
    /// The packages whose base the graph added by rule.
    implicit: BTreeSet<String>,
    users: BTreeMap<String, Vec<String>>,
    depth: BTreeMap<String, usize>,
}

/// The base `name` stands on without listing it, if any.
pub fn implicit_base(name: &str) -> Option<&'static str> {
    if name == "glibc" || name == "glibc-i386" || name.ends_with("-src") || name.starts_with("font-") {
        return None;
    }
    Some(if name.ends_with("-i386") { "glibc-i386" } else { "glibc" })
}

impl Graph {
    /// `listed`: each package's needs as Oath lists them. A need on a
    /// package the graph does not hold is kept (the tree shows it); the
    /// implicit base is added only when that package is here.
    pub fn new(listed: &BTreeMap<String, Vec<String>>) -> Self {
        let mut needs: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut implicit = BTreeSet::new();
        for (name, ns) in listed {
            let mut all = ns.clone();
            if let Some(base) = implicit_base(name)
                && listed.contains_key(base)
                && !all.iter().any(|n| n == base)
            {
                all.push(base.to_string());
                implicit.insert(name.clone());
            }
            all.sort();
            all.dedup();
            needs.insert(name.clone(), all);
        }
        let mut users: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, ns) in &needs {
            for n in ns {
                users.entry(n.clone()).or_default().push(name.clone());
            }
        }
        for us in users.values_mut() {
            us.sort();
        }
        let mut graph = Graph { needs, implicit, users, depth: BTreeMap::new() };
        let names: Vec<String> = graph.needs.keys().cloned().collect();
        for name in names {
            graph.depth_of(&name, &mut BTreeSet::new());
        }
        graph
    }

    pub fn needs(&self, name: &str) -> &[String] {
        self.needs.get(name).map_or(&[], Vec::as_slice)
    }

    /// Whether the edge `from` → `to` is worth a line: the base by rule
    /// only from a package that needs nothing else (the rest reach it
    /// through what they need).
    pub fn drawn(&self, from: &str, to: &str) -> bool {
        !(self.implicit.contains(from) && implicit_base(from) == Some(to) && self.needs(from).len() > 1)
    }

    pub fn users(&self, name: &str) -> &[String] {
        self.users.get(name).map_or(&[], Vec::as_slice)
    }

    /// How far `name` stands from the ground: 0 for a package that needs
    /// nothing, else one more than its highest need.
    pub fn depth(&self, name: &str) -> usize {
        self.depth.get(name).copied().unwrap_or(0)
    }

    fn depth_of(&mut self, name: &str, path: &mut BTreeSet<String>) -> usize {
        if let Some(d) = self.depth.get(name) {
            return *d;
        }
        // A cycle (Oath refuses them, but a hand-set pin could) counts as
        // the ground rather than recursing forever.
        if !path.insert(name.to_string()) {
            return 0;
        }
        let ns = self.needs(name).to_vec();
        let d = ns
            .iter()
            .map(|n| self.depth_of(n, path) + 1)
            .max()
            .unwrap_or(0);
        path.remove(name);
        self.depth.insert(name.to_string(), d);
        d
    }

    /// Breadth-first rings out from `name`: direct, two away, and so on.
    pub fn levels_up(&self, name: &str) -> Vec<Vec<String>> {
        self.levels(name, |n| self.users(n))
    }

    pub fn levels_down(&self, name: &str) -> Vec<Vec<String>> {
        self.levels(name, |n| self.needs(n))
    }

    fn levels<'a>(&'a self, name: &str, next: impl Fn(&str) -> &'a [String]) -> Vec<Vec<String>> {
        let mut out = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::from([name.to_string()]);
        let mut ring = vec![name.to_string()];
        while !ring.is_empty() {
            let mut nx: Vec<String> = Vec::new();
            for n in &ring {
                for m in next(n) {
                    if seen.insert(m.clone()) {
                        nx.push(m.clone());
                    }
                }
            }
            nx.sort();
            if !nx.is_empty() {
                out.push(nx.clone());
            }
            ring = nx;
        }
        out
    }

    /// Everything that uses `name`, however far up.
    pub fn above(&self, name: &str) -> BTreeSet<String> {
        self.levels_up(name).into_iter().flatten().collect()
    }

    /// Everything `name` needs, however far down.
    pub fn below(&self, name: &str) -> BTreeSet<String> {
        self.levels_down(name).into_iter().flatten().collect()
    }

    /// The packages on some path between `from` and `to` (either way up or
    /// down), both ends included; `None` when neither reaches the other.
    pub fn between(&self, from: &str, to: &str) -> Option<BTreeSet<String>> {
        if from == to {
            return None;
        }
        let (up, down) = (self.above(from), self.below(from));
        let (inner, cone) = if up.contains(to) {
            (self.below(to), up)
        } else if down.contains(to) {
            (self.above(to), down)
        } else {
            return None;
        };
        let mut out: BTreeSet<String> = inner.into_iter().filter(|x| cone.contains(x)).collect();
        out.insert(from.to_string());
        out.insert(to.to_string());
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[(&str, &[&str])]) -> Graph {
        let listed = edges
            .iter()
            .map(|(n, ns)| (n.to_string(), ns.iter().map(|s| s.to_string()).collect()))
            .collect();
        Graph::new(&listed)
    }

    #[test]
    fn every_program_stands_on_glibc_by_rule() {
        let g = graph(&[("glibc", &[]), ("zlib", &[]), ("mesa", &["zlib"]), ("mesa-i386", &[]), ("glibc-i386", &[])]);
        assert_eq!(g.needs("zlib"), ["glibc"]);
        assert_eq!(g.needs("mesa"), ["glibc", "zlib"]);
        assert_eq!(g.needs("mesa-i386"), ["glibc-i386"]);
        assert!(g.needs("glibc").is_empty());
        assert_eq!(g.depth("mesa"), 2);
        assert!(g.drawn("zlib", "glibc"));
        assert!(!g.drawn("mesa", "glibc"));
        assert!(g.drawn("mesa", "zlib"));
    }

    #[test]
    fn sources_and_fonts_stand_alone() {
        let g = graph(&[("glibc", &[]), ("codex-src", &[]), ("font-noto", &[])]);
        assert!(g.needs("codex-src").is_empty());
        assert!(g.needs("font-noto").is_empty());
        assert!(g.users("glibc").is_empty());
    }

    #[test]
    fn no_base_is_drawn_when_it_is_not_here() {
        let g = graph(&[("zlib", &[])]);
        assert!(g.needs("zlib").is_empty());
    }

    #[test]
    fn rings_and_cones() {
        let g = graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[]), ("d", &["c"])]);
        assert_eq!(g.levels_down("a"), vec![vec!["b".to_string()], vec!["c".to_string()]]);
        assert_eq!(g.levels_up("c"), vec![vec!["b".to_string(), "d".to_string()], vec!["a".to_string()]]);
        assert_eq!(g.above("c").len(), 3);
        assert_eq!(g.depth("a"), 2);
    }

    #[test]
    fn paths_between_two_packages() {
        let g = graph(&[("a", &["b", "x"]), ("b", &["c"]), ("x", &["c"]), ("c", &[]), ("d", &["c"])]);
        let p = g.between("c", "a").unwrap();
        assert_eq!(p, ["a", "b", "c", "x"].iter().map(|s| s.to_string()).collect());
        assert!(g.between("a", "d").is_none());
    }

    #[test]
    fn a_cycle_does_not_hang() {
        let g = graph(&[("a", &["b"]), ("b", &["a"])]);
        assert!(g.depth("a") <= 2);
    }
}
