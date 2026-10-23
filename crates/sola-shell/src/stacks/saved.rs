//! Remembered places: the saved layout (`Topic::TabLayout`) waiting for
//! the session to restore its windows, and the places of windows that
//! just left, waiting for their app to come back.

use std::time::Instant;

use sola_bus::topics::{TabLayout, TabLayoutZone, Workspace, Zone};

use super::desk::Desk;
use super::{GHOST_GRACE, RESTORE_GRACE, Stacks, ZONES};

/// A remembered place: a workspace, a zone, a key, its index in that zone,
/// and whether the zone showed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Slot {
    pub workspace: String,
    pub zone: Zone,
    pub key: String,
    pub at: usize,
    pub shown: bool,
}

impl Stacks {
    /// Take the saved layout: its workspaces (when it has any) and which
    /// shows, and the places windows that come within [`RESTORE_GRACE`]
    /// take. A zone saved before workspaces belongs to the first one.
    pub fn load(&mut self, layout: &TabLayout, now: Instant) {
        if !layout.workspaces.is_empty() {
            self.desks = layout
                .workspaces
                .iter()
                .map(|w| Desk::new(w.id.clone(), w.name.clone()))
                .collect();
            self.showing = self
                .desks
                .iter()
                .position(|d| d.id == layout.showing)
                .unwrap_or(0);
        }
        let first = self.desks[0].id.clone();
        self.restore.clear();
        for z in &layout.zones {
            if !ZONES.contains(&z.zone) {
                continue;
            }
            let workspace = if z.workspace.is_empty() {
                first.clone()
            } else {
                z.workspace.clone()
            };
            for (at, key) in z.apps.iter().enumerate() {
                self.restore.push(Slot {
                    workspace: workspace.clone(),
                    zone: z.zone,
                    key: key.clone(),
                    at,
                    shown: z.shown == Some(at as u32),
                });
            }
        }
        self.restore_until = Some(now + RESTORE_GRACE);
    }

    /// The layout to save: the workspaces, then each workspace's zones with
    /// their tabs and the places still waiting for a window (a restarting
    /// app, apps the session has not restored yet).
    pub fn layout(&self, now: Instant) -> TabLayout {
        let mut zones = Vec::new();
        for desk in &self.desks {
            for c in &desk.columns {
                let mut apps: Vec<String> = c.tabs.iter().map(|t| t.key.clone()).collect();
                let mut shown = c
                    .shown
                    .and_then(|w| c.tabs.iter().position(|t| t.window_id == w))
                    .map(|i| i as u32);
                let here = |s: &Slot| s.workspace == desk.id && s.zone == c.zone;
                for (g, _) in self.live_ghosts(now).filter(|(g, _)| here(g)) {
                    let at = g.at.min(apps.len());
                    apps.insert(at, g.key.clone());
                    match shown {
                        Some(s) if s as usize >= at => shown = Some(s + 1),
                        _ => {}
                    }
                    if g.shown {
                        shown = Some(at as u32);
                    }
                }
                for s in self.live_restore(now).filter(|s| here(s)) {
                    apps.push(s.key.clone());
                    if s.shown && shown.is_none() {
                        shown = Some(apps.len() as u32 - 1);
                    }
                }
                if c.zone == Zone::Float {
                    shown = None;
                }
                if !apps.is_empty() {
                    zones.push(TabLayoutZone {
                        zone: c.zone,
                        apps,
                        shown,
                        workspace: desk.id.clone(),
                    });
                }
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
