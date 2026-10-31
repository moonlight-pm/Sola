//! The zone layout (`docs/specs/2026-10-30-zone-layout-design.md`): taking
//! a new layout from Settings (`Topic::ZoneLayout`), an app asking for
//! fullscreen (`Topic::FullscreenRequest`), and what the system's zone
//! keys are. The model is [`crate::stacks`]; the geometry
//! [`crate::zoning::tile_frame`].

use sola_bus::topics::{FullscreenRequest, Zone, ZoneLayout};

use super::Shell;
use crate::bindings::{system_actions, zone_actions};

impl Shell {
    /// Settings changed the layout (`None`: retracted, the default). The
    /// tabs of a gone zone move to Full; every tab is framed again; the
    /// zone keys follow the zones.
    pub(super) fn on_zone_layout(&mut self, layout: Option<ZoneLayout>) {
        let layout = layout.unwrap_or_default();
        if !self.tabs.set_layout(layout.clone()) {
            return;
        }
        tracing::info!(
            rows = layout.rows,
            columns = layout.columns,
            zones = layout.zones.len(),
            "zone layout"
        );
        self.keys.set_actions(system_actions(zone_actions(&layout)));
        self.publish_key_actions();
        self.emit_registered_chords();
        self.emit_all_frames();
        self.emit_composition();
        self.persist_tabs();
    }

    /// An app asked to enter or leave fullscreen (sola-river granted it
    /// already): its tab goes to Cinema, or back to the zone it came from.
    pub(super) fn on_fullscreen_request(&mut self, r: FullscreenRequest) {
        let wid = self.tabs.parent_of(r.window_id).unwrap_or(r.window_id);
        let Some(zone) = self.tabs.zone_of(wid) else {
            return;
        };
        let to = match (r.fullscreen, zone) {
            (true, Zone::Cinema) | (false, Zone::Float) => return,
            (true, _) => Zone::Cinema,
            (false, Zone::Cinema) => self.tabs.cinema_return(wid),
            (false, _) => return,
        };
        tracing::info!(window_id = wid, fullscreen = r.fullscreen, ?to, "app fullscreen");
        self.move_tab(wid, to);
        self.emit_composition();
        self.persist_tabs();
    }

    /// The Cinema tab on screen now; when it changed, every tab is framed
    /// again, so only the Cinema tab on screen is in the compositor's
    /// fullscreen (the rest of Cinema's tabs keep the output's size).
    pub(super) fn sync_cinema_shown(&mut self) {
        let cinema = {
            let hidden = |app: &str| self.is_app_hidden(app);
            self.tabs.cinema_up(&hidden)
        };
        if cinema != self.cinema_shown {
            tracing::info!(?cinema, "cinema on screen");
            self.cinema_shown = cinema;
            self.emit_all_frames();
        }
    }
}
