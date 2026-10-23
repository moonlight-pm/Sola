//! One workspace's zones: each zone's tabs in bar order, what each tiled
//! zone shows, the floats' stacking, the last focused tiled zone, and the
//! tab shown bigger than its zone.

use sola_bus::topics::Zone;

use super::{Present, TILED, Tab, ZONES};

pub(super) struct Column {
    pub zone: Zone,
    pub tabs: Vec<Tab>,
    /// Tiled zones only.
    pub shown: Option<u32>,
}

pub(super) struct Desk {
    pub id: String,
    pub name: String,
    pub columns: Vec<Column>,
    /// Floats bottom → top.
    pub float_z: Vec<u32>,
    /// Where a new window goes: the last tiled zone that had focus.
    pub last_tiled: Zone,
    pub present: Option<(u32, Present)>,
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
                })
                .collect(),
            float_z: Vec::new(),
            last_tiled: TILED[0].0,
            present: None,
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

    /// Put `tab` at the end of `zone`; a tiled zone shows it when `show`
    /// (or when it showed nothing).
    pub fn push(&mut self, tab: Tab, zone: Zone, show: bool) -> bool {
        let window_id = tab.window_id;
        let col = self.column_mut(zone);
        col.tabs.push(tab);
        if zone == Zone::Float {
            self.float_z.push(window_id);
            return true;
        }
        if show || col.shown.is_none() {
            col.shown = Some(window_id);
            return true;
        }
        false
    }

    /// Take `window_id` out: its zone, index, key, and whether its zone
    /// showed it. The zone then shows the next tab (else the previous).
    pub fn take(&mut self, window_id: u32) -> Option<(Zone, usize, Tab, bool)> {
        self.float_z.retain(|&w| w != window_id);
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
        Some((col.zone, at, tab, was_shown))
    }

    pub fn clear(&mut self) {
        for c in &mut self.columns {
            c.tabs.clear();
            c.shown = None;
        }
        self.float_z.clear();
        self.present = None;
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
}
