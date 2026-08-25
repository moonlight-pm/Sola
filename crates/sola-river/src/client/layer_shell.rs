//! `river_layer_shell_v1` — opt the WM into standard wlr-layer-shell.
//!
//! River only maps layer surfaces while the window manager holds this
//! global. Binding it (and attaching per-output / per-seat children) is
//! enough for clients like **sola-kvm** (edge barriers / pointer capture)
//! or any other wlr-layer-shell client to map surfaces.
//!
//! We do not layout exclusive zones into Sola's shell geometry yet — the
//! `non_exclusive_area` event is logged and ignored. Exclusive keyboard
//! focus from a layer surface is tracked so we stop fighting it with
//! `seat.focus_window` until River reports `focus_none`.

use tracing::{debug, info};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use crate::client::AppData;
use crate::protocol::river_layer_shell_v1::{
    river_layer_shell_output_v1::RiverLayerShellOutputV1,
    river_layer_shell_seat_v1::RiverLayerShellSeatV1, river_layer_shell_v1::RiverLayerShellV1,
};
use crate::protocol::river_window_management_v1::{
    river_output_v1::RiverOutputV1, river_seat_v1::RiverSeatV1,
};

#[derive(Default)]
pub struct LayerShellState {
    /// Bound `river_layer_shell_v1` global. Presence means layer-shell
    /// clients are allowed to map.
    pub manager: Option<RiverLayerShellV1>,
    /// One child per `river_output_v1` we've seen.
    pub outputs: Vec<RiverLayerShellOutputV1>,
    /// Child for the primary `river_seat_v1`.
    pub seat: Option<RiverLayerShellSeatV1>,
    /// True while a layer surface holds exclusive keyboard focus. River
    /// ignores WM focus requests until this clears.
    pub exclusive_focus: bool,
    /// Whether we have disabled shell xkb chords for the exclusive-focus
    /// period. Used to re-enable them exactly once on focus release.
    pub chords_suppressed: bool,
    /// First manage sequence after an output appears should mark a default
    /// output for layer clients that don't pick one.
    pub need_set_default: bool,
}

/// Call after binding the global, and whenever a new river output/seat
/// appears, so layer-shell children stay in sync.
pub fn attach_output(state: &mut AppData, output: &RiverOutputV1, qh: &QueueHandle<AppData>) {
    let Some(mgr) = state.layer_shell.manager.as_ref() else {
        return;
    };
    let ls_out = mgr.get_output(output, qh, ());
    debug!("river_layer_shell_v1.get_output");
    state.layer_shell.outputs.push(ls_out);
    state.layer_shell.need_set_default = true;
}

pub fn attach_seat(state: &mut AppData, seat: &RiverSeatV1, qh: &QueueHandle<AppData>) {
    let Some(mgr) = state.layer_shell.manager.as_ref() else {
        return;
    };
    if state.layer_shell.seat.is_some() {
        return;
    }
    let ls_seat = mgr.get_seat(seat, qh, ());
    info!("river_layer_shell_v1.get_seat");
    state.layer_shell.seat = Some(ls_seat);
}

/// After the global binds late relative to seat/outputs, attach any we
/// already hold.
pub fn attach_existing(state: &mut AppData, qh: &QueueHandle<AppData>) {
    if state.layer_shell.manager.is_none() {
        return;
    }
    if let Some(seat) = state.seat.clone() {
        attach_seat(state, &seat, qh);
    }
    // Attach any river outputs that do not yet have a layer-shell child.
    if state.layer_shell.outputs.len() < state.outputs.len() {
        let start = state.layer_shell.outputs.len();
        let missing: Vec<RiverOutputV1> = state.outputs[start..].to_vec();
        for output in &missing {
            attach_output(state, output, qh);
        }
    }
}

/// During a manage sequence: mark the first layer output as default if needed.
pub fn on_manage_start(state: &mut AppData) {
    if !state.layer_shell.need_set_default {
        return;
    }
    let Some(first) = state.layer_shell.outputs.first() else {
        return;
    };
    first.set_default();
    state.layer_shell.need_set_default = false;
    debug!("river_layer_shell_output_v1.set_default");
}

impl Dispatch<RiverLayerShellV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &RiverLayerShellV1,
        _: <RiverLayerShellV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // No events on the manager.
    }
}

impl Dispatch<RiverLayerShellOutputV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &RiverLayerShellOutputV1,
        event: <RiverLayerShellOutputV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_layer_shell_v1::river_layer_shell_output_v1::Event;
        let Event::NonExclusiveArea {
            x,
            y,
            width,
            height,
        } = event;
        // Hint only — Sola's shell still uses full-output geometry for
        // zoning. Logged so we can wire exclusive-zone insets later if
        // a panel ever needs them.
        debug!(
            x,
            y, width, height, "layer-shell non_exclusive_area (ignored)"
        );
    }
}

impl Dispatch<RiverLayerShellSeatV1, ()> for AppData {
    fn event(
        state: &mut Self,
        _: &RiverLayerShellSeatV1,
        event: <RiverLayerShellSeatV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use crate::protocol::river_layer_shell_v1::river_layer_shell_seat_v1::Event;
        match event {
            Event::FocusExclusive => {
                info!("layer-shell exclusive keyboard focus");
                state.layer_shell.exclusive_focus = true;
                // Don't keep a stale window focus claim while River owns focus.
                state.focused_window = None;
                // Disable shell chords (Meta+Space launcher, Meta+Tab, …) so
                // those keys reach the layer client (e.g. sola-kvm → Mac Cmd).
                // enable/disable are manage-sequence-only.
                state.pending.manage_dirty = true;
            }
            Event::FocusNonExclusive => {
                debug!("layer-shell non-exclusive keyboard focus");
                state.layer_shell.exclusive_focus = false;
                state.pending.manage_dirty = true;
            }
            Event::FocusNone => {
                info!("layer-shell focus released");
                state.layer_shell.exclusive_focus = false;
                // Re-run manage so pending Focus applies and shell chords
                // are re-enabled.
                state.pending.manage_dirty = true;
            }
        }
    }
}

/// During a manage sequence: suppress or restore shell xkb bindings when
/// a layer surface (e.g. sola-kvm pointer capture) holds exclusive keyboard focus.
///
/// River's xkb bindings are compositor-global: matching keys are never
/// delivered to the focused client. Without this, Meta+Space still opens
/// the Sola launcher while the cursor is captured for the Mac.
pub fn sync_chord_suppression(state: &mut AppData) {
    if state.layer_shell.exclusive_focus {
        if state.layer_shell.chords_suppressed {
            return;
        }
        let n = state.chords.by_chord.len();
        for binding in state.chords.by_chord.values() {
            binding.disable();
        }
        state.layer_shell.chords_suppressed = true;
        if n > 0 {
            info!(count = n, "disabled shell chords for layer exclusive focus");
        }
    } else if state.layer_shell.chords_suppressed {
        let n = state.chords.by_chord.len();
        for binding in state.chords.by_chord.values() {
            binding.enable();
        }
        state.layer_shell.chords_suppressed = false;
        if n > 0 {
            info!(
                count = n,
                "re-enabled shell chords after layer focus release"
            );
        }
    }
}
