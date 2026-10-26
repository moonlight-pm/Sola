//! One workspace's zones: each zone's tabs in bar order, what each tiled
//! zone shows (and showed before it), whether Full or the halves are up,
//! the floats' stacking, the last focused half, and the tab in Cinema.

use sola_bus::topics::Zone;

use super::{FULL, HALVES, Tab, ZONES, is_half};

pub(super) struct Column {
    pub zone: Zone,
    pub tabs: Vec<Tab>,
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
            columns: ZONES
                .iter()
                .map(|&zone| Column {
                    zone,
                    tabs: Vec::new(),
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

    pub fn zone_of(&self, window_id: u32) -> Option<Zone> {
        self.columns
            .iter()
            .find(|c| c.tabs.iter().any(|t| t.window_id == window_id))
            .map(|c| c.zone)
    }

    pub fn tab(&self, window_id: u32) -> Option<&Tab> {
        self.columns
            .iter()
            .flat_map(|c| c.tabs.iter())
            .find(|t| t.window_id == window_id)
    }

    /// Every tab on this workspace, zone by zone.
    pub fn all_tabs(&self) -> impl Iterator<Item = &Tab> {
        self.columns.iter().flat_map(|c| c.tabs.iter())
    }

    /// Where a new tiled window goes: Full while it is up, or when no half
    /// has a tab; else the last focused half.
    pub fn landing(&self) -> Zone {
        let has = |z: Zone| !self.column(z).tabs.is_empty();
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

    /// Put `tab` at the end of `zone`; a tiled zone shows it when `show`
    /// (or when it showed nothing).
    pub fn push(&mut self, tab: Tab, zone: Zone, show: bool) -> bool {
        let at = self.column(zone).tabs.len();
        self.insert(tab, zone, at, show)
    }

    /// Put `tab` at index `at` of `zone` (clamped); a tiled zone shows it
    /// when `show` (or when it showed nothing).
    pub fn insert(&mut self, tab: Tab, zone: Zone, at: usize, show: bool) -> bool {
        let window_id = tab.window_id;
        let col = self.column_mut(zone);
        col.tabs.insert(at.min(col.tabs.len()), tab);
        if zone == Zone::Float {
            self.float_z.push(window_id);
            return true;
        }
        if show || col.shown.is_none() {
            col.show(window_id);
            return true;
        }
        false
    }

    /// Take `window_id` out: its zone, index, key, and whether its zone
    /// showed it. The zone then shows the tab it showed before (else the
    /// next, else the previous).
    pub fn take(&mut self, window_id: u32) -> Option<(Zone, usize, Tab, bool)> {
        self.float_z.retain(|&w| w != window_id);
        if self.cinema == Some(window_id) {
            self.cinema = None;
        }
        let col = self
            .columns
            .iter_mut()
            .find(|c| c.tabs.iter().any(|t| t.window_id == window_id))?;
        let at = col.tabs.iter().position(|t| t.window_id == window_id)?;
        let tab = col.tabs.remove(at);
        col.recent.retain(|&w| w != window_id);
        let was_shown = col.shown == Some(window_id);
        if was_shown {
            col.shown = col.recent.last().copied().or_else(|| {
                col.tabs
                    .get(at)
                    .or_else(|| at.checked_sub(1).and_then(|i| col.tabs.get(i)))
                    .map(|t| t.window_id)
            });
            if let Some(w) = col.shown {
                col.show(w);
            }
        }
        Some((col.zone, at, tab, was_shown))
    }

    pub fn clear(&mut self) {
        for c in &mut self.columns {
            c.tabs.clear();
            c.shown = None;
            c.recent.clear();
        }
        self.float_z.clear();
        self.cinema = None;
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
