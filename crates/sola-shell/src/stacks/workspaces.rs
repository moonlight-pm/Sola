//! The workspaces themselves: their order and names, which one shows,
//! showing one, sending a window to one, and adding, renaming, reordering
//! and deleting them.

use std::time::{SystemTime, UNIX_EPOCH};

use sola_bus::topics::{Workspace, Zone};

use super::Stacks;
use super::desk::Desk;

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

    /// The id of the workspace `window_id` is on.
    pub fn workspace_of(&self, window_id: u32) -> Option<&str> {
        self.desk_of(window_id).map(|d| self.desks[d].id.as_str())
    }

    /// Every window on workspace `index` (its tabs; not their dialogs).
    pub fn windows_on(&self, index: usize) -> Vec<u32> {
        self.desks
            .get(index)
            .map_or_else(Vec::new, |d| d.all_tabs().map(|t| t.window_id).collect())
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

    /// The workspace after (or before) the showing one, wrapping.
    pub fn step_workspace(&self, back: bool) -> usize {
        let n = self.desks.len();
        if back {
            (self.showing + n - 1) % n
        } else {
            (self.showing + 1) % n
        }
    }

    /// Send `window_id` to workspace `index`: the same zone there, at the
    /// end, shown (a float stays a float). Returns false when nothing
    /// moved.
    pub fn send(&mut self, window_id: u32, index: usize) -> bool {
        let window_id = self.parent_of(window_id).unwrap_or(window_id);
        if index >= self.desks.len() || self.desk_of(window_id) == Some(index) {
            return false;
        }
        let Some((zone, _, tab, _)) = self.take(window_id) else {
            return false;
        };
        self.desks[index].push(tab, zone, true);
        if zone != Zone::Float {
            self.desks[index].last_tiled = zone;
        }
        true
    }

    /// Add a workspace after the showing one and show it. Returns its
    /// index.
    pub fn add_workspace(&mut self, name: &str) -> usize {
        let id = self.fresh_id();
        let at = self.showing + 1;
        self.desks.insert(at, Desk::new(id, name));
        self.showing = at;
        at
    }

    pub fn rename_workspace(&mut self, index: usize, name: &str) -> bool {
        let name = name.trim();
        match self.desks.get_mut(index) {
            Some(d) if !name.is_empty() && d.name != name => {
                d.name = name.to_string();
                true
            }
            _ => false,
        }
    }

    /// Move workspace `index` one place left (`back`) or right.
    pub fn move_workspace(&mut self, index: usize, back: bool) -> bool {
        let to = if back {
            index.checked_sub(1)
        } else {
            Some(index + 1).filter(|&i| i < self.desks.len())
        };
        let Some(to) = to else {
            return false;
        };
        if index >= self.desks.len() {
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

    /// Delete workspace `index`; its windows join the workspace before it
    /// (the next, for the first), at the ends of the same zones. The last
    /// workspace stays.
    pub fn delete_workspace(&mut self, index: usize) -> bool {
        if self.desks.len() < 2 || index >= self.desks.len() {
            return false;
        }
        let showing_id = self.showing_id().to_string();
        let gone = self.desks.remove(index);
        let heir = index.saturating_sub(1);
        for col in gone.columns {
            for tab in col.tabs {
                let shown = col.shown == Some(tab.window_id);
                self.desks[heir].push(tab, col.zone, shown);
            }
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
