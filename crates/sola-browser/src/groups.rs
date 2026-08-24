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

/// Which side of a drop slot the pointer is on.
///
/// Row math lands on the next header / first loose tab when you aim at
/// the floor of a group. The top half of that next row still belongs to
/// the pocket above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropBias {
    /// Center or lower half of the slot — the row under the pointer.
    OnSlot,
    /// Top half of the slot — still the pocket above this row.
    PocketAbove,
}

/// Upper half of slot `to` is the floor of the previous pocket.
pub fn drop_bias(from: usize, start_y: f32, cursor_y: f32, row_h: f32, to: usize) -> DropBias {
    match sola_kit::components::sidebar::panel_drop_bias(from, start_y, cursor_y, row_h, to) {
        sola_kit::components::sidebar::PanelDropBias::PocketAbove => DropBias::PocketAbove,
        sola_kit::components::sidebar::PanelDropBias::OnSlot => DropBias::OnSlot,
    }
}

impl Groups {
    pub fn restore(tabs: &[SessionTab], ids: &[TabId], meta: &[SessionGroup]) -> Self {
        let mut g = Self::default();
        for m in meta {
            if m.id.trim().is_empty() || m.name.trim().is_empty() {
                continue;
            }
            if g.groups.iter().any(|gr| gr.id == m.id) {
                continue;
            }
            g.groups.push(TabGroup {
                id: m.id.clone(),
                name: m.name.clone(),
                collapsed: m.collapsed,
            });
        }
        bump_id_counter(&g.groups);
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
        let mut emitted = std::collections::HashSet::new();
        for t in tabs {
            match self.member.get(&t.id) {
                Some(gid) => {
                    if emitted.insert(gid.clone()) {
                        rows.push(StripRow::Header(gid.clone()));
                    }
                    let collapsed = self.group(gid).is_some_and(|g| g.collapsed);
                    if !collapsed {
                        rows.push(StripRow::Tab(t.id));
                    }
                }
                None => rows.push(StripRow::Tab(t.id)),
            }
        }
        rows
    }

    /// `(grouped, len)` per strip section — same order as the kit sidebar.
    pub fn section_lens(&self, tabs: &[TabInfo]) -> Vec<(bool, usize)> {
        let rows = self.visible_rows(tabs);
        let mut lens = Vec::new();
        let mut i = 0usize;
        while i < rows.len() {
            match &rows[i] {
                StripRow::Header(gid) => {
                    let start = i;
                    i += 1;
                    while i < rows.len() {
                        match &rows[i] {
                            StripRow::Header(_) => break,
                            StripRow::Tab(id) if self.of_tab(*id) == Some(gid.as_str()) => {
                                i += 1;
                            }
                            _ => break,
                        }
                    }
                    lens.push((true, i - start));
                }
                StripRow::Tab(_) => {
                    let start = i;
                    while i < rows.len() {
                        match &rows[i] {
                            StripRow::Tab(id) if self.of_tab(*id).is_none() => i += 1,
                            _ => break,
                        }
                    }
                    lens.push((false, i - start));
                }
            }
        }
        lens
    }

    /// Section index for a visible row, matching kit sidebar order.
    pub fn section_index(lens: &[(bool, usize)], row: usize) -> usize {
        let mut start = 0usize;
        for (i, (_, len)) in lens.iter().enumerate() {
            if row < start + *len {
                return i;
            }
            start += *len;
        }
        lens.len().saturating_sub(1)
    }

    pub fn section_start(lens: &[(bool, usize)], si: usize) -> usize {
        lens.iter().take(si).map(|(_, len)| *len).sum()
    }

    /// Exclusive `[start, end)` of rows that may take a sibling offset.
    /// Grouped sections skip the header so the title never yields.
    pub fn member_range(lens: &[(bool, usize)], si: usize) -> (usize, usize) {
        let mut start = 0usize;
        for (i, (grouped, len)) in lens.iter().enumerate() {
            if i == si {
                let end = start + *len;
                let first = if *grouped {
                    (start + 1).min(end)
                } else {
                    start
                };
                return (first, end);
            }
            start += *len;
        }
        (0, 0)
    }

    /// Cluster each group's members at first encounter. Loose tabs stay
    /// put — groups and ungrouped rows may intermix.
    pub fn normalize(&self, tabs: &mut Vec<TabInfo>) {
        *tabs = expand_tokens(&strip_tokens(self, tabs), tabs, self);
    }

    fn sync_group_order(&mut self, tabs: &[TabInfo]) {
        let mut next = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for t in tabs {
            if let Some(gid) = self.member.get(&t.id) {
                if seen.insert(gid.clone()) {
                    if let Some(g) = self.groups.iter().find(|g| g.id == *gid).cloned() {
                        next.push(g);
                    }
                }
            }
        }
        for g in &self.groups {
            if !next.iter().any(|n| n.id == g.id) {
                next.push(g.clone());
            }
        }
        self.groups = next;
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
        let id = new_group_id(&self.groups);
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

    /// Append `tab` as loose at the end of the strip (⌘T, xdg-open).
    pub fn append_loose(&mut self, tabs: &mut Vec<TabInfo>, tab: TabInfo) {
        self.leave(tab.id);
        tabs.push(tab);
    }

    /// Insert `tab` immediately after `after` (or append). Same group as
    /// `after`. Expands a collapsed group so the new row is visible.
    /// ⌘-click only — not ⌘T / OpenUrl.
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
        self.apply_drop_with(tabs, from, to, DropBias::OnSlot);
    }

    /// Like [`Self::apply_drop`], with a half-row bias so the floor of a
    /// group well appends to that group instead of joining the next one.
    pub fn apply_drop_with(
        &mut self,
        tabs: &mut Vec<TabInfo>,
        from: usize,
        to: usize,
        bias: DropBias,
    ) {
        let rows = self.visible_rows(tabs);
        if from >= rows.len() {
            return;
        }
        match rows[from].clone() {
            StripRow::Header(gid) => self.drop_header(tabs, &rows, &gid, to),
            StripRow::Tab(tid) => self.drop_tab(tabs, &rows, tid, from, to, bias),
        }
        self.dissolve_empty();
        self.normalize(tabs);
        self.sync_group_order(tabs);
    }

    /// Apply a kit [`sola_kit::components::Drop`] (ids, not geometry).
    pub fn apply_kit_drop(&mut self, tabs: &mut Vec<TabInfo>, drop: &sola_kit::components::Drop) {
        use sola_kit::components::Dest;
        match &drop.dest {
            Dest::BlockBefore { before } => {
                self.place_group_block(tabs, &drop.id, before.as_deref());
            }
            Dest::Sections(order) => {
                let mut tok = strip_tokens(self, tabs);
                let mut gi = 0usize;
                for t in &mut tok {
                    if let Token::Group(_) = t {
                        if gi < order.len() {
                            *t = Token::Group(order[gi].clone());
                            gi += 1;
                        }
                    }
                }
                *tabs = expand_tokens(&tok, tabs, self);
                self.sync_group_order(tabs);
            }
            Dest::Join { section, before } => {
                let Some(tid) = parse_tab(&drop.id) else {
                    return;
                };
                self.member.insert(tid, section.clone());
                splice_before(tabs, tid, before.as_deref().and_then(parse_tab), self);
            }
            Dest::Loose { before } => {
                let Some(tid) = parse_tab(&drop.id) else {
                    return;
                };
                self.member.remove(&tid);
                match before.as_deref().and_then(parse_tab) {
                    Some(next) => splice_before(tabs, tid, Some(next), self),
                    None => {
                        // End of the strip, not the start of the loose run.
                        if let Some(pos) = tabs.iter().position(|t| t.id == tid) {
                            let tab = tabs.remove(pos);
                            tabs.push(tab);
                        }
                    }
                }
            }
            Dest::BeforeGroup { id } => {
                let Some(tid) = parse_tab(&drop.id) else {
                    return;
                };
                self.member.remove(&tid);
                let before = tabs
                    .iter()
                    .find(|t| self.member.get(&t.id).map(String::as_str) == Some(id.as_str()))
                    .map(|t| t.id);
                splice_before(tabs, tid, before, self);
            }
        }
        self.dissolve_empty();
        self.normalize(tabs);
        self.sync_group_order(tabs);
    }

    fn place_group_block(&mut self, tabs: &mut Vec<TabInfo>, gid: &str, before: Option<&str>) {
        if !self.groups.iter().any(|g| g.id == gid) {
            return;
        }
        let mut tok = strip_tokens(self, tabs);
        tok.retain(|t| !matches!(t, Token::Group(id) if id == gid));
        let at = match before {
            Some(id) => tok
                .iter()
                .position(|t| match t {
                    Token::Group(g) => g == id,
                    Token::Loose(tid) => tid.0.to_string() == id,
                })
                .unwrap_or(tok.len()),
            None => tok.len(),
        };
        tok.insert(at.min(tok.len()), Token::Group(gid.to_string()));
        *tabs = expand_tokens(&tok, tabs, self);
    }

    fn drop_header(&mut self, tabs: &mut Vec<TabInfo>, rows: &[StripRow], gid: &str, to: usize) {
        let before = rows.get(to).map(|row| match row {
            StripRow::Header(id) => id.clone(),
            StripRow::Tab(tid) => tid.0.to_string(),
        });
        self.place_group_block(tabs, gid, before.as_deref());
    }

    fn drop_tab(
        &mut self,
        tabs: &mut Vec<TabInfo>,
        rows: &[StripRow],
        tid: TabId,
        from: usize,
        to: usize,
        bias: DropBias,
    ) {
        let mut rest = rows.to_vec();
        rest.remove(from);
        let insert_at = to.min(rest.len());

        let after_row = rest.get(insert_at);
        let before_row = if insert_at == 0 {
            None
        } else {
            rest.get(insert_at - 1)
        };

        // Top half of the next section's first row is still the floor of
        // the pocket above (last member of that group).
        let dest = dest_for_drop(self, before_row, after_row, bias);

        // Special-case Join-before-next: we still need to splice order.
        // Rebuild tab order from dest + existing relative orders.
        match dest {
            Dest::Noop => {}
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

enum Dest {
    Join {
        gid: String,
        after: Option<TabId>,
    },
    JoinAppend(String),
    Loose {
        after: Option<TabId>,
    },
    /// Group title is not a drop target.
    Noop,
}

fn dest_for_drop(
    groups: &Groups,
    before_row: Option<&StripRow>,
    after_row: Option<&StripRow>,
    bias: DropBias,
) -> Dest {
    if bias == DropBias::PocketAbove {
        match (before_row, after_row) {
            (Some(StripRow::Tab(prev)), Some(StripRow::Header(next_gid))) => {
                if let Some(gid) = groups.member.get(prev).cloned() {
                    if gid != *next_gid {
                        return Dest::Join {
                            gid,
                            after: Some(*prev),
                        };
                    }
                }
                return Dest::JoinAppend(next_gid.clone());
            }
            (Some(StripRow::Header(prev_gid)), Some(StripRow::Header(next_gid)))
                if prev_gid != next_gid =>
            {
                return Dest::JoinAppend(prev_gid.clone());
            }
            (Some(StripRow::Tab(prev)), Some(StripRow::Tab(next)))
                if groups.member.get(next).is_none() =>
            {
                if let Some(gid) = groups.member.get(prev).cloned() {
                    return Dest::Join {
                        gid,
                        after: Some(*prev),
                    };
                }
                return Dest::Loose { after: Some(*prev) };
            }
            _ => {}
        }
    }
    dest_on_slot(groups, after_row, before_row)
}

fn dest_on_slot(
    groups: &Groups,
    after_row: Option<&StripRow>,
    before_row: Option<&StripRow>,
) -> Dest {
    match after_row {
        Some(StripRow::Header(_)) => Dest::Noop,
        Some(StripRow::Tab(next)) => {
            if let Some(gid) = groups.member.get(next).cloned() {
                Dest::Join { gid, after: None }
            } else {
                Dest::Loose {
                    after: match before_row {
                        Some(StripRow::Tab(prev)) if !groups.member.contains_key(prev) => {
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
                if let Some(gid) = groups.member.get(prev).cloned() {
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
    }
}

enum Token {
    Loose(TabId),
    Group(String),
}

fn strip_tokens(groups: &Groups, tabs: &[TabInfo]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for t in tabs {
        match groups.member.get(&t.id) {
            None => out.push(Token::Loose(t.id)),
            Some(gid) => {
                if seen.insert(gid.clone()) {
                    out.push(Token::Group(gid.clone()));
                }
            }
        }
    }
    out
}

fn expand_tokens(tokens: &[Token], tabs: &[TabInfo], groups: &Groups) -> Vec<TabInfo> {
    let mut out = Vec::with_capacity(tabs.len());
    for tok in tokens {
        match tok {
            Token::Loose(id) => {
                if let Some(t) = tabs.iter().find(|t| t.id == *id) {
                    out.push(t.clone());
                }
            }
            Token::Group(gid) => {
                for t in tabs {
                    if groups.member.get(&t.id).map(String::as_str) == Some(gid.as_str()) {
                        out.push(t.clone());
                    }
                }
            }
        }
    }
    out
}

fn parse_tab(id: &str) -> Option<TabId> {
    id.parse::<u64>().ok().map(TabId)
}

fn splice_before(tabs: &mut Vec<TabInfo>, tid: TabId, before: Option<TabId>, groups: &Groups) {
    let Some(pos) = tabs.iter().position(|t| t.id == tid) else {
        return;
    };
    let tab = tabs.remove(pos);
    let insert = if let Some(next) = before {
        tabs.iter().position(|t| t.id == next).unwrap_or(tabs.len())
    } else if let Some(gid) = groups.member.get(&tid) {
        tabs.iter()
            .rposition(|t| groups.member.get(&t.id).map(String::as_str) == Some(gid.as_str()))
            .map(|i| i + 1)
            .unwrap_or(tabs.len())
    } else {
        tabs.iter()
            .position(|t| !groups.member.contains_key(&t.id))
            .unwrap_or(tabs.len())
    };
    tabs.insert(insert.min(tabs.len()), tab);
}

fn new_group_id(existing: &[TabGroup]) -> String {
    loop {
        let id = format!("g{}", NEXT_GROUP.fetch_add(1, Ordering::Relaxed));
        if !existing.iter().any(|g| g.id == id) {
            return id;
        }
    }
}

/// After restore, skip past the highest `gN` already on disk so the
/// next [`Groups::new_group`] cannot reuse Research's id (and pull
/// every `g1` member into the new folder).
fn bump_id_counter(groups: &[TabGroup]) {
    let max = groups
        .iter()
        .filter_map(|g| g.id.strip_prefix('g')?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    let _ = NEXT_GROUP.fetch_max(max.saturating_add(1), Ordering::Relaxed);
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
    fn drag_onto_header_is_noop() {
        let (mut g, mut tabs) = setup();
        // drag loose 4 (idx 5) onto research header (idx 3) — title is
        // not a drop target; the tab stays put.
        g.apply_drop(&mut tabs, 5, 3);
        assert_eq!(g.of_tab(TabId(4)), None);
        assert_eq!(ids(&tabs), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn drop_bias_upper_half_is_pocket_above() {
        // from 6, 1.4 rows up → slot 5, still in the top half.
        assert_eq!(
            drop_bias(6, 100.0, 100.0 - 32.0 * 1.4, 32.0, 5),
            DropBias::PocketAbove
        );
        // Center of slot 5.
        assert_eq!(
            drop_bias(6, 100.0, 100.0 - 32.0 * 1.0, 32.0, 5),
            DropBias::OnSlot
        );
    }

    #[test]
    fn drag_to_floor_of_group_stays_in_group() {
        let (mut g, mut tabs) = setup();
        // rows: H w, 1, 2, H r, 3, 4, 5
        // drag loose 5 (idx 6) onto research header (idx 3), top half:
        // still the floor of Work, not join Research.
        g.apply_drop_with(&mut tabs, 6, 3, DropBias::PocketAbove);
        assert_eq!(g.of_tab(TabId(5)), Some("work"));
        assert_eq!(ids(&tabs), vec![1, 2, 5, 3, 4]);
    }

    #[test]
    fn drag_to_floor_of_last_group_does_not_ungroup() {
        let (mut g, mut tabs) = setup();
        // drag loose 5 (idx 6) onto first loose (idx 5), top half:
        // floor of Research, not the loose run.
        g.apply_drop_with(&mut tabs, 6, 5, DropBias::PocketAbove);
        assert_eq!(g.of_tab(TabId(5)), Some("research"));
        assert_eq!(ids(&tabs), vec![1, 2, 3, 5, 4]);
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
        assert_eq!(tabs.last().map(|t| t.id.0), Some(5));
    }

    #[test]
    fn kit_drop_group_into_loose_stays_mixed() {
        use sola_kit::components::{Dest, Drop};
        let (mut g, mut tabs) = setup();
        g.apply_kit_drop(
            &mut tabs,
            &Drop {
                id: "work".into(),
                dest: Dest::BlockBefore { before: None },
            },
        );
        assert_eq!(ids(&tabs), vec![3, 4, 5, 1, 2]);
        assert_eq!(
            g.visible_rows(&tabs),
            vec![
                StripRow::Header("research".into()),
                StripRow::Tab(TabId(3)),
                StripRow::Tab(TabId(4)),
                StripRow::Tab(TabId(5)),
                StripRow::Header("work".into()),
                StripRow::Tab(TabId(1)),
                StripRow::Tab(TabId(2)),
            ]
        );
    }

    #[test]
    fn new_group_after_restore_does_not_reuse_ids() {
        let session_tabs = vec![SessionTab {
            url: "https://a/".into(),
            title: "a".into(),
            group_id: Some("g1".into()),
            ..SessionTab::default()
        }];
        let g = Groups::restore(
            &session_tabs,
            &[TabId(1)],
            &[SessionGroup {
                id: "g1".into(),
                name: "Research".into(),
                collapsed: false,
            }],
        );
        let mut g = g;
        g.new_group(TabId(2));
        assert_ne!(g.groups[1].id, "g1");
        assert_eq!(g.of_tab(TabId(1)), Some("g1"));
        assert_eq!(
            g.of_tab(TabId(2)).map(str::to_string),
            Some(g.groups[1].id.clone())
        );
        assert_eq!(g.groups[0].name, "Research");
        assert_eq!(g.groups[1].name, "Group");
        // Each group lists only its own members (no doubled strip).
        let tabs = vec![tab(1, "a"), tab(2, "b")];
        let rows = g.visible_rows(&tabs);
        assert_eq!(
            rows,
            vec![
                StripRow::Header("g1".into()),
                StripRow::Tab(TabId(1)),
                StripRow::Header(g.groups[1].id.clone()),
                StripRow::Tab(TabId(2)),
            ]
        );
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
    fn kit_drop_join_and_loose() {
        use sola_kit::components::{Dest, Drop};
        let (mut g, mut tabs) = setup();
        g.apply_kit_drop(
            &mut tabs,
            &Drop {
                id: "4".into(),
                dest: Dest::Join {
                    section: "work".into(),
                    before: Some("2".into()),
                },
            },
        );
        assert_eq!(g.of_tab(TabId(4)), Some("work"));
        assert_eq!(ids(&tabs), vec![1, 4, 2, 3, 5]);

        g.apply_kit_drop(
            &mut tabs,
            &Drop {
                id: "1".into(),
                dest: Dest::Loose {
                    before: Some("5".into()),
                },
            },
        );
        assert_eq!(g.of_tab(TabId(1)), None);
        g.normalize(&mut tabs);
        let loose: Vec<u64> = tabs
            .iter()
            .filter(|t| g.of_tab(t.id).is_none())
            .map(|t| t.id.0)
            .collect();
        assert!(loose.contains(&1));
        assert!(loose.contains(&5));
    }

    #[test]
    fn kit_drop_loose_end_appends() {
        use sola_kit::components::{Dest, Drop};
        let (mut g, mut tabs) = setup();
        g.apply_kit_drop(
            &mut tabs,
            &Drop {
                id: "2".into(),
                dest: Dest::Loose { before: None },
            },
        );
        assert_eq!(g.of_tab(TabId(2)), None);
        assert_eq!(ids(&tabs).last().copied(), Some(2));
        assert_eq!(ids(&tabs), vec![1, 3, 4, 5, 2]);
    }

    #[test]
    fn kit_drop_sections_reorders_groups() {
        use sola_kit::components::{Dest, Drop};
        let (mut g, mut tabs) = setup();
        g.apply_kit_drop(
            &mut tabs,
            &Drop {
                id: "research".into(),
                dest: Dest::Sections(vec!["research".into(), "work".into()]),
            },
        );
        assert_eq!(
            g.groups.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            vec!["research", "work"]
        );
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
    fn append_loose_lands_at_bottom_even_when_active_is_grouped() {
        let (mut g, mut tabs) = setup();
        g.append_loose(&mut tabs, tab(9, "new"));
        assert!(g.of_tab(TabId(9)).is_none());
        g.normalize(&mut tabs);
        assert_eq!(ids(&tabs), vec![1, 2, 3, 4, 5, 9]);
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
