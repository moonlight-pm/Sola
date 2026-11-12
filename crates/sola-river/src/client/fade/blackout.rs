//! The black before the machine sleeps (`Topic::Blackout`, from the shell):
//! a black layer over everything, the fade's overlay and the dim's black
//! included, rising to full over [`OVER`]. Once it is full, sola-river says
//! so (`Topic::BlackedOut`) and the shell asks for the sleep: NVIDIA's
//! driver keeps the last picture up for a second or two while it saves its
//! memory, and that picture is black (the Oath session, 2026-11-11). The
//! shell takes it off when the machine is back, or the sleep failed: down
//! over [`super::RETURN`], as the windows return from the fade.
//!
//! Its input region is empty, like the fade's: a click goes through.

use std::time::{Duration, Instant};

use sola_bus::topics::Topic;
use tracing::{info, warn};

use super::{RETURN, dim, flush};
use crate::client::AppData;

/// How long the screen takes to go black.
const OVER: Duration = Duration::from_millis(600);

#[derive(Default)]
pub struct Blackout {
    /// Going up (`true`) or coming down, since when, and from what level.
    change: Option<(bool, Instant, u32)>,
    layer: Option<dim::Dimmer>,
    /// `BlackedOut` was said for this rise.
    told: bool,
    /// Making the layer failed: say it is black anyway, so the sleep goes on.
    failed: bool,
}

/// `Topic::Blackout`: up, or down again. Asked up again while full, it says
/// so again (the shell asked once more).
pub fn set(state: &mut AppData, on: bool) {
    let b = &mut state.fade.blackout;
    let level = b.layer.as_ref().map_or(0, |l| l.level);
    match (on, b.change) {
        (true, Some((true, ..))) => {
            if b.told {
                state.bus.emit(Topic::BlackedOut);
            }
            return;
        }
        (false, None | Some((false, ..))) => return,
        (true, _) => {
            info!("blackout: the screen goes black before sleep");
            b.told = false;
            b.failed = false;
        }
        (false, _) => info!("blackout: the screen comes back"),
    }
    b.change = Some((on, Instant::now(), level));
    state.pending.render_dirty = true;
}

/// The layer's level `elapsed` into a change from `from`: up to full over
/// [`OVER`], or down to none over [`RETURN`] (quick at first, then
/// settling). And whether the change is done.
fn level(up: bool, from: u32, elapsed: Duration) -> (u32, bool) {
    let over = if up { OVER } else { RETURN };
    let t = (elapsed.as_secs_f64() / over.as_secs_f64()).clamp(0.0, 1.0);
    let level = if up {
        from + (f64::from(u32::MAX - from) * t) as u32
    } else {
        (f64::from(from) * (1.0 - t).powi(2)) as u32
    };
    (level, t >= 1.0)
}

/// Each bus tick: move the layer toward where the change has it; once it
/// is full, say so; once it is down, drop it.
pub(super) fn tick(state: &mut AppData) {
    let Some((up, since, from)) = state.fade.blackout.change else {
        return;
    };
    let (target, done) = level(up, from, since.elapsed());
    let b = &mut state.fade.blackout;
    match &mut b.layer {
        Some(layer) if layer.level != target => {
            layer.set(target);
            flush(state);
        }
        Some(_) => {}
        // Made in the next render sequence (it must be placed on top).
        None if up && !b.failed => state.pending.render_dirty = true,
        None => {}
    }
    let b = &mut state.fade.blackout;
    if up && done && !b.told && (b.layer.is_some() || b.failed) {
        b.told = true;
        info!("blackout: black");
        state.bus.emit(Topic::BlackedOut);
    }
    if !up && done {
        b.change = None;
        if b.layer.take().is_some() {
            flush(state);
        }
    }
}

/// Last in every render sequence: keep the layer on top of everything,
/// made when a change needs it.
pub(super) fn sync(state: &mut AppData) {
    let Some((up, since, from)) = state.fade.blackout.change else {
        state.fade.blackout.layer = None;
        return;
    };
    if let Some(layer) = &state.fade.blackout.layer {
        if Some(layer.size) == state.output_size {
            layer.node.place_top();
            return;
        }
    }
    if !up || state.fade.blackout.failed {
        return;
    }
    let (level, _) = level(up, from, since.elapsed());
    match dim::make(state, level) {
        Ok(layer) => state.fade.blackout.layer = Some(layer),
        Err(e) => {
            warn!(%e, "blackout layer; the sleep goes on without it");
            state.fade.blackout.failed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_rises_to_full_then_comes_down_from_where_it_was() {
        assert_eq!(level(true, 0, Duration::ZERO), (0, false));
        let (half, done) = level(true, 0, OVER / 2);
        assert!(!done && half > u32::MAX / 3 && half < u32::MAX / 3 * 2, "{half}");
        assert_eq!(level(true, 0, OVER), (u32::MAX, true));
        assert_eq!(level(true, 0, OVER * 3), (u32::MAX, true));
        // From the dim's tenth: already most of the way.
        let dim = (0.9 * f64::from(u32::MAX)) as u32;
        assert!(level(true, dim, Duration::ZERO).0 >= dim);
        assert_eq!(level(false, u32::MAX, Duration::ZERO), (u32::MAX, false));
        assert_eq!(level(false, u32::MAX, RETURN), (0, true));
    }
}
