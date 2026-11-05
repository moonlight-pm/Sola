//! The videos the windows show (`Topic::WindowVideos`): the overlay leaves
//! them out, so a video is fully opaque where the rest of its window is at
//! the desk's opacity. A video being watched (playing with sound), at least
//! half of it uncovered on screen, holds the idle fade where it is.
//!
//! What covers a video is every window above its own in the composition.

use std::collections::HashMap;

use sola_bus::topics::{Topic, WindowVideo, WindowVideos};
use tracing::debug;

use super::cut::{Rect, subtract};
use crate::client::AppData;

/// One video on screen: what of it is uncovered (output-local logical
/// pixels) and how much there is to cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub uncovered: Vec<Rect>,
    /// Its area inside its window and the output.
    pub area: i64,
    pub watched: bool,
}

impl Shown {
    /// Watched, and at least half of it in sight: it holds the fade.
    pub fn holds(&self) -> bool {
        self.watched && 2 * self.uncovered.iter().map(Rect::area).sum::<i64>() >= self.area
    }
}

/// `Topic::WindowVideos` (`None`: retracted). An overlay up is cut again in
/// the next render sequence.
pub fn set(state: &mut AppData, window_id: u32, videos: Option<Vec<WindowVideo>>) {
    let videos = videos.filter(|v| !v.is_empty());
    if state.fade.videos.get(&window_id) == videos.as_ref() {
        return;
    }
    debug!(window_id, ?videos, "window videos");
    match videos {
        Some(v) => state.fade.videos.insert(window_id, v),
        None => state.fade.videos.remove(&window_id),
    };
    if state.fade.overlay.is_some() {
        state.pending.render_dirty = true;
    }
}

/// Window `window_id` is gone: its videos go, from the bus too (an app
/// that died never takes them back).
pub fn forget(state: &mut AppData, window_id: u32) {
    if state.fade.videos.remove(&window_id).is_none() {
        return;
    }
    let _ = state.bus.retract(Topic::WindowVideos(WindowVideos {
        window_id,
        videos: Vec::new(),
    }));
    if state.fade.overlay.is_some() {
        state.pending.render_dirty = true;
    }
}

/// Every video on screen now.
pub fn shown(state: &AppData) -> Vec<Shown> {
    if state.fade.videos.is_empty() {
        return Vec::new();
    }
    let Some((w, h)) = state.output_size else {
        return Vec::new();
    };
    let (ox, oy) = state.output_origin.unwrap_or((0, 0));
    let geometry = |id: u32| {
        state.registry.geometry(id).map(|g| Rect {
            x: g.x - ox,
            y: g.y - oy,
            w: g.width,
            h: g.height,
        })
    };
    shown_in(Rect { x: 0, y: 0, w, h }, &state.last_composition, geometry, &state.fade.videos)
}

/// The videos of the windows in `order` (bottom first), each less the
/// windows above its own.
fn shown_in(
    output: Rect,
    order: &[u32],
    geometry: impl Fn(u32) -> Option<Rect>,
    videos: &HashMap<u32, Vec<WindowVideo>>,
) -> Vec<Shown> {
    let mut out = Vec::new();
    for (i, &id) in order.iter().enumerate() {
        let (Some(list), Some(window)) = (videos.get(&id), geometry(id)) else {
            continue;
        };
        let above: Vec<Rect> = order[i + 1..].iter().filter_map(|&a| geometry(a)).collect();
        let window = window.intersect(&output);
        for v in list {
            let rect = Rect {
                x: window.x.saturating_add(v.x),
                y: window.y.saturating_add(v.y),
                w: i32::try_from(v.w).unwrap_or(i32::MAX),
                h: i32::try_from(v.h).unwrap_or(i32::MAX),
            }
            .intersect(&window);
            if rect.is_empty() {
                continue;
            }
            out.push(Shown {
                uncovered: subtract(rect, &above),
                area: rect.area(),
                watched: v.watched,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    fn video(x: i32, y: i32, w: u32, h: u32, watched: bool) -> WindowVideo {
        WindowVideo { x, y, w, h, watched }
    }

    #[test]
    fn a_video_sits_in_its_window_less_the_windows_above() {
        let output = r(0, 0, 1920, 1080);
        let windows = HashMap::from([(1, r(100, 50, 1200, 900)), (2, r(1000, 0, 900, 1080))]);
        let videos = HashMap::from([(1, vec![video(0, 40, 1200, 675, true)])]);
        let geometry = |id| windows.get(&id).copied();

        // Window 2 above window 1 covers the video's right edge.
        let shown = shown_in(output, &[1, 2], geometry, &videos);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].area, 1200 * 675);
        assert_eq!(shown[0].uncovered, vec![r(100, 90, 900, 675)]);
        assert!(shown[0].holds());

        // Below it: nothing covers it.
        let shown = shown_in(output, &[2, 1], geometry, &videos);
        assert_eq!(shown[0].uncovered, vec![r(100, 90, 1200, 675)]);

        // A window not in the composition is not on screen.
        assert!(shown_in(output, &[2], geometry, &videos).is_empty());
    }

    #[test]
    fn a_video_mostly_covered_or_not_watched_does_not_hold_the_fade() {
        let output = r(0, 0, 1920, 1080);
        let windows = HashMap::from([(1, r(0, 0, 1000, 1000)), (2, r(0, 0, 800, 1000))]);
        let geometry = |id| windows.get(&id).copied();
        let watched = HashMap::from([(1, vec![video(0, 0, 1000, 500, true)])]);
        let shown = shown_in(output, &[1, 2], geometry, &watched);
        assert!(!shown[0].holds(), "a fifth of it in sight");
        let muted = HashMap::from([(1, vec![video(0, 0, 1000, 500, false)])]);
        assert!(!shown_in(output, &[1], geometry, &muted)[0].holds());
    }

    #[test]
    fn a_video_is_clipped_to_its_window_and_the_output() {
        let output = r(0, 0, 1920, 1080);
        let windows = HashMap::from([(1, r(1500, 500, 800, 800))]);
        let videos = HashMap::from([(1, vec![video(-20, 0, 1000, 400, false)])]);
        let shown = shown_in(output, &[1], |id| windows.get(&id).copied(), &videos);
        assert_eq!(shown[0].uncovered, vec![r(1500, 500, 420, 400)]);
        assert_eq!(shown[0].area, 420 * 400);
    }
}
