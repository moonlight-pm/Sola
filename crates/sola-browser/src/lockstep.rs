//! Option A lockstep — publish content scissor for river (stock Wayland).
//!
//! Chrome tracks its own [`WindowGeometry`] and the iced content bounds
//! (CSS layout px). Combined global scissor is emitted as sticky
//! [`Topic::BrowserContentScissor`] so sola-river can place
//! `sola.browser-content` under the hole.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use sola_bus::topics::{
    BrowserContentScissor, Topic, Window, WindowGeometry, BROWSER_CHROME_APP_ID,
};

struct LockstepState {
    chrome_wids: HashSet<u32>,
    chrome_geom: Option<(i32, i32, i32, i32)>,
    content_local: Option<(i32, i32, i32, i32)>,
    last_emitted: Option<BrowserContentScissor>,
}

fn state() -> &'static Mutex<LockstepState> {
    static S: OnceLock<Mutex<LockstepState>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(LockstepState {
            chrome_wids: HashSet::new(),
            chrome_geom: None,
            content_local: None,
            last_emitted: None,
        })
    })
}

/// Learn chrome window_ids from sticky `Topic::Windows`.
pub fn note_windows(windows: &[Window], chrome_app_id: &str) {
    let mut set = HashSet::new();
    for w in windows {
        if w.app_id == chrome_app_id || w.app_id == BROWSER_CHROME_APP_ID {
            set.insert(w.window_id);
        }
    }
    state().lock().unwrap().chrome_wids = set;
}

/// Update chrome origin/size from bus `WindowGeometry` when it is ours.
pub fn note_chrome_geometry(g: &WindowGeometry) {
    let mut st = state().lock().unwrap();
    if !st.chrome_wids.contains(&g.window_id) {
        return;
    }
    let next = (g.x, g.y, g.width, g.height);
    if st.chrome_geom == Some(next) {
        return;
    }
    st.chrome_geom = Some(next);
    drop(st);
    maybe_emit();
}

/// Update content hole from shader prepare (CSS layout coordinates).
pub fn note_content_local(x: f32, y: f32, w: f32, h: f32) {
    let next = (
        x.round() as i32,
        y.round() as i32,
        w.round().max(1.0) as i32,
        h.round().max(1.0) as i32,
    );
    let mut st = state().lock().unwrap();
    if st.content_local == Some(next) {
        return;
    }
    st.content_local = Some(next);
    drop(st);
    maybe_emit();
}

fn maybe_emit() {
    if !crate::content_plane::mode().is_wayland() {
        return;
    }
    let scissor = {
        let st = state().lock().unwrap();
        let chrome = match st.chrome_geom {
            Some(c) => c,
            None => return,
        };
        let local = match st.content_local {
            Some(l) => l,
            None => return,
        };
        BrowserContentScissor {
            x: chrome.0 + local.0,
            y: chrome.1 + local.1,
            width: local.2,
            height: local.3,
        }
    };
    if scissor.width <= 0 || scissor.height <= 0 {
        return;
    }
    {
        let mut st = state().lock().unwrap();
        if st.last_emitted.as_ref() == Some(&scissor) {
            return;
        }
        st.last_emitted = Some(scissor.clone());
    }
    if let Ok(mut bus) = sola_kit::app::bus().lock() {
        if let Err(e) = bus.emit(Topic::BrowserContentScissor(scissor.clone())) {
            tracing::warn!(error = %e, "BrowserContentScissor emit failed");
            return;
        }
        tracing::debug!(
            x = scissor.x,
            y = scissor.y,
            w = scissor.width,
            h = scissor.height,
            "BrowserContentScissor published"
        );
    }
}
