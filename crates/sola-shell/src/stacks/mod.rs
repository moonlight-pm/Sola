//! The tab bar's model (Next, `docs/specs/2026-10-21-next-tab-bar-design.md`):
//! the workspaces, which one shows, and on each every window in bar order
//! with the zone it is in, and which tab each tiled zone shows. The bar
//! lists a workspace's windows in one run, not by zone: loose rows and
//! app groups (`bar`; spec § App groups and keys). Pure — the shell turns
//! it into `Composition` and `Frame`s and draws the bar from it.
//!
//! A tiled zone shows one tab; the rest stay out of the composition, so
//! River hides them. The zones come from the layout (`Topic::ZoneLayout`,
//! `docs/specs/2026-10-30-zone-layout-design.md`): the grid's, Full and
//! Cinema; zones that overlap take turns on screen (`Desk::on_screen`).
//! Every float shows, except over Cinema. Only the showing workspace is
//! composed. A window with a parent (a dialog) is no tab: it
//! rides above its parent, on its parent's workspace.
//!
//! The readers without a window (`bar`, `shown`, `visible_shown`,
//! `on_screen`, `front`, `cinema_up`, `float_order`) answer for the showing
//! workspace; everything given a window finds it on whichever workspace it
//! is.

mod bar;
#[cfg(test)]
mod pin_tests;
mod desk;
#[cfg(test)]
mod group_tests;
mod saved;
#[cfg(test)]
mod tests;
mod workspaces;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use sola_bus::topics::{Zone, ZoneLayout};

use desk::Desk;
use saved::Slot;

pub use bar::{Block, BlockId, grouped};

/// The zone that covers the whole content area (every cell of the grid):
/// where a workspace's first window goes, and where a gone zone's go.
pub const FULL: Zone = Zone::Fullscreen;

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
    pub zone: Zone,
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
    /// The zones: the grid's, Full, Cinema, Floating.
    layout: ZoneLayout,
    /// In workspace order; never empty.
    desks: Vec<Desk>,
    showing: usize,
    /// Windows on every workspace: a row on each, in one zone, with each
    /// workspace's own order and what it shows.
    pinned: HashSet<u32>,
    /// child → parent.
    parents: HashMap<u32, u32>,
    /// Places from the saved layout not yet taken.
    restore: Vec<Slot>,
    restore_until: Option<Instant>,
    /// Places of windows that just left.
    ghosts: Vec<(Slot, Instant)>,
    /// Apps one of whose windows took a remembered place by the app's id
    /// (no tab key): that window. While it is open and the saved layout is
    /// being restored, a window of the app that finds no place of its own
    /// joins it: Steam maps a short-lived window first, which takes
    /// Steam's place, and its main window after.
    came_back: HashMap<String, u32>,
    /// The tab with keyboard focus: a new window opens right after it.
    focused: Option<u32>,
    /// The focused tab and the last window opened from it while it kept
    /// focus (tabs opened behind): the next one goes after that one, so a
    /// run of them stays in the order they were opened.
    opened: Option<(u32, u32)>,
    /// The zone a tab in Cinema came from: an app leaving fullscreen goes
    /// back there.
    cinema_from: HashMap<u32, Zone>,
}

impl Default for Stacks {
    fn default() -> Self {
        Self::new()
    }
}

impl Stacks {
    pub fn new() -> Self {
        let layout = ZoneLayout::default();
        Self {
            desks: vec![Desk::new(FIRST_WORKSPACE.0, FIRST_WORKSPACE.1, &layout.zones())],
            layout,
            showing: 0,
            pinned: HashSet::new(),
            parents: HashMap::new(),
            restore: Vec::new(),
            restore_until: None,
            ghosts: Vec::new(),
            came_back: HashMap::new(),
            focused: None,
            opened: None,
            cinema_from: HashMap::new(),
        }
    }

    pub fn zone_layout(&self) -> &ZoneLayout {
        &self.layout
    }

    /// Take a new zone layout. Every workspace gets columns for new zones;
    /// a gone zone's tabs move to the end of Full. Returns whether it
    /// changed.
    pub fn set_layout(&mut self, layout: ZoneLayout) -> bool {
        if layout == self.layout {
            return false;
        }
        self.layout = layout;
        let zones = self.layout.zones();
        for d in &mut self.desks {
            d.set_zones(&zones);
        }
        self.cinema_from.retain(|_, z| zones.contains(z));
        true
    }

    /// Forget every placed window on every workspace (keeps the workspaces
    /// and the saved layout), so the shell can place them again after
    /// [`Self::load`].
    pub fn clear(&mut self) {
        for d in &mut self.desks {
            d.clear();
        }
        self.pinned.clear();
    }

    // ---------------------------------------------------------------------
    // Windows arriving and leaving
    // ---------------------------------------------------------------------

    /// Place a new window. In order: the place its key just left, its
    /// key's saved place (for a tab with neither, its app's), beside a
    /// window of its app that took its app's place (a window with no tab
    /// key, while the layout is restored: on that workspace, in that zone
    /// or Floating), Floating for `floats`, else the showing workspace's
    /// landing zone (shown): Full
    /// while it is up or nothing is tiled, else the last focused half. In
    /// the bar it goes right after the focused tab, whatever that tab's
    /// zone (after the last window opened from it, while it kept focus),
    /// else at the end. `returning` windows (the ones present when the shell starts,
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
        self.place_near(window_id, app_id, key, floats, returning, &[], None, now)
    }

    /// [`Self::place`], for a window that belongs next to `kin` (a Projects
    /// session beside its project's others): with no remembered place and
    /// its opener not among them, it lands after the last of them. `opener`
    /// is the window that asked for this one (a session starting a session):
    /// with no remembered place it lands on that window's workspace, opened
    /// from there, instead of the showing one's focused tab.
    #[allow(clippy::too_many_arguments)]
    pub fn place_near(
        &mut self,
        window_id: u32,
        app_id: &str,
        key: &str,
        floats: bool,
        returning: bool,
        kin: &[u32],
        opener: Option<u32>,
        now: Instant,
    ) -> Placed {
        // Placed again (its tab's place came up): still pinned.
        let was_pinned = self.pinned.remove(&window_id);
        self.take(window_id);
        let placed = self.place_tab(window_id, app_id, key, floats, returning, kin, opener, now);
        if was_pinned {
            self.pinned.insert(window_id);
            self.spread(window_id);
        }
        placed
    }

    #[allow(clippy::too_many_arguments)]
    fn place_tab(
        &mut self,
        window_id: u32,
        app_id: &str,
        key: &str,
        floats: bool,
        returning: bool,
        kin: &[u32],
        opener: Option<u32>,
        now: Instant,
    ) -> Placed {
        let tab = |zone| Tab {
            window_id,
            app_id: app_id.to_string(),
            key: key.to_string(),
            zone,
        };
        let keys: &[&str] = if key == app_id { &[key] } else { &[key, app_id] };
        for k in keys {
            let by_app = *k == app_id;
            if let Some(slot) = self.claim_ghost(k, now) {
                if by_app {
                    self.came_back.insert(app_id.to_string(), window_id);
                }
                let copies = self.claim_ghost_copies(&slot, now);
                return self.insert_slot(tab(slot.zone), &slot, &copies);
            }
            if let Some(slot) = self.claim_restore(k, now) {
                if by_app {
                    self.came_back.insert(app_id.to_string(), window_id);
                }
                let copies = self.claim_restore_copies(&slot);
                return self.insert_slot(tab(slot.zone), &slot, &copies);
            }
        }
        if key == app_id {
            if let Some(placed) = self.follow(tab(Zone::Float), floats, now) {
                return placed;
            }
        }
        let (d, from) = match opener.and_then(|o| Some((self.desk_of(o)?, o))) {
            Some((d, o)) => (d, Some(o)),
            None => (self.showing, self.focused),
        };
        let (zone, mut at, opener) = self.landing_on(d, from, app_id, floats);
        let desk = &mut self.desks[d];
        let opener_is_kin = opener.is_some_and(|o| kin.contains(&o));
        if !opener_is_kin {
            if let Some(end) = kin.iter().filter_map(|&w| desk.bar.index_of(w)).max() {
                at = end + 1;
            }
        }
        let show = !(returning && desk.column(zone).shown.is_some());
        let shown = desk.insert(tab(zone), at, show);
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

    /// Where [`Self::place`] puts a new window of `app_id` that has no
    /// remembered slot: the zone (Float, or the showing workspace's landing
    /// zone), the index in the bar, and the focused tab (the opener). Loose
    /// rows come first, then the groups. In the bar a window goes right
    /// after the opener — after the last window opened from it while it
    /// kept focus — when that is in its group; otherwise a grouped window
    /// goes to the end of its group (its emptied group's place, when it
    /// left one), the first of a group right after the group holding the
    /// opener (the top of the groups, from a loose row), and a loose window
    /// right after a loose opener, else at the end of the loose rows. The
    /// launcher's ghost row stands there while an app starts.
    pub fn landing_slot(&self, app_id: &str, floats: bool) -> (Zone, usize, Option<u32>) {
        self.landing_on(self.showing, self.focused, app_id, floats)
    }

    /// [`Self::landing_slot`] on workspace `d`, opened from `from`.
    fn landing_on(
        &self,
        d: usize,
        from: Option<u32>,
        app_id: &str,
        floats: bool,
    ) -> (Zone, usize, Option<u32>) {
        let desk = &self.desks[d];
        let zone = if floats { Zone::Float } else { desk.landing(&self.layout) };
        let bar = &desk.bar;
        let opener = from.filter(|&f| bar.index_of(f).is_some());
        let after = match (opener, self.opened) {
            (Some(f), Some((from, last))) if from == f && bar.index_of(last).is_some() => {
                Some(last)
            }
            _ => opener,
        };
        let loose_len = bar.loose_len();
        let after_end = after.and_then(|a| bar.block_end(a));
        let in_group = |w: u32| bar.tab(w).is_some_and(|t| t.app_id == app_id);
        // The app's first window here is a loose row; its second makes the
        // group.
        let first = !bar.tabs().any(|t| t.app_id == app_id);
        let at = if !grouped(app_id) || first {
            after_end.unwrap_or(loose_len).min(loose_len)
        } else {
            match bar.group_range(app_id) {
                Some((start, end)) if start == end => start,
                Some((_, end)) => match after.filter(|&a| in_group(a)) {
                    Some(a) => bar.index_of(a).map_or(end, |i| i + 1),
                    None => end,
                },
                None => after_end.unwrap_or(bar.len()).max(loose_len),
            }
        };
        (zone, at, opener)
    }

    /// Put `tab` (a window with no place of its own) beside the window of
    /// its app that came back by the app's id, while that one is open and
    /// the saved layout is still being restored: on its workspace, right
    /// after it in the bar, in its zone (Floating for `floats`), shown
    /// there.
    fn follow(&mut self, mut tab: Tab, floats: bool, now: Instant) -> Option<Placed> {
        let &leader = self.came_back.get(&tab.app_id)?;
        let restoring = self.restore_until.is_some_and(|until| now < until);
        let d = self.desk_of(leader).filter(|_| restoring);
        let Some(d) = d else {
            self.came_back.remove(&tab.app_id);
            return None;
        };
        let pinned = self.is_pinned(leader);
        let window_id = tab.window_id;
        let desk = &mut self.desks[d];
        tab.zone = if floats { Zone::Float } else { desk.zone_of(leader)? };
        let zone = tab.zone;
        let at = desk.bar.index_of(leader).map_or(desk.bar.len(), |i| i + 1);
        let shown = desk.insert(tab, at, true);
        if pinned {
            self.pinned.insert(window_id);
            self.spread(window_id);
        }
        Some(Placed {
            zone,
            shown,
            returning: true,
        })
    }

    /// Put `tab` in its remembered place, from the saved layout or from when
    /// it left. A pinned window's goes back to each other workspace too, where
    /// `copies` says it was there, else at the end.
    fn insert_slot(&mut self, mut tab: Tab, slot: &Slot, copies: &[Slot]) -> Placed {
        // A zone the layout no longer has: the end of Full.
        if !self.layout.contains(tab.zone) {
            tab.zone = FULL;
        }
        let zone = tab.zone;
        let window_id = tab.window_id;
        let d = self.desk_index(&slot.workspace).unwrap_or(self.showing);
        let mut shown = self.put_back(d, tab.clone(), slot);
        if slot.pinned {
            for copy in copies {
                if let Some(i) = self.desk_index(&copy.workspace) {
                    self.put_back(i, tab.clone(), copy);
                }
            }
            self.pinned.insert(window_id);
            self.spread(window_id);
            shown = self.desks[self.showing].shows(window_id);
        }
        Placed {
            zone,
            shown,
            returning: true,
        }
    }

    /// Put `tab` in `slot` on workspace `d`: before the first tab saved after
    /// it, or that left from after it (or added since).
    fn put_back(&mut self, d: usize, tab: Tab, slot: &Slot) -> bool {
        let desk = &mut self.desks[d];
        if slot.saved {
            desk.insert_ranked(tab, slot.at, slot.shown)
        } else {
            desk.insert_back(tab, slot.at, slot.shown)
        }
    }

    /// A pinned window: on every workspace, at the end of the bar where it
    /// has no row (a zone showing something goes on showing it).
    fn spread(&mut self, window_id: u32) {
        let Some(tab) = self.desks.iter().find_map(|d| d.tab(window_id)).cloned() else {
            return;
        };
        for desk in &mut self.desks {
            if desk.tab(window_id).is_none() {
                desk.push(tab.clone(), false);
            }
        }
    }

    /// Whether `window_id` is on every workspace (a dialog: its parent).
    pub fn is_pinned(&self, window_id: u32) -> bool {
        let window_id = self.parent_of(window_id).unwrap_or(window_id);
        self.pinned.contains(&window_id)
    }

    /// Pin `window_id` (a dialog: its parent) to every workspace; where it
    /// joins, it does not cover what shows. Pinned already, it stays on the
    /// showing workspace alone. Returns whether it is pinned now, `None` when
    /// it is not placed.
    pub fn toggle_pin(&mut self, window_id: u32) -> Option<bool> {
        let window_id = self.parent_of(window_id).unwrap_or(window_id);
        let here = self.desk_of(window_id)?;
        if self.pinned.remove(&window_id) {
            for (i, desk) in self.desks.iter_mut().enumerate() {
                if i != here {
                    desk.take(window_id);
                }
            }
            return Some(false);
        }
        self.pinned.insert(window_id);
        self.spread(window_id);
        Some(true)
    }

    /// `window_id` turns out to be the tab `key` (paired after it was
    /// placed). It is remembered by that key from now on, and when a place
    /// is waiting for the key (a restore, a restart) it moves there.
    pub fn set_key(&mut self, window_id: u32, key: &str, now: Instant) -> Option<Placed> {
        let d = self.desk_of(window_id)?;
        let tab = self.desks[d].tab(window_id)?;
        if tab.key == key {
            return None;
        }
        let app_id = tab.app_id.clone();
        for desk in &mut self.desks {
            if let Some(tab) = desk.bar.tabs_mut().find(|t| t.window_id == window_id) {
                tab.key = key.to_string();
            }
        }
        if !self.waiting(key, now) {
            return None;
        }
        let floats = self.zone_of(window_id) == Some(Zone::Float);
        Some(self.place(window_id, &app_id, key, floats, true, now))
    }

    /// A window closed. Its place waits [`GHOST_GRACE`] for its app to
    /// come back. A zone whose shown tab left shows the tab it showed
    /// before (else the next, else the previous). Returns the zone it was
    /// in. While the saved layout is still being restored, a window back in
    /// its saved place keeps that place: the bar is only half back, and the
    /// windows still to come line up by the saved layout (the desk restarted
    /// just after the shell: Mail and Settings, back first, came back under
    /// the pages, 2026-11-06).
    pub fn remove(&mut self, window_id: u32, now: Instant) -> Option<Zone> {
        self.parents.retain(|child, parent| *child != window_id && *parent != window_id);
        self.cinema_from.remove(&window_id);
        if self.focused == Some(window_id) {
            self.focused = None;
        }
        // Unsettled, it was never in a place of its own: that one still waits
        // under its tab's key, and a new window must not take this one.
        let leaves_a_place = !self.unsettled(window_id, now);
        let pinned = self.pinned.remove(&window_id);
        let restoring = self.restore_until.is_some_and(|until| now < until);
        let mut left = None;
        for desk in &mut self.desks {
            let others_away = self
                .ghosts
                .iter()
                .any(|(g, left)| g.workspace == desk.id && now < *left + GHOST_GRACE);
            if !others_away && desk.tab(window_id).is_some() {
                desk.number_tabs();
            }
            let rank = desk.rank_of(window_id).filter(|_| restoring);
            let at = rank.unwrap_or_else(|| desk.number_of(window_id));
            if let Some((zone, _, tab, shown)) = desk.take(window_id) {
                left = Some(zone);
                if leaves_a_place {
                    self.ghosts.push((
                        Slot {
                            workspace: desk.id.clone(),
                            zone,
                            key: tab.key,
                            at,
                            shown,
                            pinned,
                            saved: rank.is_some(),
                        },
                        now,
                    ));
                }
            }
        }
        left
    }

    /// Take a window off every workspace it is on, without leaving a ghost
    /// (it is moving, or became a dialog).
    fn take(&mut self, window_id: u32) -> Option<(Zone, usize, Tab, bool)> {
        let d = self.desk_of(window_id)?;
        let taken = self.desks[d].take(window_id);
        for desk in &mut self.desks {
            desk.take(window_id);
        }
        taken
    }

    /// `child` is a dialog of `parent`: it leaves any zone and rides above
    /// its parent. Returns true when it was a tab.
    pub fn set_parent(&mut self, child: u32, parent: u32) -> bool {
        self.parents.insert(child, parent);
        self.pinned.remove(&child);
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
    /// parent, for a dialog), and its workspace. Its zone is picked, so it
    /// comes on screen over whatever overlaps it. Returns false when it is
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
            desk.column_mut(zone).show(window_id);
            desk.pick(zone);
        }
        true
    }

    /// Keyboard focus moved to `window_id`: a new window joins its zone
    /// from now on. Focus never changes what is on screen; only showing a
    /// tab does.
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
        if let Some(zone) = desk.zone_of(window_id).filter(|z| *z != Zone::Float) {
            desk.last_zone = zone;
        }
    }

    /// Move `window_id` to `zone` on its workspace and show it there (its
    /// zone comes on screen with it). Its row keeps its place in the bar.
    /// Returns false when nothing moved.
    pub fn move_to(&mut self, window_id: u32, zone: Zone) -> bool {
        if !self.layout.contains(zone) {
            return false;
        }
        match self.zone_of(window_id) {
            Some(from) if zone == Zone::Cinema && from != Zone::Cinema => {
                self.cinema_from.insert(window_id, from);
            }
            Some(_) if zone != Zone::Cinema => {
                self.cinema_from.remove(&window_id);
            }
            _ => {}
        }
        let Some(d) = self.desk_of(window_id) else {
            return false;
        };
        // A pinned window has one zone: every workspace's row moves with it.
        // Only here does it come up; elsewhere it goes on as it was.
        for (i, desk) in self.desks.iter_mut().enumerate() {
            let here = i == d;
            let shown = desk.zone_of(window_id).is_some_and(|z| desk.column(z).shown == Some(window_id));
            if desk.rezone(window_id, zone, here || shown) && here {
                desk.pick(zone);
            }
        }
        true
    }

    /// Move `window_id`'s row in the bar to just before `before`'s: within
    /// its group for a grouped window (the group's end when `before` is not
    /// in it), else its own block before `before`'s block (else to the
    /// end). It keeps its zone, and nothing shows or hides. Returns false
    /// when nothing moved.
    pub fn reorder(&mut self, window_id: u32, before: Option<u32>) -> bool {
        let Some(d) = self.desk_of(window_id) else {
            return false;
        };
        self.desks[d].bar.reorder(window_id, before)
    }

    /// Move a block of the showing workspace's bar (a group by its header,
    /// or a loose row) before block `before` (else to the end). Returns
    /// false when nothing moved.
    pub fn move_block(&mut self, id: &BlockId, before: Option<&BlockId>) -> bool {
        self.desks[self.showing].bar.move_block(id, before)
    }

    /// The tab after (or before) `from` in the showing workspace's bar,
    /// wrapping, whatever their zones (a dialog steps from its parent).
    /// Without `from`, or from a window not in this bar, the first (the
    /// last, `back`). `None` when there is nothing else to step to.
    pub fn step(&self, from: Option<u32>, back: bool) -> Option<u32> {
        self.step_among(from, back, &|_| true)
    }

    /// [`Self::step`] through the tabs of `from`'s app only (Super+\`).
    pub fn step_app(&self, from: Option<u32>, back: bool) -> Option<u32> {
        let from = from.map(|w| self.parent_of(w).unwrap_or(w));
        let app = from.and_then(|w| self.desks[self.showing].tab(w))?.app_id.clone();
        self.step_among(from, back, &|t| t.app_id == app)
    }

    /// [`Self::step`] through the tabs `among` keeps, in bar order.
    pub fn step_among(
        &self,
        from: Option<u32>,
        back: bool,
        among: &dyn Fn(&Tab) -> bool,
    ) -> Option<u32> {
        let tabs: Vec<&Tab> = self.desks[self.showing].bar.tabs().filter(|t| among(t)).collect();
        let from = from.map(|w| self.parent_of(w).unwrap_or(w));
        let here = from.and_then(|w| tabs.iter().position(|t| t.window_id == w));
        let n = tabs.len();
        let next = match here {
            Some(_) if n < 2 => return None,
            Some(i) if back => (i + n - 1) % n,
            Some(i) => (i + 1) % n,
            None if back => n.checked_sub(1)?,
            None => 0,
        };
        tabs.get(next).map(|t| t.window_id)
    }

    /// The zone a tab in Cinema came from (Full when it has none): where
    /// it goes when its app leaves fullscreen.
    pub fn cinema_return(&self, window_id: u32) -> Zone {
        self.cinema_from
            .get(&window_id)
            .copied()
            .filter(|z| self.layout.contains(*z))
            .unwrap_or(FULL)
    }

    // ---------------------------------------------------------------------
    // Reading: the showing workspace
    // ---------------------------------------------------------------------

    /// The showing workspace's tabs in bar order.
    pub fn bar(&self) -> Vec<&Tab> {
        self.bar_on(self.showing)
    }

    /// The showing workspace's bar as blocks: loose rows and app groups
    /// (an empty group is a remembered place, not drawn).
    pub fn blocks(&self) -> &[Block] {
        self.desks[self.showing].bar.blocks()
    }

    /// Whether `app`'s group is folded on the showing workspace.
    pub fn is_folded(&self, app: &str) -> bool {
        self.desks[self.showing].bar.is_folded(app)
    }

    /// Fold `app`'s group on the showing workspace, or open it.
    pub fn toggle_folded(&mut self, app: &str) {
        self.desks[self.showing].bar.toggle_folded(app);
    }

    pub fn shown(&self, zone: Zone) -> Option<u32> {
        self.desks[self.showing].column(zone).shown
    }

    /// Floats on the showing workspace, bottom → top.
    pub fn float_order(&self) -> &[u32] {
        &self.desks[self.showing].float_z
    }

    /// The Cinema tab on screen on the showing workspace, if Cinema is up.
    pub fn cinema_up(&self, hidden: &dyn Fn(&str) -> bool) -> Option<u32> {
        let desk = &self.desks[self.showing];
        desk.on_screen(&self.layout, hidden)
            .contains(&Zone::Cinema)
            .then(|| desk.visible_shown(Zone::Cinema, hidden))
            .flatten()
    }

    /// What a tiled zone of the showing workspace puts on screen: its
    /// shown tab, or, when that tab's app is hidden (Super+H), the nearest
    /// tab that is not.
    pub fn visible_shown(&self, zone: Zone, hidden: &dyn Fn(&str) -> bool) -> Option<u32> {
        self.desks[self.showing].visible_shown(zone, hidden)
    }

    /// The tiled zones of the showing workspace on screen
    /// ([`Desk::on_screen`]).
    pub fn on_screen(&self, hidden: &dyn Fn(&str) -> bool) -> Vec<Zone> {
        self.desks[self.showing].on_screen(&self.layout, hidden)
    }

    /// Where focus belongs on the showing workspace when the focused tab
    /// left the screen: the last looked-at zone's tab, else the first on
    /// screen's.
    pub fn front(&self, hidden: &dyn Fn(&str) -> bool) -> Option<u32> {
        self.desks[self.showing].front(&self.layout, hidden)
    }

    // ---------------------------------------------------------------------
    // Reading: any workspace, any window
    // ---------------------------------------------------------------------

    /// Workspace `workspace`'s tabs in bar order.
    pub fn bar_on(&self, workspace: usize) -> Vec<&Tab> {
        self.desks
            .get(workspace)
            .map_or_else(Vec::new, |d| d.bar.tabs().collect())
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
        // A pinned window is on every workspace, so on the one showing.
        if self.desks[self.showing].tab(window_id).is_some() {
            return Some(self.showing);
        }
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

