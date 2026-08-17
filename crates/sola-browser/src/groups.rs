//! Chrome-owned tab groups. CEF never sees these.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::engine::{TabId, TabInfo};
use crate::session::{SessionGroup, SessionTab};

static NEXT_GROUP: AtomicU64 = AtomicU64::new(1);

/// One named, collapsible folder in the strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabGroup {
    pub id: String,
    pub name: String,
    pub collapsed: bool,
}

/// Groups in strip order + per-tab membership.
#[derive(Debug, Clone, Default)]
pub struct Groups {
    pub groups: Vec<TabGroup>,
    pub member: HashMap<TabId, String>,
}

/// A visible strip row (header or tab). Collapsed members are omitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StripRow {
    Header(String),
    Tab(TabId),
}

impl Groups {
    pub fn restore(tabs: &[SessionTab], ids: &[TabId], meta: &[SessionGroup]) -> Self {
        let mut g = Self::default();
        for m in meta {
            if m.id.trim().is_empty() || m.name.trim().is_empty() {
                continue;
            }
            g.groups.push(TabGroup {
                id: m.id.clone(),
                name: m.name.clone(),
                collapsed: m.collapsed,
            });
        }
        for (tab, id) in tabs.iter().zip(ids.iter()) {
            let Some(gid) = tab.group_id.as_deref() else {
                continue;
            };
            if g.groups.iter().any(|gr| gr.id == gid) {
                g.member.insert(*id, gid.to_string());
            }
        }
        g.dissolve_empty();
        g
    }

    pub fn to_session(&self) -> Vec<SessionGroup> {
        self.groups
            .iter()
            .map(|g| SessionGroup {
                id: g.id.clone(),
                name: g.name.clone(),
                collapsed: g.collapsed,
            })
            .collect()
    }

    pub fn group(&self, id: &str) -> Option<&TabGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn group_mut(&mut self, id: &str) -> Option<&mut TabGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    pub fn of_tab(&self, id: TabId) -> Option<&str> {
        self.member.get(&id).map(String::as_str)
    }

    pub fn visible_rows(&self, tabs: &[TabInfo]) -> Vec<StripRow> {
        let mut rows = Vec::new();
        for g in &self.groups {
            rows.push(StripRow::Header(g.id.clone()));
            if g.collapsed {
                continue;
            }
            for t in tabs {
                if self.member.get(&t.id).map(String::as_str) == Some(g.id.as_str()) {
                    rows.push(StripRow::Tab(t.id));
                }
            }
        }
        for t in tabs {
            if !self.member.contains_key(&t.id) {
                rows.push(StripRow::Tab(t.id));
            }
        }
        rows
    }

    /// Groups at the top (in `self.groups` order), then the loose run.
    pub fn normalize(&self, tabs: &mut Vec<TabInfo>) {
        let mut out = Vec::with_capacity(tabs.len());
        for g in &self.groups {
            for t in tabs.iter() {
                if self.member.get(&t.id).map(String::as_str) == Some(g.id.as_str()) {
                    out.push(t.clone());
                }
            }
        }
        for t in tabs.iter() {
            if !self.member.contains_key(&t.id) {
                out.push(t.clone());
            }
        }
        *tabs = out;
    }

    pub fn dissolve_empty(&mut self) {
        self.groups
            .retain(|g| self.member.values().any(|id| id == &g.id));
        let live: std::collections::HashSet<&str> =
            self.groups.iter().map(|g| g.id.as_str()).collect();
        self.member.retain(|_, gid| live.contains(gid.as_str()));
    }

    pub fn on_tab_closed(&mut self, id: TabId) {
        self.member.remove(&id);
        self.dissolve_empty();
    }

    pub fn next_name(&self) -> String {
        let mut n = 1u32;
        loop {
            let name = if n == 1 {
                "Group".to_string()
            } else {
                format!("Group {n}")
            };
            if !self.groups.iter().any(|g| g.name == name) {
                return name;
            }
            n += 1;
        }
    }

    /// New group at the end of the groups region. Tab leaves any old group.
    pub fn new_group(&mut self, tab: TabId) {
        self.leave(tab);
        let id = new_group_id();
        self.groups.push(TabGroup {
            id: id.clone(),
            name: self.next_name(),
            collapsed: false,
        });
        self.member.insert(tab, id);
        self.dissolve_empty();
    }

    pub fn add_to(&mut self, tab: TabId, group_id: &str) {
        if !self.groups.iter().any(|g| g.id == group_id) {
            return;
        }
        self.leave(tab);
        self.member.insert(tab, group_id.to_string());
        self.dissolve_empty();
    }

    /// Insert `tab` immediately after `after` (or append). Same group as
    /// `after`. Expands a collapsed group so the new row is visible.
    pub fn insert_beside(&mut self, tabs: &mut Vec<TabInfo>, after: TabId, tab: TabInfo) {
        let id = tab.id;
        match tabs.iter().position(|t| t.id == after) {
            Some(i) => tabs.insert(i + 1, tab),
            None => tabs.push(tab),
        }
        if let Some(gid) = self.of_tab(after).map(str::to_string) {
            self.add_to(id, &gid);
            if let Some(g) = self.group_mut(&gid) {
                g.collapsed = false;
            }
        }
    }

    pub fn ungroup_tab(&mut self, tab: TabId) {
        self.leave(tab);
        self.dissolve_empty();
    }

    /// Dissolve the group; members become loose (order kept by normalize).
    pub fn ungroup_all(&mut self, group_id: &str) {
        self.member.retain(|_, gid| gid != group_id);
        self.groups.retain(|g| g.id != group_id);
    }

    pub fn toggle(&mut self, group_id: &str) {
        if let Some(g) = self.group_mut(group_id) {
            g.collapsed = !g.collapsed;
        }
    }

    pub fn rename(&mut self, group_id: &str, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        if let Some(g) = self.group_mut(group_id) {
            g.name = name;
        }
    }

    fn leave(&mut self, tab: TabId) {
        self.member.remove(&tab);
    }

    /// Apply a finished drag of visible row `from` → `to`.
    pub fn apply_drop(&mut self, tabs: &mut Vec<TabInfo>, from: usize, to: usize) {
        let rows = self.visible_rows(tabs);
        if from >= rows.len() {
            return;
        }
        match rows[from].clone() {
            StripRow::Header(gid) => self.drop_header(tabs, &rows, &gid, to),
            StripRow::Tab(tid) => self.drop_tab(tabs, &rows, tid, from, to),
        }
        self.dissolve_empty();
        self.normalize(tabs);
    }

    fn drop_header(&mut self, _tabs: &[TabInfo], rows: &[StripRow], gid: &str, to: usize) {
        let Some(from_g) = self.groups.iter().position(|g| g.id == gid) else {
            return;
        };
        // Map the visible drop index onto a group index. Loose region
        // clamps to the last group (header drag never ungroups).
        let mut dest = 0usize;
        let last = self.groups.len().saturating_sub(1);
        for (i, row) in rows.iter().enumerate() {
            match row {
                StripRow::Header(id) => {
                    if let Some(gi) = self.groups.iter().position(|g| g.id == *id) {
                        dest = gi;
                    }
                }
                StripRow::Tab(tid) => {
                    if self.member.contains_key(tid) {
                        if let Some(gid) = self.member.get(tid) {
                            if let Some(gi) = self.groups.iter().position(|g| g.id == *gid) {
                                dest = gi;
                            }
                        }
                    } else {
                        dest = last;
                    }
                }
            }
            if i >= to {
                break;
            }
        }
        if dest == from_g {
            return;
        }
        let g = self.groups.remove(from_g);
        let dest = dest.min(self.groups.len());
        self.groups.insert(dest, g);
    }

    fn drop_tab(
        &mut self,
        tabs: &mut Vec<TabInfo>,
        rows: &[StripRow],
        tid: TabId,
        from: usize,
        to: usize,
    ) {
        let mut rest = rows.to_vec();
        rest.remove(from);
        let insert_at = to.min(rest.len());

        enum Dest {
            Join { gid: String, after: Option<TabId> },
            JoinAppend(String),
            Loose { after: Option<TabId> },
        }

        let after_row = rest.get(insert_at);
        let before_row = if insert_at == 0 {
            None
        } else {
            rest.get(insert_at - 1)
        };

        let dest = match after_row {
            Some(StripRow::Header(gid)) => Dest::JoinAppend(gid.clone()),
            Some(StripRow::Tab(next)) => {
                if let Some(gid) = self.member.get(next).cloned() {
                    Dest::Join {
                        gid,
                        after: None, // insert before `next`
                    }
                } else {
                    Dest::Loose {
                        after: match before_row {
                            Some(StripRow::Tab(prev)) if !self.member.contains_key(prev) => {
                                Some(*prev)
                            }
                            _ => None,
                        },
                    }
                }
            }
            None => match before_row {
                Some(StripRow::Header(gid)) => Dest::JoinAppend(gid.clone()),
                Some(StripRow::Tab(prev)) => {
                    if let Some(gid) = self.member.get(prev).cloned() {
                        Dest::Join {
                            gid,
                            after: Some(*prev),
                        }
                    } else {
                        Dest::Loose { after: Some(*prev) }
                    }
                }
                None => Dest::Loose { after: None },
            },
        };

        // Special-case Join-before-next: we still need to splice order.
        // Rebuild tab order from dest + existing relative orders.
        match dest {
            Dest::JoinAppend(gid) => {
                self.member.insert(tid, gid.clone());
                if let Some(pos) = tabs.iter().position(|t| t.id == tid) {
                    let tab = tabs.remove(pos);
                    let insert = tabs
                        .iter()
                        .rposition(|t| {
                            self.member.get(&t.id).map(String::as_str) == Some(gid.as_str())
                        })
                        .map(|i| i + 1)
                        .unwrap_or(tabs.len());
                    tabs.insert(insert.min(tabs.len()), tab);
                }
            }
            Dest::Join { gid, after } => {
                self.member.insert(tid, gid.clone());
                if let Some(pos) = tabs.iter().position(|t| t.id == tid) {
                    let tab = tabs.remove(pos);
                    let insert = match after {
                        Some(prev) => tabs
                            .iter()
                            .position(|t| t.id == prev)
                            .map(|i| i + 1)
                            .unwrap_or(tabs.len()),
                        None => {
                            // Insert before the `next` tab still in `rest` at insert_at.
                            if let Some(StripRow::Tab(next)) = rest.get(insert_at) {
                                tabs.iter()
                                    .position(|t| t.id == *next)
                                    .unwrap_or(tabs.len())
                            } else {
                                tabs.len()
                            }
                        }
                    };
                    tabs.insert(insert.min(tabs.len()), tab);
                }
            }
            Dest::Loose { after } => {
                self.member.remove(&tid);
                if let Some(pos) = tabs.iter().position(|t| t.id == tid) {
                    let tab = tabs.remove(pos);
                    let insert = match after {
                        Some(prev) => tabs
                            .iter()
                            .position(|t| t.id == prev)
                            .map(|i| i + 1)
                            .unwrap_or(tabs.len()),
                        None => {
                            // Start of loose run (or empty).
                            tabs.iter()
                                .position(|t| !self.member.contains_key(&t.id))
                                .unwrap_or(tabs.len())
                        }
                    };
                    tabs.insert(insert.min(tabs.len()), tab);
                }
            }
        }
    }
}

fn new_group_id() -> String {
    format!("g{}", NEXT_GROUP.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u64, title: &str) -> TabInfo {
        TabInfo::chrome(TabId(id), format!("https://x/{id}"), title)
    }

    fn ids(tabs: &[TabInfo]) -> Vec<u64> {
        tabs.iter().map(|t| t.id.0).collect()
    }

    fn setup() -> (Groups, Vec<TabInfo>) {
        let mut g = Groups::default();
        g.groups.push(TabGroup {
            id: "work".into(),
            name: "Work".into(),
            collapsed: false,
        });
        g.groups.push(TabGroup {
            id: "research".into(),
            name: "Research".into(),
            collapsed: false,
        });
        g.member.insert(TabId(1), "work".into());
        g.member.insert(TabId(2), "work".into());
        g.member.insert(TabId(3), "research".into());
        let tabs = vec![
            tab(1, "a"),
            tab(2, "b"),
            tab(3, "c"),
            tab(4, "d"),
            tab(5, "e"),
        ];
        g.normalize(&mut { tabs.clone() });
        let mut tabs = tabs;
        g.normalize(&mut tabs);
        (g, tabs)
    }

    #[test]
    fn normalize_groups_then_loose() {
        let (g, tabs) = setup();
        assert_eq!(ids(&tabs), vec![1, 2, 3, 4, 5]);
        let rows = g.visible_rows(&tabs);
        assert_eq!(
            rows,
            vec![
                StripRow::Header("work".into()),
                StripRow::Tab(TabId(1)),
                StripRow::Tab(TabId(2)),
                StripRow::Header("research".into()),
                StripRow::Tab(TabId(3)),
                StripRow::Tab(TabId(4)),
                StripRow::Tab(TabId(5)),
            ]
        );
    }

    #[test]
    fn collapsed_hides_members() {
        let (mut g, tabs) = setup();
        g.group_mut("work").unwrap().collapsed = true;
        let rows = g.visible_rows(&tabs);
        assert_eq!(
            rows,
            vec![
                StripRow::Header("work".into()),
                StripRow::Header("research".into()),
                StripRow::Tab(TabId(3)),
                StripRow::Tab(TabId(4)),
                StripRow::Tab(TabId(5)),
            ]
        );
    }

    #[test]
    fn drag_loose_among_members_joins() {
        let (mut g, mut tabs) = setup();
        // rows: H w, 1, 2, H r, 3, 4, 5  — drag 4 (idx 5) to idx 2 (before tab 2)
        g.apply_drop(&mut tabs, 5, 2);
        assert_eq!(g.of_tab(TabId(4)), Some("work"));
        assert_eq!(ids(&tabs), vec![1, 4, 2, 3, 5]);
    }

    #[test]
    fn drag_onto_header_appends() {
        let (mut g, mut tabs) = setup();
        // drag loose 4 (idx 5) onto research header (idx 3)
        g.apply_drop(&mut tabs, 5, 3);
        assert_eq!(g.of_tab(TabId(4)), Some("research"));
        assert_eq!(ids(&tabs), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn drag_member_to_loose_ungroups() {
        let (mut g, mut tabs) = setup();
        // drag work tab 1 (idx 1) to first loose slot (idx 5, tab 4)
        g.apply_drop(&mut tabs, 1, 5);
        assert_eq!(g.of_tab(TabId(1)), None);
        assert_eq!(g.of_tab(TabId(2)), Some("work"));
        assert!(ids(&tabs).ends_with(&[2, 3, 1, 4, 5]) || ids(&tabs).contains(&1));
        assert!(!g.member.contains_key(&TabId(1)));
        g.normalize(&mut tabs);
        let loose: Vec<u64> = tabs
            .iter()
            .filter(|t| g.of_tab(t.id).is_none())
            .map(|t| t.id.0)
            .collect();
        assert!(loose.contains(&1));
        assert!(loose.contains(&4));
        assert!(loose.contains(&5));
    }

    #[test]
    fn last_member_out_dissolves() {
        let (mut g, mut tabs) = setup();
        // research has only tab 3 (idx 4). Drag to loose (idx 5).
        g.apply_drop(&mut tabs, 4, 5);
        assert!(g.group("research").is_none());
        assert_eq!(g.of_tab(TabId(3)), None);
    }

    #[test]
    fn new_group_lifts_tab() {
        let (mut g, mut tabs) = setup();
        g.new_group(TabId(5));
        g.normalize(&mut tabs);
        assert_eq!(g.groups.len(), 3);
        assert_eq!(g.groups[2].name, "Group");
        assert_eq!(
            g.of_tab(TabId(5)).map(str::to_string),
            Some(g.groups[2].id.clone())
        );
        assert_eq!(tabs.last().map(|t| t.id.0), Some(4)); // remaining loose
    }

    #[test]
    fn empty_dissolves_on_close() {
        let (mut g, _) = setup();
        g.on_tab_closed(TabId(3));
        assert!(g.group("research").is_none());
    }

    #[test]
    fn restore_coalesces_and_drops_orphans() {
        let session_tabs = vec![
            SessionTab {
                url: "https://a/".into(),
                title: "a".into(),
                group_id: Some("work".into()),
                ..SessionTab::default()
            },
            SessionTab {
                url: "https://b/".into(),
                title: "b".into(),
                group_id: Some("missing".into()),
                ..SessionTab::default()
            },
            SessionTab {
                url: "https://c/".into(),
                title: "c".into(),
                group_id: Some("work".into()),
                ..SessionTab::default()
            },
        ];
        let ids = vec![TabId(10), TabId(11), TabId(12)];
        let meta = vec![SessionGroup {
            id: "work".into(),
            name: "Work".into(),
            collapsed: true,
        }];
        let g = Groups::restore(&session_tabs, &ids, &meta);
        assert_eq!(g.groups.len(), 1);
        assert_eq!(g.of_tab(TabId(10)), Some("work"));
        assert_eq!(g.of_tab(TabId(11)), None);
        assert_eq!(g.of_tab(TabId(12)), Some("work"));
        assert!(g.groups[0].collapsed);
    }

    #[test]
    fn header_drag_reorders_blocks() {
        let (mut g, mut tabs) = setup();
        // drag research header (idx 3) to work header (idx 0)
        g.apply_drop(&mut tabs, 3, 0);
        assert_eq!(
            g.groups.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            vec!["research", "work"]
        );
        g.normalize(&mut tabs);
        assert_eq!(ids(&tabs), vec![3, 1, 2, 4, 5]);
    }

    #[test]
    fn insert_beside_loose_goes_under_active() {
        let (mut g, mut tabs) = setup();
        g.insert_beside(&mut tabs, TabId(4), tab(9, "new"));
        assert_eq!(ids(&tabs), vec![1, 2, 3, 4, 9, 5]);
        assert!(g.of_tab(TabId(9)).is_none());
    }

    #[test]
    fn insert_beside_joins_active_group() {
        let (mut g, mut tabs) = setup();
        g.insert_beside(&mut tabs, TabId(1), tab(9, "new"));
        assert_eq!(g.of_tab(TabId(9)), Some("work"));
        g.normalize(&mut tabs);
        assert_eq!(ids(&tabs), vec![1, 9, 2, 3, 4, 5]);
    }

    #[test]
    fn insert_beside_expands_collapsed_group() {
        let (mut g, mut tabs) = setup();
        g.group_mut("work").unwrap().collapsed = true;
        g.insert_beside(&mut tabs, TabId(2), tab(9, "new"));
        assert!(!g.group("work").unwrap().collapsed);
        assert_eq!(g.of_tab(TabId(9)), Some("work"));
    }
}
