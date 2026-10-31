//! Remembered places: the saved layout (`Topic::TabLayout`) waiting for
//! the session to restore its windows, and the places of windows that
//! just left, waiting for their app to come back.

use std::collections::HashMap;
use std::time::Instant;

use sola_bus::topics::{TabLayout, TabLayoutZone, Workspace, WorkspacePicks, Zone};

use super::desk::Desk;
use super::{FULL, GHOST_GRACE, RESTORE_GRACE, Stacks};

/// A remembered place: a workspace, a zone, a key, its index in that
/// workspace's bar, and whether the zone showed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Slot {
    pub workspace: String,
    pub zone: Zone,
    pub key: String,
    pub at: usize,
    pub shown: bool,
}

impl Stacks {
    /// Take the saved layout: its workspaces (when it has any), which
    /// shows, each one's zones in the order they were picked (an older
    /// file: whether Full was up), and the places windows that come within
    /// [`RESTORE_GRACE`] take. Its zones are runs of a workspace's bar, in
    /// order (a layout saved before the bar mixed zones has one run per
    /// zone). A zone saved before workspaces belongs to the first one; a
    /// zone the layout no longer has sends its windows to Full when they
    /// come.
    pub fn load(&mut self, layout: &TabLayout, now: Instant) {
        if !layout.workspaces.is_empty() {
            self.desks = layout
                .workspaces
                .iter()
                .map(|w| Desk::new(w.id.clone(), w.name.clone(), &self.layout.zones()))
                .collect();
            self.showing = self
                .desks
                .iter()
                .position(|d| d.id == layout.showing)
                .unwrap_or(0);
        }
        for d in &mut self.desks {
            d.picks = match layout.picks.iter().find(|p| p.workspace == d.id) {
                Some(p) => p.zones.clone(),
                None if layout.full.contains(&d.id) => vec![FULL],
                None => Vec::new(),
            };
            if let Some(&z) = d.picks.first() {
                d.last_zone = z;
            }
        }
        let first = self.desks[0].id.clone();
        self.restore.clear();
        // Each workspace's next index in its bar.
        let mut next: HashMap<String, usize> = HashMap::new();
        for z in &layout.zones {
            let workspace = if z.workspace.is_empty() {
                first.clone()
            } else {
                z.workspace.clone()
            };
            let start = next.entry(workspace.clone()).or_default();
            for (i, key) in z.apps.iter().enumerate() {
                self.restore.push(Slot {
                    workspace: workspace.clone(),
                    zone: z.zone,
                    key: key.clone(),
                    at: *start + i,
                    shown: z.shown == Some(i as u32),
                });
            }
            *start += z.apps.len();
        }
        self.restore_until = Some(now + RESTORE_GRACE);
    }

    /// The layout to save: the workspaces, then each workspace's bar as
    /// runs of tabs in one zone, with the places still waiting for a window
    /// (a restarting app, apps the session has not restored yet).
    pub fn layout(&self, now: Instant) -> TabLayout {
        let mut zones: Vec<TabLayoutZone> = Vec::new();
        for desk in &self.desks {
            // The bar: (zone, key, the zone shows it).
            let mut bar: Vec<(Zone, String, bool)> = desk
                .tabs
                .iter()
                .map(|t| {
                    let shown = desk.column(t.zone).shown == Some(t.window_id);
                    (t.zone, t.key.clone(), shown)
                })
                .collect();
            let here = |s: &Slot| s.workspace == desk.id;
            for (g, _) in self.live_ghosts(now).filter(|(g, _)| here(g)) {
                if g.shown {
                    for row in bar.iter_mut().filter(|row| row.0 == g.zone) {
                        row.2 = false;
                    }
                }
                let at = g.at.min(bar.len());
                bar.insert(at, (g.zone, g.key.clone(), g.shown));
            }
            for s in self.live_restore(now).filter(|s| here(s)) {
                let taken = bar.iter().any(|row| row.0 == s.zone && row.2);
                bar.push((s.zone, s.key.clone(), s.shown && !taken));
            }
            for (zone, key, shown) in bar {
                let run = match zones.last_mut() {
                    Some(z) if z.zone == zone && z.workspace == desk.id => z,
                    _ => {
                        zones.push(TabLayoutZone {
                            zone,
                            apps: Vec::new(),
                            shown: None,
                            workspace: desk.id.clone(),
                        });
                        zones.last_mut().expect("just pushed")
                    }
                };
                if shown && zone != Zone::Float {
                    run.shown = Some(run.apps.len() as u32);
                }
                run.apps.push(key);
            }
        }
        TabLayout {
            zones,
            workspaces: self
                .desks
                .iter()
                .map(|d| Workspace {
                    id: d.id.clone(),
                    name: d.name.clone(),
                })
                .collect(),
            showing: self.desks[self.showing].id.clone(),
            full: self
                .desks
                .iter()
                .filter(|d| d.picks.first() == Some(&FULL))
                .map(|d| d.id.clone())
                .collect(),
            picks: self
                .desks
                .iter()
                .filter(|d| !d.picks.is_empty())
                .map(|d| WorkspacePicks {
                    workspace: d.id.clone(),
                    zones: d.picks.clone(),
                })
                .collect(),
        }
    }

    pub(super) fn live_ghosts(&self, now: Instant) -> impl Iterator<Item = &(Slot, Instant)> {
        self.ghosts.iter().filter(move |(_, left)| now < *left + GHOST_GRACE)
    }

    pub(super) fn live_restore(&self, now: Instant) -> impl Iterator<Item = &Slot> {
        let live = self.restore_until.is_some_and(|until| now < until);
        self.restore.iter().filter(move |_| live)
    }

    pub(super) fn claim_ghost(&mut self, key: &str, now: Instant) -> Option<Slot> {
        self.ghosts.retain(|(_, left)| now < *left + GHOST_GRACE);
        let i = self.ghosts.iter().position(|(g, _)| g.key == key)?;
        Some(self.ghosts.remove(i).0)
    }

    pub(super) fn claim_restore(&mut self, key: &str, now: Instant) -> Option<Slot> {
        if !self.restore_until.is_some_and(|until| now < until) {
            self.restore.clear();
            return None;
        }
        let i = self.restore.iter().position(|s| s.key == key)?;
        Some(self.restore.remove(i))
    }

    /// Whether a place is waiting for `key`.
    pub(super) fn waiting(&self, key: &str, now: Instant) -> bool {
        self.live_ghosts(now).any(|(g, _)| g.key == key)
            || self.live_restore(now).any(|s| s.key == key)
    }
}
