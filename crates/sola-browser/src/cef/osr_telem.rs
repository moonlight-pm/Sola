//! Rate-limited OSR paint / HTML5-drag counters.
//!
//! Default `sola_browser=info` already prints these. A 250 ms window
//! during a drag or paint storm; idle stays quiet.

use std::cell::Cell;

use crate::engine::monotonic_ms;

const WINDOW_MS: u64 = 250;

/// CEF-thread counters (helper process).
#[derive(Default)]
pub struct HelperTelem {
    win_ms: Cell<u64>,
    paints: Cell<u32>,
    paint_full: Cell<u32>,
    paint_partial: Cell<u32>,
    incomplete_dst: Cell<u32>,
    dirty_px: Cell<u64>,
    overlays: Cell<u32>,
    overlay_full: Cell<u32>,
    overlay_alloc: Cell<u32>,
    overlay_unique: Cell<u32>,
    moves: Cell<u32>,
    mailbox_drop: Cell<u32>,
    last_move_ms: Cell<u64>,
    last_overlay_ms: Cell<u64>,
    last_paint_ms: Cell<u64>,
    drag_live: Cell<bool>,
}

impl HelperTelem {
    pub fn note_paint(&self, full: bool, incomplete_dst: bool, dirty_px: u64) {
        self.paints.set(self.paints.get() + 1);
        if full {
            self.paint_full.set(self.paint_full.get() + 1);
        } else {
            self.paint_partial.set(self.paint_partial.get() + 1);
        }
        if incomplete_dst {
            self.incomplete_dst.set(self.incomplete_dst.get() + 1);
        }
        self.dirty_px
            .set(self.dirty_px.get().saturating_add(dirty_px));
        self.last_paint_ms.set(monotonic_ms());
        self.maybe_flush();
    }

    pub fn note_overlay(&self, full: bool, unique_buf: bool, dirty_px: u64) {
        self.overlays.set(self.overlays.get() + 1);
        if full {
            self.overlay_full.set(self.overlay_full.get() + 1);
        }
        if unique_buf {
            self.overlay_unique.set(self.overlay_unique.get() + 1);
        } else {
            self.overlay_alloc.set(self.overlay_alloc.get() + 1);
        }
        self.dirty_px
            .set(self.dirty_px.get().saturating_add(dirty_px));
        self.last_overlay_ms.set(monotonic_ms());
        self.maybe_flush();
    }

    pub fn note_move(&self) {
        self.moves.set(self.moves.get() + 1);
        self.last_move_ms.set(monotonic_ms());
        self.maybe_flush();
    }

    pub fn note_mailbox_drop(&self) {
        self.mailbox_drop.set(self.mailbox_drop.get() + 1);
    }

    pub fn note_start_dragging(
        &self,
        x: i32,
        y: i32,
        ghost: Option<(u32, u32)>,
        from_cef_image: bool,
    ) {
        self.drag_live.set(true);
        let (gw, gh) = ghost.unwrap_or((0, 0));
        tracing::info!(
            x,
            y,
            ghost_w = gw,
            ghost_h = gh,
            from_cef_image,
            "osr start_dragging (HTML5 native)"
        );
        self.maybe_flush();
    }

    pub fn note_drag_end(&self, why: &'static str) {
        self.flush();
        self.drag_live.set(false);
        tracing::info!(why, "osr drag end");
    }

    fn maybe_flush(&self) {
        let now = monotonic_ms();
        let start = self.win_ms.get();
        if start == 0 {
            self.win_ms.set(now);
            return;
        }
        if now.saturating_sub(start) < WINDOW_MS {
            return;
        }
        let activity = self.paints.get() + self.overlays.get() + self.moves.get();
        if activity == 0 && !self.drag_live.get() {
            self.win_ms.set(now);
            return;
        }
        self.flush();
    }

    fn flush(&self) {
        let now = monotonic_ms();
        let start = self.win_ms.get();
        let dt = now.saturating_sub(start).max(1);
        let paints = self.paints.get();
        let overlays = self.overlays.get();
        let moves = self.moves.get();
        if paints == 0 && overlays == 0 && moves == 0 && !self.drag_live.get() {
            self.win_ms.set(now);
            return;
        }
        let overlay_lag = self
            .last_overlay_ms
            .get()
            .saturating_sub(self.last_move_ms.get());
        let paint_lag = self
            .last_paint_ms
            .get()
            .saturating_sub(self.last_move_ms.get());
        tracing::info!(
            dt_ms = dt,
            paints,
            paint_full = self.paint_full.get(),
            paint_partial = self.paint_partial.get(),
            incomplete_dst = self.incomplete_dst.get(),
            overlays,
            overlay_full = self.overlay_full.get(),
            overlay_alloc = self.overlay_alloc.get(),
            overlay_unique = self.overlay_unique.get(),
            moves,
            mailbox_drop = self.mailbox_drop.get(),
            dirty_mpx = self.dirty_px.get() as f64 / 1_000_000.0,
            overlay_lag_ms = overlay_lag,
            paint_lag_ms = paint_lag,
            html5_drag = self.drag_live.get(),
            "osr window"
        );
        self.paints.set(0);
        self.paint_full.set(0);
        self.paint_partial.set(0);
        self.incomplete_dst.set(0);
        self.dirty_px.set(0);
        self.overlays.set(0);
        self.overlay_full.set(0);
        self.overlay_alloc.set(0);
        self.overlay_unique.set(0);
        self.moves.set(0);
        self.mailbox_drop.set(0);
        self.win_ms.set(now);
    }
}

pub fn dirty_px(rects: &[crate::cef::paint::DirtyRect], w: u32, h: u32) -> u64 {
    if crate::cef::paint::is_full_damage(rects, w, h) {
        return u64::from(w).saturating_mul(u64::from(h));
    }
    rects
        .iter()
        .map(|r| u64::from(r.w).saturating_mul(u64::from(r.h)))
        .sum()
}
