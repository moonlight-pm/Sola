//! Five global screens + dwindle trees + float/fullscreen placement.

use std::collections::HashMap;

use sola_bus::topics::{FrameUpdate, ScreenFloat, ScreenLayout, Window};

use crate::tiling::{
    cinema_area, frame_from_rect, fullscreen_area, usable_area, Compass, Dir, Rect, Tree,
};
use crate::zoning::MENUBAR_HEIGHT;

pub const SCREEN_COUNT: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Float,
    Tiled,
    Fullscreen,
    Cinema,
}

#[derive(Debug)]
pub struct ScreenState {
    current: u8,
    former: u8,
    trees: [Tree; SCREEN_COUNT as usize],
    /// Screen (1..=5) for every known window.
    screen_of: HashMap<u32, u8>,
    special: HashMap<u32, Mode>,
    dirty: bool,
}

impl Default for ScreenState {
    fn default() -> Self {
        Self {
            current: 1,
            former: 1,
            trees: Default::default(),
            screen_of: HashMap::new(),
            special: HashMap::new(),
            dirty: false,
        }
    }
}

impl ScreenState {
    pub fn current(&self) -> u8 {
        self.current
    }

    pub fn former(&self) -> u8 {
        self.former
    }

    pub fn occupied(&self, screen: u8) -> bool {
        self.screen_of.values().any(|&s| s == screen)
    }

    pub fn screen_of(&self, window_id: u32) -> u8 {
        self.screen_of.get(&window_id).copied().unwrap_or(self.current)
    }

    pub fn is_on_current(&self, window_id: u32) -> bool {
        self.screen_of(window_id) == self.current
    }

    pub fn mode(&self, window_id: u32) -> Mode {
        if let Some(m) = self.special.get(&window_id) {
            return *m;
        }
        if self.tree(self.screen_of(window_id)).contains(window_id) {
            Mode::Tiled
        } else {
            Mode::Float
        }
    }

    pub fn is_tiled(&self, window_id: u32) -> bool {
        matches!(self.mode(window_id), Mode::Tiled)
    }

    pub fn take_dirty(&mut self) -> bool {
        let d = self.dirty;
        self.dirty = false;
        d
    }

    fn mark(&mut self) {
        self.dirty = true;
    }

    fn idx(screen: u8) -> usize {
        (screen.clamp(1, SCREEN_COUNT) - 1) as usize
    }

    fn tree(&self, screen: u8) -> &Tree {
        &self.trees[Self::idx(screen)]
    }

    fn tree_mut(&mut self, screen: u8) -> &mut Tree {
        &mut self.trees[Self::idx(screen)]
    }

    /// Ensure a newly mapped window lives on a screen (current, unless a
    /// persisted tree/float already claims its app_id).
    pub fn ensure_window(&mut self, window_id: u32, app_id: &str) {
        if self.screen_of.contains_key(&window_id) {
            return;
        }
        for s in 1..=SCREEN_COUNT {
            if self.tree_mut(s).attach_window(app_id, window_id) {
                self.screen_of.insert(window_id, s);
                self.mark();
                return;
            }
        }
        self.screen_of.insert(window_id, self.current);
        self.mark();
    }

    pub fn forget_window(&mut self, window_id: u32) {
        if let Some(screen) = self.screen_of.remove(&window_id) {
            self.tree_mut(screen).remove(window_id);
        }
        self.special.remove(&window_id);
        self.mark();
    }

    pub fn switch_to(&mut self, screen: u8) {
        let screen = screen.clamp(1, SCREEN_COUNT);
        if screen == self.current {
            return;
        }
        self.former = self.current;
        self.current = screen;
        self.mark();
    }

    pub fn cycle(&mut self, next: bool) {
        let n = if next {
            if self.current == SCREEN_COUNT {
                1
            } else {
                self.current + 1
            }
        } else if self.current == 1 {
            SCREEN_COUNT
        } else {
            self.current - 1
        };
        self.switch_to(n);
    }

    pub fn switch_former(&mut self) {
        self.switch_to(self.former);
    }

    pub fn send_to(&mut self, window_id: u32, app_id: &str, dest: u8, live: Option<Rect>) {
        let dest = dest.clamp(1, SCREEN_COUNT);
        let src = self.screen_of(window_id);
        let was_tiled = self.tree(src).contains(window_id);
        if was_tiled {
            self.tree_mut(src).remove(window_id);
        }
        self.special.remove(&window_id);
        self.screen_of.insert(window_id, dest);
        if was_tiled {
            let focused = self.tree(dest).windows().last().copied();
            self.tree_mut(dest)
                .insert(focused, window_id, app_id, live);
        }
        self.switch_to(dest);
        self.mark();
    }

    /// Super+Y: tile the float, or float the tile.
    pub fn toggle_tile(
        &mut self,
        window_id: u32,
        app_id: &str,
        focused_tiled: Option<u32>,
        live: Option<Rect>,
    ) -> Mode {
        self.special.remove(&window_id);
        let screen = self.screen_of(window_id);
        if self.tree(screen).contains(window_id) {
            self.tree_mut(screen).remove(window_id);
            self.mark();
            Mode::Float
        } else {
            self.tree_mut(screen)
                .insert(focused_tiled, window_id, app_id, live);
            self.mark();
            Mode::Tiled
        }
    }

    pub fn toggle_fullscreen(&mut self, window_id: u32) -> Mode {
        match self.special.get(&window_id).copied() {
            Some(Mode::Fullscreen) => {
                self.special.remove(&window_id);
                self.mark();
                self.mode(window_id)
            }
            _ => {
                self.special.insert(window_id, Mode::Fullscreen);
                self.mark();
                Mode::Fullscreen
            }
        }
    }

    pub fn toggle_cinema(&mut self, window_id: u32) -> Mode {
        match self.special.get(&window_id).copied() {
            Some(Mode::Cinema) => {
                self.special.remove(&window_id);
                self.mark();
                self.mode(window_id)
            }
            _ => {
                self.special.insert(window_id, Mode::Cinema);
                self.mark();
                Mode::Cinema
            }
        }
    }

    pub fn toggle_split(&mut self, window_id: u32) {
        let screen = self.screen_of(window_id);
        self.tree_mut(screen).toggle_split(window_id);
        self.mark();
    }

    pub fn swap(&mut self, a: u32, b: u32) {
        if self.screen_of(a) != self.screen_of(b) {
            return;
        }
        let screen = self.screen_of(a);
        self.tree_mut(screen).swap(a, b);
        self.mark();
    }

    pub fn neighbor(&self, focused: u32, dir: Compass, output: (i32, i32)) -> Option<u32> {
        let screen = self.screen_of(focused);
        let area = usable_area(output.0, output.1, MENUBAR_HEIGHT);
        let laid = self.tree(screen).layout(area);
        self.tree(screen).neighbor(focused, dir, &laid)
    }

    /// Geometry neighbor among `candidates` (window_id, rect) on this screen.
    pub fn neighbor_among(
        focused: u32,
        dir: Compass,
        candidates: &[(u32, Rect)],
    ) -> Option<u32> {
        let dummy = Tree::default();
        dummy.neighbor(focused, dir, candidates)
    }

    pub fn apply_move_op(&mut self, window_id: u32, drop: Rect, output: (i32, i32)) {
        if !self.is_tiled(window_id) {
            return;
        }
        let screen = self.screen_of(window_id);
        let area = usable_area(output.0, output.1, MENUBAR_HEIGHT);
        let laid = self.tree(screen).layout(area);
        let (cx, cy) = drop.center();
        if let Some(&(other, _)) = laid.iter().find(|(id, r)| *id != window_id && r.contains(cx, cy))
        {
            self.tree_mut(screen).swap(window_id, other);
            self.mark();
        }
    }

    pub fn apply_resize_op(&mut self, window_id: u32, new_rect: Rect, output: (i32, i32)) {
        if !self.is_tiled(window_id) {
            return;
        }
        let screen = self.screen_of(window_id);
        let Some((dir, is_first)) = self.tree(screen).parent_split(window_id) else {
            return;
        };
        let area = usable_area(output.0, output.1, MENUBAR_HEIGHT);
        let laid = self.tree(screen).layout(area);
        let Some((_, old)) = laid.iter().find(|(id, _)| *id == window_id) else {
            return;
        };
        // Parent cell ≈ old rect plus inner gap toward the sibling.
        let parent_span = match dir {
            Dir::Row => old.w + crate::tiling::GAP_INNER,
            Dir::Col => old.h + crate::tiling::GAP_INNER,
        };
        // Reconstruct parent size from the two children if possible.
        let sibling = laid.iter().find(|(id, _)| *id != window_id);
        let parent_span = sibling
            .map(|(_, s)| match dir {
                Dir::Row => old.w + crate::tiling::GAP_INNER + s.w,
                Dir::Col => old.h + crate::tiling::GAP_INNER + s.h,
            })
            .unwrap_or(parent_span);
        let new_span = match dir {
            Dir::Row => new_rect.w,
            Dir::Col => new_rect.h,
        };
        let mut ratio = new_span as f32 / parent_span.max(1) as f32;
        if !is_first {
            ratio = 1.0 - ratio;
        }
        self.tree_mut(screen).set_ratio(window_id, ratio);
        self.mark();
    }

    /// Frames for tiled / fullscreen / cinema windows on the **current**
    /// screen. Floats are omitted (shell must not re-frame them).
    pub fn current_managed_frames(
        &self,
        output_w: i32,
        output_h: i32,
    ) -> Vec<FrameUpdate> {
        let mut frames = Vec::new();
        let area = usable_area(output_w, output_h, MENUBAR_HEIGHT);
        let fs = fullscreen_area(output_w, output_h, MENUBAR_HEIGHT);
        let cinema = cinema_area(output_w, output_h);
        for (wid, r) in self.tree(self.current).layout(area) {
            match self.special.get(&wid) {
                Some(Mode::Fullscreen) => {
                    frames.push(frame_from_rect(wid, fs, false));
                }
                Some(Mode::Cinema) => {
                    frames.push(frame_from_rect(wid, cinema, true));
                }
                _ => frames.push(frame_from_rect(wid, r, false)),
            }
        }
        // Fullscreen/cinema floats (not in the tree).
        for (&wid, mode) in &self.special {
            if self.screen_of(wid) != self.current {
                continue;
            }
            if self.tree(self.current).contains(wid) {
                continue;
            }
            match mode {
                Mode::Fullscreen => frames.push(frame_from_rect(wid, fs, false)),
                Mode::Cinema => frames.push(frame_from_rect(wid, cinema, true)),
                Mode::Tiled | Mode::Float => {}
            }
        }
        frames
    }

    pub fn snapshot(&self, known: &[Window], floats: &HashMap<String, ScreenFloat>) -> ScreenLayout {
        let trees = self
            .trees
            .iter()
            .map(|t| t.persist_nodes())
            .collect();
        let mut float_map = floats.clone();
        for w in known {
            if w.app_id == "sola-shell" {
                continue;
            }
            if self.mode(w.window_id) == Mode::Float {
                float_map.entry(w.app_id.clone()).or_insert(ScreenFloat {
                    app_id: w.app_id.clone(),
                    screen: self.screen_of(w.window_id),
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                });
                if let Some(f) = float_map.get_mut(&w.app_id) {
                    f.screen = self.screen_of(w.window_id);
                }
            }
        }
        let mut special = HashMap::new();
        for w in known {
            match self.special.get(&w.window_id) {
                Some(Mode::Fullscreen) => {
                    special.insert(w.app_id.clone(), "fullscreen".into());
                }
                Some(Mode::Cinema) => {
                    special.insert(w.app_id.clone(), "cinema".into());
                }
                _ => {}
            }
        }
        ScreenLayout {
            current: self.current,
            former: self.former,
            trees,
            floats: float_map,
            special,
        }
    }

    pub fn restore(&mut self, layout: ScreenLayout) {
        self.current = layout.current.clamp(1, SCREEN_COUNT);
        self.former = layout.former.clamp(1, SCREEN_COUNT);
        for (i, node) in layout.trees.into_iter().take(SCREEN_COUNT as usize).enumerate() {
            self.trees[i] = Tree::from_persist(node);
        }
        self.mark();
    }

    pub fn restore_special_for(&mut self, window_id: u32, app_id: &str, layout: &ScreenLayout) {
        match layout.special.get(app_id).map(String::as_str) {
            Some("fullscreen") => {
                self.special.insert(window_id, Mode::Fullscreen);
            }
            Some("cinema") => {
                self.special.insert(window_id, Mode::Cinema);
            }
            _ => {}
        }
        if let Some(f) = layout.floats.get(app_id) {
            self.screen_of.insert(window_id, f.screen.clamp(1, SCREEN_COUNT));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_wraps() {
        let mut s = ScreenState::default();
        s.cycle(true);
        assert_eq!(s.current(), 2);
        s.switch_to(5);
        s.cycle(true);
        assert_eq!(s.current(), 1);
        s.cycle(false);
        assert_eq!(s.current(), 5);
    }

    #[test]
    fn former_is_previous_current() {
        let mut s = ScreenState::default();
        s.switch_to(3);
        s.switch_to(5);
        assert_eq!(s.former(), 3);
        s.switch_former();
        assert_eq!(s.current(), 3);
    }
}
