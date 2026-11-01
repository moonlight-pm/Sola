//! One workspace: every tab in bar order (as blocks: loose rows and app
//! groups, [`Bar`]), each in its zone; what each tiled zone shows (and
//! showed before it), the order its zones were picked, the floats'
//! stacking, and the zone last looked at.

use sola_bus::topics::{Zone, ZoneLayout};

use super::bar::Bar;
use super::{FULL, Tab};

pub(super) struct Column {
    pub zone: Zone,
    /// Tiled zones only.
    pub shown: Option<u32>,
    /// The tabs it has shown, least recent first: when the shown tab
    /// leaves, the one before it comes back.
    recent: Vec<u32>,
}

impl Column {
    fn new(zone: Zone) -> Self {
        Self {
            zone,
            shown: None,
            recent: Vec::new(),
        }
    }

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
    /// Every tab in bar order, as blocks. The bar lists them so, whatever
    /// their zones; a zone's own order is its tabs' order here.
    pub bar: Bar,
    pub columns: Vec<Column>,
    /// Floats bottom → top.
    pub float_z: Vec<u32>,
    /// The tiled zones, most recently picked first. See [`Self::on_screen`].
    pub picks: Vec<Zone>,
    /// The tiled zone last looked at (focused, or picked): where a new
    /// window goes.
    pub last_zone: Zone,
}

impl Desk {
    pub fn new(id: impl Into<String>, name: impl Into<String>, zones: &[Zone]) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            bar: Bar::default(),
            columns: zones.iter().map(|&z| Column::new(z)).collect(),
            float_z: Vec::new(),
            picks: Vec::new(),
            last_zone: FULL,
        }
    }

    /// The layout changed: columns for new zones; a gone zone's tabs move
    /// to the end of Full (shown there when Full showed nothing).
    pub fn set_zones(&mut self, zones: &[Zone]) {
        for &z in zones {
            if !self.columns.iter().any(|c| c.zone == z) {
                self.columns.push(Column::new(z));
            }
        }
        let gone: Vec<Zone> = self
            .columns
            .iter()
            .map(|c| c.zone)
            .filter(|z| !zones.contains(z))
            .collect();
        if gone.is_empty() {
            return;
        }
        let moved: Vec<u32> = self
            .bar
            .tabs()
            .filter(|t| gone.contains(&t.zone))
            .map(|t| t.window_id)
            .collect();
        for t in self.bar.tabs_mut().filter(|t| gone.contains(&t.zone)) {
            t.zone = FULL;
        }
        self.columns.retain(|c| !gone.contains(&c.zone));
        let full = self.column_mut(FULL);
        for w in moved {
            if full.shown.is_none() {
                full.show(w);
            }
        }
        self.picks.retain(|z| !gone.contains(z));
        if gone.contains(&self.last_zone) {
            self.last_zone = FULL;
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
        self.bar.tabs().filter(move |t| t.zone == zone)
    }

    pub fn zone_of(&self, window_id: u32) -> Option<Zone> {
        self.tab(window_id).map(|t| t.zone)
    }

    pub fn tab(&self, window_id: u32) -> Option<&Tab> {
        self.bar.tab(window_id)
    }

    /// Where a new tiled window goes: the zone last looked at. The first
    /// window of a workspace goes to Full, and so does one opened while
    /// Cinema was looked at last (it takes no new windows). A zone that
    /// emptied gives way to what is on screen now.
    pub fn landing(&self, layout: &ZoneLayout) -> Zone {
        if !self.bar.tabs().any(|t| t.zone != Zone::Float) {
            return FULL;
        }
        match self.last_zone {
            Zone::Cinema | Zone::Float => FULL,
            z if self.in_zone(z).next().is_some() => z,
            _ => self
                .on_screen(layout, &|_| false)
                .into_iter()
                .find(|z| *z != Zone::Cinema)
                .unwrap_or(FULL),
        }
    }

    /// A tab in tiled `zone` was shown: it comes first, and pushes off
    /// whatever overlaps it.
    pub fn pick(&mut self, zone: Zone) {
        if zone == Zone::Float {
            return;
        }
        self.picks.retain(|z| *z != zone);
        self.picks.insert(0, zone);
        self.last_zone = zone;
    }

    /// Put `tab` at the end of the bar; a tiled zone shows it when `show`
    /// (or when it showed nothing).
    pub fn push(&mut self, tab: Tab, show: bool) -> bool {
        let at = self.bar.len();
        self.insert(tab, at, show)
    }

    /// Put `tab` at index `at` of the bar (in its app's group, when it has
    /// one: [`Bar::insert`]), in its zone; a tiled zone shows it when
    /// `show` (or when it showed nothing).
    pub fn insert(&mut self, tab: Tab, at: usize, show: bool) -> bool {
        let (window_id, zone) = (tab.window_id, tab.zone);
        self.bar.insert(tab, at);
        self.placed(window_id, zone, show)
    }

    /// Put back `tab`, saved at index `rank` of the bar, among the windows
    /// already back ([`Bar::insert_ranked`]).
    pub fn insert_ranked(
        &mut self,
        tab: Tab,
        rank: usize,
        rank_of: &dyn Fn(u32) -> Option<usize>,
        show: bool,
    ) -> bool {
        let (window_id, zone) = (tab.window_id, tab.zone);
        self.bar.insert_ranked(tab, rank, rank_of);
        self.placed(window_id, zone, show)
    }

    /// A tab was put in the bar: stack a float, show a tiled one when
    /// `show` or its zone showed nothing. Returns whether its zone shows it.
    fn placed(&mut self, window_id: u32, zone: Zone, show: bool) -> bool {
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
        let (at, tab) = self.bar.remove(window_id)?;
        let zone = tab.zone;
        let tabs: Vec<&Tab> = self.bar.tabs().collect();
        let next = tabs[at..].iter().find(|t| t.zone == zone);
        let prev = tabs[..at].iter().rev().find(|t| t.zone == zone);
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

    pub fn clear(&mut self) {
        self.bar.clear();
        for c in &mut self.columns {
            c.shown = None;
            c.recent.clear();
        }
        self.float_z.clear();
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

    /// The tiled zones on screen. Walk the zones most recently picked
    /// first (then the ones never picked: the grid's, Full, Cinema); each
    /// with something to show takes its cells unless one already taken
    /// overlaps it. So picking Full takes the halves off, and picking a
    /// half takes Full off and brings the other half back. Listed in the
    /// layout's order.
    pub fn on_screen(&self, layout: &ZoneLayout, hidden: &dyn Fn(&str) -> bool) -> Vec<Zone> {
        let mut order: Vec<Zone> = self.picks.clone();
        for z in layout.tiled() {
            if !order.contains(&z) {
                order.push(z);
            }
        }
        let mut taken: Vec<Zone> = Vec::new();
        for z in order {
            if !layout.contains(z) || self.visible_shown(z, hidden).is_none() {
                continue;
            }
            if taken.iter().any(|t| layout.overlaps(*t, z)) {
                continue;
            }
            taken.push(z);
        }
        // In the layout's order (left to right, as the grid lists them).
        let tiled = layout.tiled();
        taken.sort_by_key(|z| tiled.iter().position(|t| t == z));
        taken
    }

    /// The tab focus belongs on: the last looked-at zone's when it is on
    /// screen, else the first on screen's.
    pub fn front(&self, layout: &ZoneLayout, hidden: &dyn Fn(&str) -> bool) -> Option<u32> {
        let on = self.on_screen(layout, hidden);
        let zone = if on.contains(&self.last_zone) {
            self.last_zone
        } else {
            *on.first()?
        };
        self.visible_shown(zone, hidden)
    }
}
