//! One workspace: every tab in bar order, each in its zone; what each
//! tiled zone shows (and showed before it), whether Full or the halves
//! are up, the floats' stacking, the last focused half, and the tab in
//! Cinema.

use sola_bus::topics::Zone;

use super::{FULL, HALVES, Tab, ZONES, is_half};

pub(super) struct Column {
    pub zone: Zone,
    /// Tiled zones only.
    pub shown: Option<u32>,
    /// The tabs it has shown, least recent first: when the shown tab
    /// leaves, the one before it comes back.
    recent: Vec<u32>,
}

impl Column {
    /// Show `window_id` (a tiled zone).
    pub fn show(&mut self, window_id: u32) {
        self.shown = Some(window_id);
        self.recent.retain(|&w| w != window_id);
        self.recent.push(window_id);
    }
}

pub(super) struct Desk {
    pub id: String,
    pub name: String,
    /// Every tab in bar order. The bar lists them so, whatever their
    /// zones; a zone's own order is its tabs' order here.
    pub tabs: Vec<Tab>,
    pub columns: Vec<Column>,
    /// Floats bottom → top.
    pub float_z: Vec<u32>,
    /// Full was picked last (else the halves were). See [`Self::on_screen`].
    pub full: bool,
    /// Where a new window goes while the halves are up: the last half
    /// that had focus.
    pub last_half: Zone,
    /// The tab shown over the whole output, menubar and bar included.
    pub cinema: Option<u32>,
}

impl Desk {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            tabs: Vec::new(),
            columns: ZONES
                .iter()
                .map(|&zone| Column {
                    zone,
                    shown: None,
                    recent: Vec::new(),
                })
                .collect(),
            float_z: Vec::new(),
            full: false,
            last_half: HALVES[0].0,
            cinema: None,
        }
    }

    pub fn column(&self, zone: Zone) -> &Column {
        self.columns
            .iter()
            .find(|c| c.zone == zone)
            .unwrap_or(&self.columns[0])
    }

    pub fn column_mut(&mut self, zone: Zone) -> &mut Column {
        let i = self
            .columns
            .iter()
            .position(|c| c.zone == zone)
            .unwrap_or(0);
        &mut self.columns[i]
    }

    /// `zone`'s tabs, in bar order.
    pub fn in_zone(&self, zone: Zone) -> impl Iterator<Item = &Tab> {
        self.tabs.iter().filter(move |t| t.zone == zone)
    }

    pub fn zone_of(&self, window_id: u32) -> Option<Zone> {
        self.tab(window_id).map(|t| t.zone)
    }

    pub fn tab(&self, window_id: u32) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.window_id == window_id)
    }

    /// Where a new tiled window goes: Full while it is up, or when no half
    /// has a tab; else the last focused half.
    pub fn landing(&self) -> Zone {
        let has = |z: Zone| self.in_zone(z).next().is_some();
        if (self.full && has(FULL)) || !HALVES.iter().any(|(z, _)| has(*z)) {
            FULL
        } else {
            self.last_half
        }
    }

    /// A tab in `zone` was shown: Full takes the screen, or the halves do.
    pub fn pick(&mut self, zone: Zone) {
        if zone == FULL {
            self.full = true;
        } else if is_half(zone) {
            self.full = false;
            self.last_half = zone;
        }
    }

    /// Put `tab` at the end of the bar; a tiled zone shows it when `show`
    /// (or when it showed nothing).
    pub fn push(&mut self, tab: Tab, show: bool) -> bool {
        let at = self.tabs.len();
        self.insert(tab, at, show)
    }

    /// Put `tab` at index `at` of the bar (clamped), in its zone; a tiled
    /// zone shows it when `show` (or when it showed nothing).
    pub fn insert(&mut self, tab: Tab, at: usize, show: bool) -> bool {
        let (window_id, zone) = (tab.window_id, tab.zone);
        self.tabs.insert(at.min(self.tabs.len()), tab);
        if zone == Zone::Float {
            self.float_z.push(window_id);
            return true;
        }
        let col = self.column_mut(zone);
        if show || col.shown.is_none() {
            col.show(window_id);
            return true;
        }
        false
    }

    /// Take `window_id` out: its zone, its index in the bar, the tab, and
    /// whether its zone showed it. The zone then shows the tab it showed
    /// before (else its next in the bar, else its previous).
    pub fn take(&mut self, window_id: u32) -> Option<(Zone, usize, Tab, bool)> {
        self.float_z.retain(|&w| w != window_id);
        if self.cinema == Some(window_id) {
            self.cinema = None;
        }
        let at = self.tabs.iter().position(|t| t.window_id == window_id)?;
        let tab = self.tabs.remove(at);
        let zone = tab.zone;
        let next = self.tabs[at..].iter().find(|t| t.zone == zone);
        let prev = self.tabs[..at].iter().rev().find(|t| t.zone == zone);
        let fallback = next.or(prev).map(|t| t.window_id);
        let col = self.column_mut(zone);
        col.recent.retain(|&w| w != window_id);
        let was_shown = col.shown == Some(window_id);
        if was_shown {
            col.shown = col.recent.last().copied().or(fallback);
            if let Some(w) = col.shown {
                col.show(w);
            }
        }
        Some((zone, at, tab, was_shown))
    }

    /// Move `window_id` to just before `before` in the bar (else to the
    /// end). Its zone and what every zone shows stay.
    pub fn reorder(&mut self, window_id: u32, before: Option<u32>) -> bool {
        if before == Some(window_id) {
            return false;
        }
        let Some(from) = self.tabs.iter().position(|t| t.window_id == window_id) else {
            return false;
        };
        let tab = self.tabs.remove(from);
        let to = before
            .and_then(|b| self.tabs.iter().position(|t| t.window_id == b))
            .unwrap_or(self.tabs.len());
        self.tabs.insert(to, tab);
        to != from
    }

    pub fn clear(&mut self) {
        self.tabs.clear();
        for c in &mut self.columns {
            c.shown = None;
            c.recent.clear();
        }
        self.float_z.clear();
        self.cinema = None;
    }

    /// What a tiled zone puts on screen: its shown tab, or, when that
    /// tab's app is hidden (Super+H), the nearest tab that is not.
    pub fn visible_shown(&self, zone: Zone, hidden: &dyn Fn(&str) -> bool) -> Option<u32> {
        let tabs: Vec<&Tab> = self.in_zone(zone).collect();
        let shown = self.column(zone).shown;
        let at = shown.and_then(|w| tabs.iter().position(|t| t.window_id == w))?;
        let after = tabs[at..].iter();
        let before = tabs[..at].iter().rev();
        after
            .chain(before)
            .find(|t| !hidden(&t.app_id))
            .map(|t| t.window_id)
    }

    /// The tiled zones on screen: Full alone when it was picked last (or
    /// the halves have nothing to show) and has something to show itself;
    /// else the halves with something to show, left to right.
    pub fn on_screen(&self, hidden: &dyn Fn(&str) -> bool) -> Vec<Zone> {
        let halves: Vec<Zone> = HALVES
            .iter()
            .map(|(z, _)| *z)
            .filter(|z| self.visible_shown(*z, hidden).is_some())
            .collect();
        let full = self.visible_shown(FULL, hidden).is_some();
        if full && (self.full || halves.is_empty()) {
            vec![FULL]
        } else {
            halves
        }
    }

    /// The tab focus belongs on: Full's when it is up, else the last
    /// focused half's (else the other half's).
    pub fn front(&self, hidden: &dyn Fn(&str) -> bool) -> Option<u32> {
        let on = self.on_screen(hidden);
        let zone = if on.contains(&self.last_half) {
            self.last_half
        } else {
            *on.first()?
        };
        self.visible_shown(zone, hidden)
    }
}
