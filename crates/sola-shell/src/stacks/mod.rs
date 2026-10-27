//! The tab bar's model (Next, `docs/specs/2026-10-21-next-tab-bar-design.md`):
//! the workspaces, which one shows, and on each the zone every window is
//! in, each zone's tabs in bar order, and which tab each tiled zone shows.
//! Pure — the shell turns it into `Composition` and `Frame`s and draws the
//! bar from it.
//!
//! A tiled zone shows one tab; the rest stay out of the composition, so
//! River hides them. Full and the two halves take turns on screen: picking
//! a tab in one leaves the other out. Every float shows. Only the showing
//! workspace is composed. A window with a parent (a dialog) is no tab: it
//! rides above its parent, on its parent's workspace.
//!
//! The readers without a window (`tabs`, `shown`, `visible_shown`,
//! `on_screen`, `front`, `cinema`, `float_order`) answer for the showing
//! workspace; everything given a window finds it on whichever workspace it
//! is.

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

/// The zone that covers the whole content area. It and [`HALVES`] take
/// turns on screen.
pub const FULL: Zone = Zone::Fullscreen;

/// The halves, left to right, and their shares of the content area (they
/// add up to one). canto (1920 × 1080) has two; configurable before Next
/// goes back to novus.
pub const HALVES: [(Zone, f32); 2] = [(Zone::Left, 0.5), (Zone::Right, 0.5)];

/// The zones that show one tab at a time.
pub const TILED: [Zone; 3] = [FULL, Zone::Left, Zone::Right];

/// Every zone the bar lists, in bar order.
pub const ZONES: [Zone; 4] = [FULL, Zone::Left, Zone::Right, Zone::Float];

/// The workspace a fresh desk starts with.
pub const FIRST_WORKSPACE: (&str, &str) = ("main", "Main");

/// How long a departed window's place waits for its app to come back
/// (a restart, a re-exec after install).
const GHOST_GRACE: Duration = Duration::from_secs(30);

/// How long the saved layout waits for the session to restore its apps.
const RESTORE_GRACE: Duration = Duration::from_secs(120);

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
    /// The tab with keyboard focus: a new window opens right after it.
    focused: Option<u32>,
    /// The focused tab and the last window opened from it while it kept
    /// focus (tabs opened behind): the next one goes after that one, so a
    /// run of them stays in the order they were opened.
    opened: Option<(u32, u32)>,
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
            focused: None,
            opened: None,
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
    /// `floats`, else the showing workspace's landing zone (shown): Full
    /// while it is up or nothing is tiled, else the last focused half. It
    /// goes right after the focused tab when that tab is in the zone (after
    /// the last window opened from it, while it kept focus), else at the
    /// end. `returning` windows (the ones present when the shell starts,
    /// and tabs opened behind) take a remembered place if there is one
    /// and otherwise join without being shown over the zone's tab.
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
        let (zone, at, opener) = self.landing_slot(floats);
        let desk = &mut self.desks[self.showing];
        let show = !(returning && desk.column(zone).shown.is_some());
        let shown = desk.insert(tab, zone, at, show);
        if shown {
            desk.pick(zone);
        }
        self.opened = opener.map(|f| (f, window_id));
        Placed {
            zone,
            shown,
            returning,
        }
    }

    /// Where [`Self::place`] puts a new window that has no remembered slot:
    /// the zone (Float, or the showing workspace's landing zone), the index
    /// there — right after the focused tab, after the last window opened
    /// from it while it kept focus — and that focused tab (the opener).
    /// The launcher's ghost row stands there while an app starts.
    pub fn landing_slot(&self, floats: bool) -> (Zone, usize, Option<u32>) {
        let desk = &self.desks[self.showing];
        let zone = if floats { Zone::Float } else { desk.landing() };
        let tabs = &desk.column(zone).tabs;
        let index_of = |w: u32| tabs.iter().position(|t| t.window_id == w);
        let opener = self.focused.filter(|&f| index_of(f).is_some());
        let after = match (opener, self.opened) {
            (Some(f), Some((from, last))) if from == f && index_of(last).is_some() => Some(last),
            _ => opener,
        };
        let at = after.and_then(index_of).map_or(tabs.len(), |i| i + 1);
        (zone, at, opener)
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
            col.show(window_id);
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
    /// come back. A zone whose shown tab left shows the tab it showed
    /// before (else the next, else the previous). Returns the zone it was
    /// in.
    pub fn remove(&mut self, window_id: u32, now: Instant) -> Option<Zone> {
        self.parents.retain(|child, parent| *child != window_id && *parent != window_id);
        if self.focused == Some(window_id) {
            self.focused = None;
        }
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
    /// parent, for a dialog), and its workspace. A tab in Full puts Full on
    /// screen; one in a half, the halves. Returns false when it is not
    /// placed.
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
            desk.column_mut(zone).show(window_id);
            desk.pick(zone);
        }
        true
    }

    /// Keyboard focus moved to `window_id`: with the halves up, a new
    /// window joins its half from now on. Focus never switches between
    /// Full and the halves; only showing a tab does.
    pub fn focus(&mut self, window_id: u32) {
        let window_id = self.parent_of(window_id).unwrap_or(window_id);
        let Some(d) = self.desk_of(window_id) else {
            return;
        };
        self.focused = Some(window_id);
        if self.opened.is_some_and(|(from, _)| from != window_id) {
            self.opened = None;
        }
        let desk = &mut self.desks[d];
        if let Some(zone) = desk.zone_of(window_id).filter(|z| is_half(*z)) {
            desk.last_half = zone;
        }
    }

    /// Move `window_id` to `zone` on its workspace, in front of `before`
    /// (else at the end), and show it there (Full, or the halves, comes on
    /// screen with it). Returns false when nothing moved.
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
            col.show(window_id);
            desk.pick(zone);
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

    /// The window group (zone) after (or before) `from`'s on the showing
    /// workspace, wrapping, skipping groups with nothing to show — and what
    /// it shows: a tiled zone's shown tab, the top float. Without `from` (or
    /// from a window in no group), the first (the last, `back`). `None` when
    /// no other group has anything to show.
    pub fn step_group(
        &self,
        from: Option<u32>,
        back: bool,
        hidden: &dyn Fn(&str) -> bool,
    ) -> Option<u32> {
        let desk = &self.desks[self.showing];
        let groups: Vec<(Zone, u32)> = ZONES
            .iter()
            .filter_map(|&zone| {
                let front = if zone == Zone::Float {
                    desk.float_z.iter().rev().copied().find(|&w| {
                        desk.tab(w).is_some_and(|t| !hidden(&t.app_id))
                    })
                } else {
                    desk.visible_shown(zone, hidden)
                };
                front.map(|w| (zone, w))
            })
            .collect();
        let from = from.map(|w| self.parent_of(w).unwrap_or(w));
        let here = from
            .and_then(|w| desk.zone_of(w))
            .and_then(|z| groups.iter().position(|(g, _)| *g == z));
        let n = groups.len();
        let next = match here {
            Some(_) if n < 2 => return None,
            Some(i) if back => (i + n - 1) % n,
            Some(i) => (i + 1) % n,
            None if back => n.checked_sub(1)?,
            None => 0,
        };
        groups.get(next).map(|(_, w)| *w)
    }

    /// Toggle Cinema (the whole output, menubar and bar included) for
    /// `window_id`. Returns whether it is in Cinema now.
    pub fn toggle_cinema(&mut self, window_id: u32) -> bool {
        let Some(d) = self.desk_of(window_id) else {
            return false;
        };
        let desk = &mut self.desks[d];
        desk.cinema = (desk.cinema != Some(window_id)).then_some(window_id);
        desk.cinema.is_some()
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

    /// The tab in Cinema on the showing workspace.
    pub fn cinema(&self) -> Option<u32> {
        self.cinema_on(self.showing)
    }

    /// What a tiled zone of the showing workspace puts on screen: its
    /// shown tab, or, when that tab's app is hidden (Super+H), the nearest
    /// tab that is not.
    pub fn visible_shown(&self, zone: Zone, hidden: &dyn Fn(&str) -> bool) -> Option<u32> {
        self.desks[self.showing].visible_shown(zone, hidden)
    }

    /// The tiled zones of the showing workspace on screen: Full alone, or
    /// the halves with something to show.
    pub fn on_screen(&self, hidden: &dyn Fn(&str) -> bool) -> Vec<Zone> {
        self.desks[self.showing].on_screen(hidden)
    }

    /// Where focus belongs on the showing workspace when the focused tab
    /// left the screen: Full's tab, or the last focused half's (else the
    /// other half's).
    pub fn front(&self, hidden: &dyn Fn(&str) -> bool) -> Option<u32> {
        self.desks[self.showing].front(hidden)
    }

    // ---------------------------------------------------------------------
    // Reading: any workspace, any window
    // ---------------------------------------------------------------------

    pub fn tabs_on(&self, workspace: usize, zone: Zone) -> &[Tab] {
        self.desks
            .get(workspace)
            .map_or(&[], |d| d.column(zone).tabs.as_slice())
    }

    pub fn cinema_on(&self, workspace: usize) -> Option<u32> {
        self.desks.get(workspace).and_then(|d| d.cinema)
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

fn is_half(zone: Zone) -> bool {
    HALVES.iter().any(|(z, _)| *z == zone)
}
