//! The tab bar's model (Next, `docs/specs/2026-10-21-next-tab-bar-design.md`):
//! which zone each window is in, each zone's tabs in bar order, and which
//! tab each tiled zone shows. Pure — the shell turns it into
//! `Composition` and `Frame`s and draws the bar from it.
//!
//! A tiled zone shows one tab; the rest stay out of the composition, so
//! River hides them. Every float shows. A window with a parent (a dialog)
//! is no tab: it rides above its parent.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use sola_bus::topics::{TabLayout, TabLayoutZone, Zone};

/// The tiled zones, left to right, and their share of the content area.
/// canto (1920 × 1080) has two halves; configurable before Next goes back
/// to novus.
pub const TILED: [(Zone, f32); 2] = [(Zone::Left, 0.5), (Zone::Right, 0.5)];

/// Every zone the bar lists, in bar order.
pub const ZONES: [Zone; 3] = [Zone::Left, Zone::Right, Zone::Float];

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
}

/// A remembered place: a zone, an app, its index in that zone, and
/// whether the zone showed it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Slot {
    zone: Zone,
    app_id: String,
    at: usize,
    shown: bool,
}

struct Column {
    zone: Zone,
    tabs: Vec<Tab>,
    /// Tiled zones only.
    shown: Option<u32>,
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
    columns: Vec<Column>,
    /// Floats bottom → top.
    float_z: Vec<u32>,
    /// child → parent.
    parents: HashMap<u32, u32>,
    /// Where a new window goes: the last tiled zone that had focus.
    last_tiled: Zone,
    present: Option<(u32, Present)>,
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
            columns: ZONES
                .iter()
                .map(|&zone| Column {
                    zone,
                    tabs: Vec::new(),
                    shown: None,
                })
                .collect(),
            float_z: Vec::new(),
            parents: HashMap::new(),
            last_tiled: TILED[0].0,
            present: None,
            restore: Vec::new(),
            restore_until: None,
            ghosts: Vec::new(),
            ranks: HashMap::new(),
        }
    }

    // ---------------------------------------------------------------------
    // Saved layout
    // ---------------------------------------------------------------------

    /// Take the saved layout. Windows that come within [`RESTORE_GRACE`]
    /// take their app's place.
    pub fn load(&mut self, layout: &TabLayout, now: Instant) {
        self.restore.clear();
        for z in &layout.zones {
            if !ZONES.contains(&z.zone) {
                continue;
            }
            for (at, app_id) in z.apps.iter().enumerate() {
                self.restore.push(Slot {
                    zone: z.zone,
                    app_id: app_id.clone(),
                    at,
                    shown: z.shown == Some(at as u32),
                });
            }
        }
        self.restore_until = Some(now + RESTORE_GRACE);
    }

    /// Forget every placed window (keeps the saved layout), so the shell
    /// can place them again after [`Self::load`].
    pub fn clear(&mut self) {
        for c in &mut self.columns {
            c.tabs.clear();
            c.shown = None;
        }
        self.float_z.clear();
        self.present = None;
        self.ranks.clear();
    }

    /// The layout to save: each zone's tabs, then the places still waiting
    /// for a window (a restarting app, apps the session has not restored
    /// yet).
    pub fn layout(&self, now: Instant) -> TabLayout {
        let zones = self
            .columns
            .iter()
            .map(|c| {
                let mut apps: Vec<String> = c.tabs.iter().map(|t| t.app_id.clone()).collect();
                let mut shown = c
                    .shown
                    .and_then(|w| c.tabs.iter().position(|t| t.window_id == w))
                    .map(|i| i as u32);
                for (g, _) in self.live_ghosts(now).filter(|(g, _)| g.zone == c.zone) {
                    let at = g.at.min(apps.len());
                    apps.insert(at, g.app_id.clone());
                    match shown {
                        Some(s) if s as usize >= at => shown = Some(s + 1),
                        _ => {}
                    }
                    if g.shown {
                        shown = Some(at as u32);
                    }
                }
                for s in self.live_restore(now).filter(|s| s.zone == c.zone) {
                    apps.push(s.app_id.clone());
                    if s.shown && shown.is_none() {
                        shown = Some(apps.len() as u32 - 1);
                    }
                }
                if c.zone == Zone::Float {
                    shown = None;
                }
                TabLayoutZone {
                    zone: c.zone,
                    apps,
                    shown,
                }
            })
            .collect();
        TabLayout { zones }
    }

    fn live_ghosts(&self, now: Instant) -> impl Iterator<Item = &(Slot, Instant)> {
        self.ghosts.iter().filter(move |(_, left)| now < *left + GHOST_GRACE)
    }

    fn live_restore(&self, now: Instant) -> impl Iterator<Item = &Slot> {
        let live = self.restore_until.is_some_and(|until| now < until);
        self.restore.iter().filter(move |_| live)
    }

    // ---------------------------------------------------------------------
    // Windows arriving and leaving
    // ---------------------------------------------------------------------

    /// Place a new window. In order: the place its app just left, its
    /// app's saved place, Floating for `floats`, else the end of the last
    /// focused tiled zone (shown). `returning` windows (the ones present
    /// when the shell starts) take a remembered place if there is one and
    /// otherwise join without being shown over the zone's tab.
    pub fn place(
        &mut self,
        window_id: u32,
        app_id: &str,
        floats: bool,
        returning: bool,
        now: Instant,
    ) -> Placed {
        self.remove_quiet(window_id);
        if let Some(slot) = self.claim_ghost(app_id, now) {
            return self.insert_slot(window_id, app_id, slot.zone, slot.at, slot.shown, None);
        }
        if let Some(slot) = self.claim_restore(app_id, now) {
            let at = slot.at;
            return self.insert_slot(window_id, app_id, slot.zone, at, slot.shown, Some(at));
        }
        let zone = if floats { Zone::Float } else { self.last_tiled };
        let col = self.column_mut(zone);
        col.tabs.push(Tab {
            window_id,
            app_id: app_id.to_string(),
        });
        let mut shown = true;
        if zone == Zone::Float {
            self.float_z.push(window_id);
        } else if returning && col.shown.is_some() {
            shown = false;
        } else {
            col.shown = Some(window_id);
        }
        Placed {
            zone,
            shown,
            returning,
        }
    }

    fn insert_slot(
        &mut self,
        window_id: u32,
        app_id: &str,
        zone: Zone,
        at: usize,
        was_shown: bool,
        rank: Option<usize>,
    ) -> Placed {
        let ranks = &self.ranks;
        let col = self
            .columns
            .iter_mut()
            .find(|c| c.zone == zone)
            .expect("zone is in ZONES");
        let index = match rank {
            // Before the first tab saved after it (or added since).
            Some(r) => col
                .tabs
                .iter()
                .position(|t| ranks.get(&t.window_id).is_none_or(|&other| other > r))
                .unwrap_or(col.tabs.len()),
            None => at.min(col.tabs.len()),
        };
        col.tabs.insert(
            index,
            Tab {
                window_id,
                app_id: app_id.to_string(),
            },
        );
        let mut shown = true;
        if zone == Zone::Float {
            self.float_z.push(window_id);
        } else if was_shown || col.shown.is_none() {
            col.shown = Some(window_id);
        } else {
            shown = false;
        }
        if let Some(r) = rank {
            self.ranks.insert(window_id, r);
        }
        Placed {
            zone,
            shown,
            returning: true,
        }
    }

    fn claim_ghost(&mut self, app_id: &str, now: Instant) -> Option<Slot> {
        self.ghosts
            .retain(|(_, left)| now < *left + GHOST_GRACE);
        let i = self.ghosts.iter().position(|(g, _)| g.app_id == app_id)?;
        Some(self.ghosts.remove(i).0)
    }

    fn claim_restore(&mut self, app_id: &str, now: Instant) -> Option<Slot> {
        if !self.restore_until.is_some_and(|until| now < until) {
            self.restore.clear();
            return None;
        }
        let i = self.restore.iter().position(|s| s.app_id == app_id)?;
        Some(self.restore.remove(i))
    }

    /// A window closed. Its place waits [`GHOST_GRACE`] for its app to
    /// come back. A zone whose shown tab left shows the next tab (else
    /// the previous). Returns the zone it was in.
    pub fn remove(&mut self, window_id: u32, now: Instant) -> Option<Zone> {
        self.parents.retain(|child, parent| *child != window_id && *parent != window_id);
        let (zone, at, app_id, was_shown) = self.take(window_id)?;
        self.ghosts.push((
            Slot {
                zone,
                app_id,
                at,
                shown: was_shown,
            },
            now,
        ));
        Some(zone)
    }

    /// Remove without leaving a ghost (the window is moving, or became a
    /// dialog).
    fn remove_quiet(&mut self, window_id: u32) -> Option<Zone> {
        self.take(window_id).map(|(zone, ..)| zone)
    }

    fn take(&mut self, window_id: u32) -> Option<(Zone, usize, String, bool)> {
        self.float_z.retain(|&w| w != window_id);
        self.ranks.remove(&window_id);
        if self.present.is_some_and(|(w, _)| w == window_id) {
            self.present = None;
        }
        let col = self
            .columns
            .iter_mut()
            .find(|c| c.tabs.iter().any(|t| t.window_id == window_id))?;
        let at = col.tabs.iter().position(|t| t.window_id == window_id)?;
        let tab = col.tabs.remove(at);
        let was_shown = col.shown == Some(window_id);
        if was_shown {
            col.shown = col
                .tabs
                .get(at)
                .or_else(|| at.checked_sub(1).and_then(|i| col.tabs.get(i)))
                .map(|t| t.window_id);
        }
        Some((col.zone, at, tab.app_id, was_shown))
    }

    /// `child` is a dialog of `parent`: it leaves any zone and rides above
    /// its parent. Returns true when it was a tab.
    pub fn set_parent(&mut self, child: u32, parent: u32) -> bool {
        self.parents.insert(child, parent);
        self.remove_quiet(child).is_some()
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
    /// parent, for a dialog). Returns false when it is not placed.
    pub fn show(&mut self, window_id: u32) -> bool {
        let window_id = self.parent_of(window_id).unwrap_or(window_id);
        let Some(zone) = self.zone_of(window_id) else {
            return false;
        };
        if zone == Zone::Float {
            self.float_z.retain(|&w| w != window_id);
            self.float_z.push(window_id);
        } else {
            self.column_mut(zone).shown = Some(window_id);
            self.last_tiled = zone;
        }
        true
    }

    /// Keyboard focus moved to `window_id`: a new window joins its zone
    /// from now on.
    pub fn focus(&mut self, window_id: u32) {
        let window_id = self.parent_of(window_id).unwrap_or(window_id);
        if let Some(zone) = self.zone_of(window_id).filter(|z| *z != Zone::Float) {
            self.last_tiled = zone;
        }
    }

    /// Move `window_id` to `zone`, in front of `before` (else at the end),
    /// and show it there. Returns false when nothing moved.
    pub fn move_to(&mut self, window_id: u32, zone: Zone, before: Option<u32>) -> bool {
        if !ZONES.contains(&zone) || before == Some(window_id) {
            return false;
        }
        let Some(from) = self.zone_of(window_id) else {
            return false;
        };
        let app_id = self
            .column(from)
            .tabs
            .iter()
            .find(|t| t.window_id == window_id)
            .map(|t| t.app_id.clone())
            .unwrap_or_default();
        self.remove_quiet(window_id);
        let col = self.column_mut(zone);
        let index = before
            .and_then(|b| col.tabs.iter().position(|t| t.window_id == b))
            .unwrap_or(col.tabs.len());
        col.tabs.insert(index, Tab { window_id, app_id });
        if zone == Zone::Float {
            self.float_z.push(window_id);
        } else {
            col.shown = Some(window_id);
            self.last_tiled = zone;
        }
        true
    }

    /// The tab after (or before) `window_id` in its zone, wrapping.
    pub fn step(&self, window_id: u32, back: bool) -> Option<u32> {
        let window_id = self.parent_of(window_id).unwrap_or(window_id);
        let zone = self.zone_of(window_id)?;
        let tabs = &self.column(zone).tabs;
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
        if self.zone_of(window_id).is_none() {
            return None;
        }
        self.present = match self.present {
            Some((w, p)) if w == window_id && p == present => None,
            _ => Some((window_id, present)),
        };
        self.present.map(|(_, p)| p)
    }

    // ---------------------------------------------------------------------
    // Reading
    // ---------------------------------------------------------------------

    pub fn tabs(&self, zone: Zone) -> &[Tab] {
        &self.column(zone).tabs
    }

    pub fn shown(&self, zone: Zone) -> Option<u32> {
        self.column(zone).shown
    }

    pub fn zone_of(&self, window_id: u32) -> Option<Zone> {
        self.columns
            .iter()
            .find(|c| c.tabs.iter().any(|t| t.window_id == window_id))
            .map(|c| c.zone)
    }

    /// Floats and dialogs: the shell does not size them.
    pub fn is_floating(&self, window_id: u32) -> bool {
        self.parents.contains_key(&window_id) || self.zone_of(window_id) == Some(Zone::Float)
    }

    /// Floats, bottom → top.
    pub fn float_order(&self) -> &[u32] {
        &self.float_z
    }

    pub fn present(&self) -> Option<(u32, Present)> {
        self.present
    }

    /// What a tiled zone puts on screen: its shown tab, or, when that
    /// tab's app is hidden (Super+H), the nearest tab that is not.
    pub fn visible_shown(&self, zone: Zone, hidden: &dyn Fn(&str) -> bool) -> Option<u32> {
        let col = self.column(zone);
        let at = col
            .shown
            .and_then(|w| col.tabs.iter().position(|t| t.window_id == w))?;
        let after = col.tabs[at..].iter();
        let before = col.tabs[..at].iter().rev();
        after
            .chain(before)
            .find(|t| !hidden(&t.app_id))
            .map(|t| t.window_id)
    }

    /// Tiled zones with something on screen, left to right.
    pub fn occupied(&self, hidden: &dyn Fn(&str) -> bool) -> Vec<Zone> {
        TILED
            .iter()
            .map(|(z, _)| *z)
            .filter(|z| self.visible_shown(*z, hidden).is_some())
            .collect()
    }

    fn column(&self, zone: Zone) -> &Column {
        self.columns
            .iter()
            .find(|c| c.zone == zone)
            .unwrap_or(&self.columns[0])
    }

    fn column_mut(&mut self, zone: Zone) -> &mut Column {
        let i = self
            .columns
            .iter()
            .position(|c| c.zone == zone)
            .unwrap_or(0);
        &mut self.columns[i]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> bool {
        false
    }

    fn ids(s: &Stacks, zone: Zone) -> Vec<u32> {
        s.tabs(zone).iter().map(|t| t.window_id).collect()
    }

    #[test]
    fn new_windows_join_the_focused_zone_and_show() {
        let t = Instant::now();
        let mut s = Stacks::new();
        let p = s.place(1, "sola-terminal", false, false, t);
        assert_eq!(p, Placed { zone: Zone::Left, shown: true, returning: false });
        s.place(2, "sola-browser", false, false, t);
        assert_eq!(ids(&s, Zone::Left), vec![1, 2]);
        assert_eq!(s.shown(Zone::Left), Some(2));

        s.move_to(2, Zone::Right, None);
        s.focus(2);
        s.place(3, "sola-mail", false, false, t);
        assert_eq!(ids(&s, Zone::Right), vec![2, 3]);
        assert_eq!(s.shown(Zone::Right), Some(3));
        assert_eq!(s.shown(Zone::Left), Some(1));
    }

    #[test]
    fn floats_and_dialogs() {
        let t = Instant::now();
        let mut s = Stacks::new();
        s.place(1, "sola-terminal", false, false, t);
        let p = s.place(2, "sola-scope", true, false, t);
        assert_eq!(p.zone, Zone::Float);
        assert!(s.is_floating(2));
        assert_eq!(s.float_order(), &[2]);

        s.place(3, "sola-terminal", false, false, t);
        assert!(s.set_parent(3, 1));
        assert_eq!(ids(&s, Zone::Left), vec![1]);
        assert!(s.is_floating(3));
        assert_eq!(s.children_of(1), vec![3]);
        // Showing a dialog shows its parent.
        assert!(s.show(3));
        assert_eq!(s.shown(Zone::Left), Some(1));
    }

    #[test]
    fn closing_the_shown_tab_shows_the_next_then_the_previous() {
        let t = Instant::now();
        let mut s = Stacks::new();
        for w in 1..=3 {
            s.place(w, "app", false, false, t);
        }
        s.show(2);
        s.remove(2, t);
        assert_eq!(s.shown(Zone::Left), Some(3));
        s.remove(3, t);
        assert_eq!(s.shown(Zone::Left), Some(1));
        s.remove(1, t);
        assert_eq!(s.shown(Zone::Left), None);
    }

    #[test]
    fn a_restarting_app_comes_back_to_its_place() {
        let t = Instant::now();
        let mut s = Stacks::new();
        s.place(1, "sola-terminal", false, false, t);
        s.place(2, "sola-browser", false, false, t);
        s.place(3, "sola-mail", false, false, t);
        s.show(2);
        s.remove(2, t);
        let p = s.place(9, "sola-browser", false, false, t + Duration::from_secs(3));
        assert_eq!(p, Placed { zone: Zone::Left, shown: true, returning: true });
        assert_eq!(ids(&s, Zone::Left), vec![1, 9, 3]);

        // Past the grace, a new window is just new.
        s.remove(9, t);
        let later = t + GHOST_GRACE + Duration::from_secs(1);
        let p = s.place(10, "sola-browser", false, false, later);
        assert!(!p.returning);
        assert_eq!(ids(&s, Zone::Left), vec![1, 3, 10]);
    }

    #[test]
    fn saved_layout_restores_in_order_whatever_order_windows_come() {
        let t = Instant::now();
        let layout = TabLayout {
            zones: vec![
                TabLayoutZone {
                    zone: Zone::Left,
                    apps: vec!["term".into(), "browser".into(), "mail".into()],
                    shown: Some(1),
                },
                TabLayoutZone {
                    zone: Zone::Right,
                    apps: vec!["design".into()],
                    shown: Some(0),
                },
                TabLayoutZone {
                    zone: Zone::Float,
                    apps: vec!["scope".into()],
                    shown: None,
                },
            ],
        };
        let mut s = Stacks::new();
        s.load(&layout, t);
        s.place(3, "mail", false, false, t);
        s.place(2, "browser", false, false, t);
        s.place(4, "design", false, false, t);
        s.place(1, "term", false, false, t);
        s.place(5, "scope", false, false, t);
        assert_eq!(ids(&s, Zone::Left), vec![1, 2, 3]);
        assert_eq!(s.shown(Zone::Left), Some(2));
        assert_eq!(ids(&s, Zone::Right), vec![4]);
        assert_eq!(s.float_order(), &[5]);
        assert_eq!(s.layout(t), layout);
    }

    #[test]
    fn layout_keeps_places_still_waiting() {
        let t = Instant::now();
        let mut s = Stacks::new();
        s.load(
            &TabLayout {
                zones: vec![TabLayoutZone {
                    zone: Zone::Left,
                    apps: vec!["term".into(), "mail".into()],
                    shown: Some(1),
                }],
            },
            t,
        );
        s.place(1, "term", false, false, t);
        let l = s.layout(t);
        assert_eq!(l.zones[0].apps, vec!["term".to_string(), "mail".to_string()]);
        assert_eq!(l.zones[0].shown, Some(0));
        // After the grace the unclaimed place is dropped.
        let l = s.layout(t + RESTORE_GRACE + Duration::from_secs(1));
        assert_eq!(l.zones[0].apps, vec!["term".to_string()]);
    }

    #[test]
    fn returning_windows_do_not_cover_the_zone() {
        let t = Instant::now();
        let mut s = Stacks::new();
        s.place(1, "term", false, true, t);
        let p = s.place(2, "browser", false, true, t);
        assert!(!p.shown);
        assert_eq!(s.shown(Zone::Left), Some(1));
    }

    #[test]
    fn move_and_step() {
        let t = Instant::now();
        let mut s = Stacks::new();
        for w in 1..=3 {
            s.place(w, "app", false, false, t);
        }
        assert!(s.move_to(1, Zone::Right, None));
        assert_eq!(s.shown(Zone::Right), Some(1));
        assert_eq!(ids(&s, Zone::Left), vec![2, 3]);
        assert!(s.move_to(3, Zone::Left, Some(2)));
        assert_eq!(ids(&s, Zone::Left), vec![3, 2]);
        assert_eq!(s.step(3, false), Some(2));
        assert_eq!(s.step(3, true), Some(2));
        assert_eq!(s.step(1, false), None);
    }

    #[test]
    fn hidden_apps_give_way() {
        let t = Instant::now();
        let mut s = Stacks::new();
        s.place(1, "term", false, false, t);
        s.place(2, "mail", false, false, t);
        let hide_mail = |a: &str| a == "mail";
        assert_eq!(s.visible_shown(Zone::Left, &hide_mail), Some(1));
        assert_eq!(s.occupied(&none), vec![Zone::Left]);
        s.move_to(1, Zone::Right, None);
        assert_eq!(s.occupied(&hide_mail), vec![Zone::Right]);
    }

    #[test]
    fn present_toggles_and_ends_when_the_window_moves() {
        let t = Instant::now();
        let mut s = Stacks::new();
        s.place(1, "term", false, false, t);
        assert_eq!(s.toggle_present(1, Present::Fullscreen), Some(Present::Fullscreen));
        assert_eq!(s.toggle_present(1, Present::Cinema), Some(Present::Cinema));
        assert_eq!(s.toggle_present(1, Present::Cinema), None);
        s.toggle_present(1, Present::Fullscreen);
        s.move_to(1, Zone::Right, None);
        assert_eq!(s.present(), None);
    }
}
