//! The tab bar's model (Next, `docs/specs/2026-10-21-next-tab-bar-design.md`):
//! the workspaces, which one shows, and on each the zone every window is
//! in, each zone's tabs in bar order, and which tab each tiled zone shows.
//! Pure — the shell turns it into `Composition` and `Frame`s and draws the
//! bar from it.
//!
//! A tiled zone shows one tab; the rest stay out of the composition, so
//! River hides them. Every float shows. Only the showing workspace is
//! composed. A window with a parent (a dialog) is no tab: it rides above
//! its parent, on its parent's workspace.
//!
//! The readers without a window (`tabs`, `shown`, `visible_shown`,
//! `occupied`, `present`, `float_order`) answer for the showing workspace;
//! everything given a window finds it on whichever workspace it is.

mod desk;
mod saved;
#[cfg(test)]
mod tests;
mod workspaces;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use sola_bus::topics::Zone;

use desk::Desk;
use saved::Slot;

/// The tiled zones, left to right, and their share of the content area.
/// canto (1920 × 1080) has two halves; configurable before Next goes back
/// to novus.
pub const TILED: [(Zone, f32); 2] = [(Zone::Left, 0.5), (Zone::Right, 0.5)];

/// Every zone the bar lists, in bar order.
pub const ZONES: [Zone; 3] = [Zone::Left, Zone::Right, Zone::Float];

/// The workspace a fresh desk starts with.
pub const FIRST_WORKSPACE: (&str, &str) = ("main", "Main");

/// How long a departed window's place waits for its app to come back
/// (a restart, a re-exec after install).
const GHOST_GRACE: Duration = Duration::from_secs(30);

/// How long the saved layout waits for the session to restore its apps.
const RESTORE_GRACE: Duration = Duration::from_secs(120);

/// Ways to show a tab bigger than its zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Present {
    /// The whole content area (both zones), under the menubar.
    Fullscreen,
    /// The whole output, menubar and bar included.
    Cinema,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub window_id: u32,
    pub app_id: String,
    /// What its place is remembered by: the tab key an app announced
    /// (`Topic::Tab`), else the app id.
    pub key: String,
}

/// Where [`Stacks::place`] put a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub zone: Zone,
    /// The zone shows it now (always true for a float).
    pub shown: bool,
    /// It took a remembered place (restored, or back from a restart), so
    /// it should not take focus.
    pub returning: bool,
}

pub struct Stacks {
    /// In workspace order; never empty.
    desks: Vec<Desk>,
    showing: usize,
    /// child → parent.
    parents: HashMap<u32, u32>,
    /// Places from the saved layout not yet taken.
    restore: Vec<Slot>,
    restore_until: Option<Instant>,
    /// Places of windows that just left.
    ghosts: Vec<(Slot, Instant)>,
    /// Saved index of each restored window, so windows that come back in
    /// any order still line up as saved.
    ranks: HashMap<u32, usize>,
}

impl Default for Stacks {
    fn default() -> Self {
        Self::new()
    }
}

impl Stacks {
    pub fn new() -> Self {
        Self {
            desks: vec![Desk::new(FIRST_WORKSPACE.0, FIRST_WORKSPACE.1)],
            showing: 0,
            parents: HashMap::new(),
            restore: Vec::new(),
            restore_until: None,
            ghosts: Vec::new(),
            ranks: HashMap::new(),
        }
    }

    /// Forget every placed window on every workspace (keeps the workspaces
    /// and the saved layout), so the shell can place them again after
    /// [`Self::load`].
    pub fn clear(&mut self) {
        for d in &mut self.desks {
            d.clear();
        }
        self.ranks.clear();
    }

    // ---------------------------------------------------------------------
    // Windows arriving and leaving
    // ---------------------------------------------------------------------

    /// Place a new window. In order: the place its key just left, its
    /// key's saved place (for a tab with neither, its app's), Floating for
    /// `floats`, else the end of the showing workspace's last focused tiled
    /// zone (shown). `returning` windows (the ones present when the shell
    /// starts) take a remembered place if there is one and otherwise join
    /// without being shown over the zone's tab.
    pub fn place(
        &mut self,
        window_id: u32,
        app_id: &str,
        key: &str,
        floats: bool,
        returning: bool,
        now: Instant,
    ) -> Placed {
        self.take(window_id);
        let tab = Tab {
            window_id,
            app_id: app_id.to_string(),
            key: key.to_string(),
        };
        let keys: &[&str] = if key == app_id { &[key] } else { &[key, app_id] };
        for k in keys {
            if let Some(slot) = self.claim_ghost(k, now) {
                return self.insert_slot(tab, &slot, None);
            }
            if let Some(slot) = self.claim_restore(k, now) {
                let at = slot.at;
                return self.insert_slot(tab, &slot, Some(at));
            }
        }
        let desk = &mut self.desks[self.showing];
        let zone = if floats { Zone::Float } else { desk.last_tiled };
        let show = !(returning && desk.column(zone).shown.is_some());
        let shown = desk.push(tab, zone, show);
        Placed {
            zone,
            shown,
            returning,
        }
    }

    fn insert_slot(&mut self, tab: Tab, slot: &Slot, rank: Option<usize>) -> Placed {
        let window_id = tab.window_id;
        let d = self.desk_index(&slot.workspace).unwrap_or(self.showing);
        let ranks = &self.ranks;
        let desk = &mut self.desks[d];
        let col = desk.column_mut(slot.zone);
        let index = match rank {
            // Before the first tab saved after it (or added since).
            Some(r) => col
                .tabs
                .iter()
                .position(|t| ranks.get(&t.window_id).is_none_or(|&other| other > r))
                .unwrap_or(col.tabs.len()),
            None => slot.at.min(col.tabs.len()),
        };
        col.tabs.insert(index, tab);
        let mut shown = true;
        if slot.zone == Zone::Float {
            desk.float_z.push(window_id);
        } else if slot.shown || col.shown.is_none() {
            col.shown = Some(window_id);
        } else {
            shown = false;
        }
        if let Some(r) = rank {
            self.ranks.insert(window_id, r);
        }
        Placed {
            zone: slot.zone,
            shown,
            returning: true,
        }
    }

    /// `window_id` turns out to be the tab `key` (paired after it was
    /// placed). It is remembered by that key from now on, and when a place
    /// is waiting for the key (a restore, a restart) it moves there.
    pub fn set_key(&mut self, window_id: u32, key: &str, now: Instant) -> Option<Placed> {
        let d = self.desk_of(window_id)?;
        let tab = self.desks[d]
            .columns
            .iter_mut()
            .flat_map(|c| c.tabs.iter_mut())
            .find(|t| t.window_id == window_id)?;
        if tab.key == key {
            return None;
        }
        tab.key = key.to_string();
        let app_id = tab.app_id.clone();
        if !self.waiting(key, now) {
            return None;
        }
        let floats = self.zone_of(window_id) == Some(Zone::Float);
        Some(self.place(window_id, &app_id, key, floats, true, now))
    }

    /// A window closed. Its place waits [`GHOST_GRACE`] for its app to
    /// come back. A zone whose shown tab left shows the next tab (else
    /// the previous). Returns the zone it was in.
    pub fn remove(&mut self, window_id: u32, now: Instant) -> Option<Zone> {
        self.parents.retain(|child, parent| *child != window_id && *parent != window_id);
        let workspace = self.desks[self.desk_of(window_id)?].id.clone();
        let (zone, at, tab, was_shown) = self.take(window_id)?;
        self.ghosts.push((
            Slot {
                workspace,
                zone,
                key: tab.key,
                at,
                shown: was_shown,
            },
            now,
        ));
        Some(zone)
    }

    /// Take a window out of its workspace without leaving a ghost (it is
    /// moving, or became a dialog).
    fn take(&mut self, window_id: u32) -> Option<(Zone, usize, Tab, bool)> {
        self.ranks.remove(&window_id);
        let d = self.desk_of(window_id)?;
        self.desks[d].take(window_id)
    }

    /// `child` is a dialog of `parent`: it leaves any zone and rides above
    /// its parent. Returns true when it was a tab.
    pub fn set_parent(&mut self, child: u32, parent: u32) -> bool {
        self.parents.insert(child, parent);
        self.take(child).is_some()
    }

    /// `child` has no parent any more.
    pub fn clear_parent(&mut self, child: u32) -> bool {
        self.parents.remove(&child).is_some()
    }

    pub fn parent_of(&self, child: u32) -> Option<u32> {
        self.parents.get(&child).copied()
    }

    /// Dialogs of `parent`, in the order they arrived.
    pub fn children_of(&self, parent: u32) -> Vec<u32> {
        let mut kids: Vec<u32> = self
            .parents
            .iter()
            .filter(|(_, p)| **p == parent)
            .map(|(c, _)| *c)
            .collect();
        kids.sort_unstable();
        kids
    }

    // ---------------------------------------------------------------------
    // Moving between tabs
    // ---------------------------------------------------------------------

    /// Show `window_id` in its zone (raise it, for a float; show its
    /// parent, for a dialog), and its workspace. Returns false when it is
    /// not placed.
    pub fn show(&mut self, window_id: u32) -> bool {
        let window_id = self.parent_of(window_id).unwrap_or(window_id);
        let Some(d) = self.desk_of(window_id) else {
            return false;
        };
        self.showing = d;
        let desk = &mut self.desks[d];
        let Some(zone) = desk.zone_of(window_id) else {
            return false;
        };
        if zone == Zone::Float {
            desk.float_z.retain(|&w| w != window_id);
            desk.float_z.push(window_id);
        } else {
            desk.column_mut(zone).shown = Some(window_id);
            desk.last_tiled = zone;
        }
        true
    }

    /// Keyboard focus moved to `window_id`: a new window joins its zone
    /// from now on.
    pub fn focus(&mut self, window_id: u32) {
        let window_id = self.parent_of(window_id).unwrap_or(window_id);
        let Some(d) = self.desk_of(window_id) else {
            return;
        };
        let desk = &mut self.desks[d];
        if let Some(zone) = desk.zone_of(window_id).filter(|z| *z != Zone::Float) {
            desk.last_tiled = zone;
        }
    }

    /// Move `window_id` to `zone` on its workspace, in front of `before`
    /// (else at the end), and show it there. Returns false when nothing
    /// moved.
    pub fn move_to(&mut self, window_id: u32, zone: Zone, before: Option<u32>) -> bool {
        if !ZONES.contains(&zone) || before == Some(window_id) {
            return false;
        }
        let Some(d) = self.desk_of(window_id) else {
            return false;
        };
        let Some((_, _, tab, _)) = self.take(window_id) else {
            return false;
        };
        let desk = &mut self.desks[d];
        let col = desk.column_mut(zone);
        let index = before
            .and_then(|b| col.tabs.iter().position(|t| t.window_id == b))
            .unwrap_or(col.tabs.len());
        col.tabs.insert(index, tab);
        if zone == Zone::Float {
            desk.float_z.push(window_id);
        } else {
            col.shown = Some(window_id);
            desk.last_tiled = zone;
        }
        true
    }

    /// The tab after (or before) `window_id` in its zone, wrapping.
    pub fn step(&self, window_id: u32, back: bool) -> Option<u32> {
        let window_id = self.parent_of(window_id).unwrap_or(window_id);
        let desk = &self.desks[self.desk_of(window_id)?];
        let tabs = &desk.column(desk.zone_of(window_id)?).tabs;
        if tabs.len() < 2 {
            return None;
        }
        let i = tabs.iter().position(|t| t.window_id == window_id)?;
        let n = tabs.len();
        let next = if back { (i + n - 1) % n } else { (i + 1) % n };
        Some(tabs[next].window_id)
    }

    /// Toggle `present` for `window_id`. Returns what it is now.
    pub fn toggle_present(&mut self, window_id: u32, present: Present) -> Option<Present> {
        let d = self.desk_of(window_id)?;
        let desk = &mut self.desks[d];
        desk.present = match desk.present {
            Some((w, p)) if w == window_id && p == present => None,
            _ => Some((window_id, present)),
        };
        desk.present.map(|(_, p)| p)
    }

    // ---------------------------------------------------------------------
    // Reading: the showing workspace
    // ---------------------------------------------------------------------

    pub fn tabs(&self, zone: Zone) -> &[Tab] {
        self.tabs_on(self.showing, zone)
    }

    pub fn shown(&self, zone: Zone) -> Option<u32> {
        self.desks[self.showing].column(zone).shown
    }

    /// Floats on the showing workspace, bottom → top.
    pub fn float_order(&self) -> &[u32] {
        &self.desks[self.showing].float_z
    }

    pub fn present(&self) -> Option<(u32, Present)> {
        self.present_on(self.showing)
    }

    /// What a tiled zone of the showing workspace puts on screen: its
    /// shown tab, or, when that tab's app is hidden (Super+H), the nearest
    /// tab that is not.
    pub fn visible_shown(&self, zone: Zone, hidden: &dyn Fn(&str) -> bool) -> Option<u32> {
        self.desks[self.showing].visible_shown(zone, hidden)
    }

    /// Tiled zones of the showing workspace with something on screen.
    pub fn occupied(&self, hidden: &dyn Fn(&str) -> bool) -> Vec<Zone> {
        self.occupied_on(self.showing, hidden)
    }

    // ---------------------------------------------------------------------
    // Reading: any workspace, any window
    // ---------------------------------------------------------------------

    pub fn tabs_on(&self, workspace: usize, zone: Zone) -> &[Tab] {
        self.desks
            .get(workspace)
            .map_or(&[], |d| d.column(zone).tabs.as_slice())
    }

    pub fn present_on(&self, workspace: usize) -> Option<(u32, Present)> {
        self.desks.get(workspace).and_then(|d| d.present)
    }

    pub fn occupied_on(&self, workspace: usize, hidden: &dyn Fn(&str) -> bool) -> Vec<Zone> {
        self.desks
            .get(workspace)
            .map_or_else(Vec::new, |d| d.occupied(hidden))
    }

    /// The zone `window_id` is in, on whichever workspace.
    pub fn zone_of(&self, window_id: u32) -> Option<Zone> {
        self.desks.iter().find_map(|d| d.zone_of(window_id))
    }

    /// Floats and dialogs: the shell does not size them.
    pub fn is_floating(&self, window_id: u32) -> bool {
        self.parents.contains_key(&window_id) || self.zone_of(window_id) == Some(Zone::Float)
    }

    /// The index of the workspace `window_id` is on (a dialog: its
    /// parent's).
    pub fn desk_of(&self, window_id: u32) -> Option<usize> {
        let window_id = self.parent_of(window_id).unwrap_or(window_id);
        self.desks.iter().position(|d| d.tab(window_id).is_some())
    }

    /// Whether `window_id` is on the showing workspace (a window the bar
    /// has not placed counts as showing).
    pub fn on_showing(&self, window_id: u32) -> bool {
        self.desk_of(window_id).is_none_or(|d| d == self.showing)
    }

    fn desk_index(&self, id: &str) -> Option<usize> {
        self.desks.iter().position(|d| d.id == id)
    }
}
