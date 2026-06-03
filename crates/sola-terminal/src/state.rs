use std::collections::BTreeMap;

/// Metadata for a single terminal tab, as persisted on the bus.
#[derive(Clone, Debug, PartialEq)]
pub struct TabMeta {
    pub id: String,
    pub tmux_session: String,
    pub cwd: Option<String>,
    pub ordinal: u32,
}

/// Runtime tab model. Keyed by tab id, ordered by ordinal.
/// Phase 2: add `runtime: HashMap<String, TabRuntime>` here
#[derive(Default)]
pub struct Tabs {
    meta: BTreeMap<String, TabMeta>,
}

impl Tabs {
    pub fn upsert_meta(&mut self, m: TabMeta) {
        self.meta.insert(m.id.clone(), m);
    }

    pub fn remove(&mut self, id: &str) {
        self.meta.remove(id);
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
}
