//! The workspaces themselves: their order and names, which one shows,
//! showing one, sending a window to one, and adding, renaming, reordering
//! and deleting them.

use std::time::{SystemTime, UNIX_EPOCH};

use sola_bus::topics::{Workspace, Zone};

use super::desk::Desk;
use super::{COMMON, Stacks};

impl Stacks {
    /// The workspaces in order.
    pub fn workspaces(&self) -> Vec<Workspace> {
        self.desks
            .iter()
            .map(|d| Workspace {
                id: d.id.clone(),
                name: d.name.clone(),
            })
            .collect()
    }

    pub fn showing(&self) -> usize {
        self.showing
    }

    pub fn showing_id(&self) -> &str {
        &self.desks[self.showing].id
    }

    pub fn workspace_count(&self) -> usize {
        self.desks.len()
    }

    /// The id of the workspace `window_id` is on: Common for a common window,
    /// though it shows on them all.
    pub fn workspace_of(&self, window_id: u32) -> Option<&str> {
        if self.is_common(window_id) {
            return Some(self.desks[COMMON].id.as_str());
        }
        self.desk_of(window_id).map(|d| self.desks[d].id.as_str())
    }

    /// The windows that are workspace `index`'s own (its tabs; not their
    /// dialogs): a common window is only Common's.
    pub fn windows_on(&self, index: usize) -> Vec<u32> {
        self.desks.get(index).map_or_else(Vec::new, |d| {
            d.bar
                .tabs()
                .map(|t| t.window_id)
                .filter(|w| index == COMMON || !self.is_common(*w))
                .collect()
        })
    }

    /// Show workspace `index`. Returns false when it is already showing or
    /// does not exist.
    pub fn show_workspace(&mut self, index: usize) -> bool {
        if index >= self.desks.len() || index == self.showing {
            return false;
        }
        self.showing = index;
        true
    }

    /// The workspace after (or before) the showing one, wrapping, passing
    /// over `skipped` (by id). The showing one when no other is left.
    pub fn step_workspace(&self, back: bool, skipped: &[String]) -> usize {
        let n = self.desks.len();
        (1..n)
            .map(|step| {
                if back {
                    (self.showing + n - step) % n
                } else {
                    (self.showing + step) % n
                }
            })
            .find(|&i| !skipped.contains(&self.desks[i].id))
            .unwrap_or(self.showing)
    }

    /// Send `window_id` to workspace `index`: the same zone there, at the
    /// end of its bar (of its app's group there, when it has one), shown
    /// (a float stays a float; Full or the halves come up with it).
    /// Returns false when nothing moved.
    ///
    /// A window sent to Common stays where it is and joins every other
    /// workspace too. A common window leaves only from Common, for one other
    /// workspace, which keeps its row; it is gone from the rest.
    pub fn send(&mut self, window_id: u32, index: usize) -> bool {
        let window_id = self.parent_of(window_id).unwrap_or(window_id);
        if index >= self.desks.len() {
            return false;
        }
        if self.is_common(window_id) {
            return self.send_common_away(window_id, index);
        }
        if index == COMMON {
            let Some(tab) = self.desk_of(window_id).and_then(|d| self.desks[d].tab(window_id)) else {
                return false;
            };
            let tab = tab.clone();
            self.desks[COMMON].push(tab, false);
            self.spread(window_id);
            return true;
        }
        if self.desk_of(window_id) == Some(index) {
            return false;
        }
        let Some((zone, _, tab, _)) = self.take(window_id) else {
            return false;
        };
        self.desks[index].push(tab, true);
        self.desks[index].pick(zone);
        true
    }

    /// A common window, sent from Common to workspace `index`: there it
    /// stays, shown, and it leaves every other. Only Common can let it go.
    fn send_common_away(&mut self, window_id: u32, index: usize) -> bool {
        if self.showing != COMMON || index == COMMON {
            return false;
        }
        for (i, desk) in self.desks.iter_mut().enumerate() {
            if i != index {
                desk.take(window_id);
            }
        }
        let desk = &mut self.desks[index];
        if let Some(zone) = desk.zone_of(window_id) {
            if zone != Zone::Float {
                desk.column_mut(zone).show(window_id);
            }
            desk.pick(zone);
        }
        true
    }

    /// Add a workspace after the showing one and show it, with Common's
    /// windows on it. Returns its index.
    pub fn add_workspace(&mut self, name: &str) -> usize {
        let id = self.fresh_id();
        let at = self.showing + 1;
        let mut desk = Desk::new(id, name, &self.layout.zones());
        for tab in self.desks[COMMON].bar.tabs() {
            desk.push(tab.clone(), false);
        }
        self.desks.insert(at, desk);
        self.showing = at;
        at
    }

    /// Rename workspace `index`. Common keeps its name.
    pub fn rename_workspace(&mut self, index: usize, name: &str) -> bool {
        let name = name.trim();
        if index == COMMON {
            return false;
        }
        match self.desks.get_mut(index) {
            Some(d) if !name.is_empty() && d.name != name => {
                d.name = name.to_string();
                true
            }
            _ => false,
        }
    }

    /// Move workspace `index` one place left (`back`) or right. Common stays
    /// first.
    pub fn move_workspace(&mut self, index: usize, back: bool) -> bool {
        let to = if back {
            index.checked_sub(1)
        } else {
            Some(index + 1).filter(|&i| i < self.desks.len())
        };
        let Some(to) = to else {
            return false;
        };
        if index >= self.desks.len() || index == COMMON || to == COMMON {
            return false;
        }
        let showing_id = self.showing_id().to_string();
        self.desks.swap(index, to);
        self.showing = self
            .desks
            .iter()
            .position(|d| d.id == showing_id)
            .unwrap_or(0);
        true
    }

    /// Move workspace `from` to place `to`, the ones between making room.
    /// Common stays first.
    pub fn reorder_workspace(&mut self, from: usize, to: usize) -> bool {
        let fixed = from == COMMON || to == COMMON;
        if fixed || from == to || from >= self.desks.len() || to >= self.desks.len() {
            return false;
        }
        let showing_id = self.showing_id().to_string();
        let desk = self.desks.remove(from);
        self.desks.insert(to, desk);
        self.showing = self
            .desks
            .iter()
            .position(|d| d.id == showing_id)
            .unwrap_or(0);
        true
    }

    /// Delete workspace `index`; its own windows join the workspace before
    /// it (the next, for the first) in the same zones, at the end of its bar.
    /// The last workspace stays, and Common is never deleted.
    pub fn delete_workspace(&mut self, index: usize) -> bool {
        if self.desks.len() < COMMON + 3 || index >= self.desks.len() || index == COMMON {
            return false;
        }
        let showing_id = self.showing_id().to_string();
        let gone = self.desks.remove(index);
        let heir = index.saturating_sub(1).max(COMMON + 1);
        // Common's windows are on the heir already.
        let own: Vec<&super::Tab> = gone
            .bar
            .tabs()
            .filter(|t| self.desks[COMMON].tab(t.window_id).is_none())
            .collect();
        let shown: Vec<bool> = own
            .iter()
            .map(|t| gone.column(t.zone).shown == Some(t.window_id))
            .collect();
        let tabs: Vec<super::Tab> = own.into_iter().cloned().collect();
        for (tab, shown) in tabs.into_iter().zip(shown) {
            self.desks[heir].push(tab, shown);
        }
        self.showing = self
            .desks
            .iter()
            .position(|d| d.id == showing_id)
            .unwrap_or(heir);
        true
    }

    /// A stable id no workspace has had in this session's memory: the
    /// time it was made.
    fn fresh_id(&self) -> String {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let mut n = millis;
        loop {
            let id = format!("ws-{n:x}");
            if self.desks.iter().all(|d| d.id != id) {
                return id;
            }
            n += 1;
        }
    }
}
