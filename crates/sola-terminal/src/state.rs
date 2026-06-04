use std::collections::{BTreeMap, HashMap};

use crate::emulator::Emulator;
use crate::pty::PtyBackend;

/// Live per-tab runtime: the alacritty emulator + the PTY backend handle.
///
/// Held in `Tabs.runtime` keyed by tab id. The MAIN side reads `emulator` for
/// the renderer (Task 2.5) and calls `emulator.resize()`; the reader thread
/// (spawned by `PtyBackend::spawn_or_attach`) drives a cloned term handle. The
/// runtime is dropped when the tab is removed — but the tab-close path calls
/// `backend.close()` FIRST, because a plain drop deliberately preserves the
/// tmux session (so a crash doesn't nuke sessions).
pub struct TabRuntime {
    pub emulator: Emulator,
    pub backend: PtyBackend,
}

/// Metadata for a single terminal tab, as persisted on the bus.
#[derive(Clone, Debug, PartialEq)]
pub struct TabMeta {
    pub id: String,
    pub tmux_session: String,
    pub cwd: Option<String>,
    pub ordinal: u32,
}

/// Runtime tab model. Keyed by tab id, ordered by ordinal.
#[derive(Default)]
pub struct Tabs {
    meta: BTreeMap<String, TabMeta>,
    /// Live runtime (emulator + PTY backend) per attached tab. A tab can have
    /// `meta` without a `runtime` momentarily during attach; lookups tolerate
    /// the gap.
    runtime: HashMap<String, TabRuntime>,
}

impl Tabs {
    pub fn upsert_meta(&mut self, m: TabMeta) {
        self.meta.insert(m.id.clone(), m);
    }

    /// Install the live runtime for an attached tab.
    pub fn insert_runtime(&mut self, id: String, rt: TabRuntime) {
        self.runtime.insert(id, rt);
    }

    pub fn runtime(&self, id: &str) -> Option<&TabRuntime> {
        self.runtime.get(id)
    }

    pub fn runtime_mut(&mut self, id: &str) -> Option<&mut TabRuntime> {
        self.runtime.get_mut(id)
    }

    /// Remove the tab's metadata AND its runtime. The runtime's `PtyBackend`
    /// drops here (preserving tmux); callers that want the tmux session GONE
    /// must call `backend.close()` before `remove`.
    pub fn remove(&mut self, id: &str) {
        self.meta.remove(id);
        self.runtime.remove(id);
    }

    pub fn get(&self, id: &str) -> Option<&TabMeta> {
        self.meta.get(id)
    }

    pub fn len(&self) -> usize {
        self.meta.len()
    }

    pub fn is_empty(&self) -> bool {
        self.meta.is_empty()
    }

    pub fn ids_in_order(&self) -> Vec<String> {
        let mut v: Vec<&TabMeta> = self.meta.values().collect();
        v.sort_by(|a, b| a.ordinal.cmp(&b.ordinal).then(a.id.cmp(&b.id)));
        v.into_iter().map(|m| m.id.clone()).collect()
    }

    pub fn ordered_meta(&self) -> Vec<TabMeta> {
        self.ids_in_order()
            .into_iter()
            .filter_map(|id| self.meta.get(&id).cloned())
            .collect()
    }

    /// The cwd of the given tab, if known. Used by `NewTab` to inherit the
    /// active tab's directory as the new session's start dir.
    pub fn cwd_of(&self, id: &str) -> Option<String> {
        self.meta.get(id).and_then(|m| m.cwd.clone())
    }

    /// Iterator over all live tab runtimes (mutable).
    ///
    /// Used by the resize path (Task 2.6) to push a new grid size into every
    /// attached tab without knowing their ids.
    pub fn runtimes_mut(&mut self) -> impl Iterator<Item = &mut TabRuntime> {
        self.runtime.values_mut()
    }
}

/// Next ordinal for a new tab: `max(existing) + 1`, or 0 when there are no
/// tabs. Pure so it can be unit-tested without spawning a PTY.
pub fn next_ordinal(existing: &[u32]) -> u32 {
    existing.iter().copied().max().map(|m| m + 1).unwrap_or(0)
}

/// The cwd a new tab should start in, given the active tab's cwd. Currently a
/// straight inherit (active dir or `None`); factored out so the policy is
/// testable and easy to evolve.
pub fn inherit_cwd(active_cwd: Option<&str>) -> Option<String> {
    active_cwd.map(|s| s.to_string())
}

/// Pick the tab to focus after `closing` is removed, given the current display
/// order (`ids_in_order`, BEFORE removal). Returns the previous tab in order,
/// or the next one if `closing` was first, or `None` if it was the only tab.
/// Pure so the selection policy is unit-testable.
pub fn next_active_after_close(order: &[String], closing: &str) -> Option<String> {
    let idx = order.iter().position(|id| id == closing)?;
    if idx > 0 {
        order.get(idx - 1).cloned()
    } else {
        // First tab closed — fall through to the next one (now first).
        order.get(idx + 1).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: &str, ord: u32) -> TabMeta {
        TabMeta {
            id: id.to_string(),
            tmux_session: format!("sola-{id}"),
            cwd: None,
            ordinal: ord,
        }
    }

    #[test]
    fn upsert_keeps_sorted_by_ordinal() {
        let mut tabs = Tabs::default();
        tabs.upsert_meta(meta("b", 2));
        tabs.upsert_meta(meta("a", 1));
        assert_eq!(tabs.ids_in_order(), vec!["a", "b"]);
    }

    #[test]
    fn remove_drops_the_tab() {
        let mut tabs = Tabs::default();
        tabs.upsert_meta(meta("a", 1));
        tabs.remove("a");
        assert!(tabs.is_empty());
    }

    #[test]
    fn next_ordinal_is_max_plus_one() {
        assert_eq!(next_ordinal(&[]), 0);
        assert_eq!(next_ordinal(&[0]), 1);
        assert_eq!(next_ordinal(&[0, 2, 1]), 3);
        // Gaps are fine — we take max, not count.
        assert_eq!(next_ordinal(&[5, 1]), 6);
    }

    #[test]
    fn inherit_cwd_passes_through_active_dir() {
        assert_eq!(inherit_cwd(Some("/home/x")), Some("/home/x".to_string()));
        assert_eq!(inherit_cwd(None), None);
    }

    #[test]
    fn close_active_picks_previous_neighbor() {
        let order = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        // Closing a middle/last tab focuses the previous one.
        assert_eq!(next_active_after_close(&order, "b"), Some("a".to_string()));
        assert_eq!(next_active_after_close(&order, "c"), Some("b".to_string()));
    }

    #[test]
    fn close_first_tab_picks_next() {
        let order = vec!["a".to_string(), "b".to_string()];
        assert_eq!(next_active_after_close(&order, "a"), Some("b".to_string()));
    }

    #[test]
    fn close_only_tab_picks_none() {
        let order = vec!["a".to_string()];
        assert_eq!(next_active_after_close(&order, "a"), None);
    }

    #[test]
    fn close_unknown_id_picks_none() {
        let order = vec!["a".to_string()];
        assert_eq!(next_active_after_close(&order, "zzz"), None);
    }
}
