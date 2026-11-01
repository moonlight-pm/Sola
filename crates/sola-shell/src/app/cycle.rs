//! Stepping between windows by key (`docs/specs/2026-10-21-next-tab-bar-design.md`
//! § App groups and keys): Super+Tab through the windows whose place covers
//! the pointer, Super+\` through the focused app's windows on this
//! workspace. Both go in bar order, wrapping; neither walks the whole bar.

use sola_bus::topics::Zone;

use super::Shell;

/// A window's place on the output: x, y, width, height.
type Rect = (i32, i32, i32, i32);

fn contains((x, y, w, h): Rect, (px, py): (i32, i32)) -> bool {
    px >= x && py >= y && px < x + w && py < y + h
}

impl Shell {
    /// Super+\` / Super+Shift+\`: the focused app's next (previous) window
    /// on the showing workspace.
    pub(crate) fn step_app(&mut self, back: bool) {
        let from = self.focused_window_id;
        if let Some(next) = self.tabs.step_app(from, back) {
            tracing::info!(?from, to = next, back, "next window of the app");
            self.activate_window(next);
        }
    }

    /// Super+Tab / Super+Shift+Tab: the next (previous) window whose place
    /// covers the pointer — any tiled zone there (Full and the half under
    /// it, Cinema), the floats under it — from the one on screen there.
    /// With the pointer on no window's place (the bar, the top row, a gap),
    /// the focused window's spot.
    pub(crate) fn step_here(&mut self, back: bool) {
        let Some(spot) = self.spot() else {
            return;
        };
        let here = self.windows_at(spot);
        let from = self.on_screen_at(spot);
        let next = self
            .tabs
            .step_among(from, back, &|t| here.contains(&t.window_id));
        if let Some(next) = next {
            tracing::info!(?spot, ?from, to = next, back, windows = here.len(), "next window here");
            self.activate_window(next);
        }
    }

    /// Where Super+Tab looks: the pointer when the chord came, when some
    /// window's place covers it; else the middle of the focused window.
    fn spot(&self) -> Option<(i32, i32)> {
        if let Some(p) = self.chord_pointer.filter(|&p| !self.windows_at(p).is_empty()) {
            return Some(p);
        }
        let w = self.focused_window_id?;
        let w = self.tabs.parent_of(w).unwrap_or(w);
        let (x, y, width, height) = self.place_of(w)?;
        Some((x + width / 2, y + height / 2))
    }

    /// The showing workspace's windows whose place covers `point`.
    fn windows_at(&self, point: (i32, i32)) -> Vec<u32> {
        self.tabs
            .bar()
            .iter()
            .filter(|t| self.place_of(t.window_id).is_some_and(|r| contains(r, point)))
            .map(|t| t.window_id)
            .collect()
    }

    /// The window on screen at `point`: the top float there, else what the
    /// tiled zone on screen there shows.
    fn on_screen_at(&self, point: (i32, i32)) -> Option<u32> {
        let hidden = |app: &str| self.is_app_hidden(app);
        if let Some(cinema) = self.tabs.cinema_up(&hidden) {
            return Some(cinema);
        }
        let float = self
            .tabs
            .float_order()
            .iter()
            .rev()
            .find(|&&w| self.place_of(w).is_some_and(|r| contains(r, point)));
        if let Some(&w) = float {
            return Some(w);
        }
        let (ow, oh) = self.zoning.output_size?;
        let layout = self.tabs.zone_layout();
        self.tabs
            .on_screen(&hidden)
            .into_iter()
            .find(|&z| {
                let f = crate::zoning::tile_frame(0, layout, z, ow, oh);
                contains((f.x, f.y, f.width, f.height), point)
            })
            .and_then(|z| self.tabs.visible_shown(z, &hidden))
    }

    /// Where `window_id`'s place is: its float's rectangle, the whole
    /// output in Cinema, else its zone's tile.
    fn place_of(&self, window_id: u32) -> Option<Rect> {
        let (ow, oh) = self.zoning.output_size?;
        match self.tabs.zone_of(window_id)? {
            Zone::Float => self
                .zoning
                .live_geometry
                .get(&window_id)
                .map(|g| (g.x, g.y, g.width, g.height)),
            Zone::Cinema => Some((0, 0, ow, oh)),
            zone => {
                let f = crate::zoning::tile_frame(window_id, self.tabs.zone_layout(), zone, ow, oh);
                Some((f.x, f.y, f.width, f.height))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rect_holds_its_inside_only() {
        let r = (10, 20, 100, 50);
        assert!(contains(r, (10, 20)));
        assert!(contains(r, (109, 69)));
        assert!(!contains(r, (110, 30)));
        assert!(!contains(r, (50, 70)));
    }
}
