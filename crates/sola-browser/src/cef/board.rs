//! sola-design board tabs (`Cmd::OpenBoard`): each board's own viewport, a
//! pinned layout, and detail renders.
//!
//! A board lays out at its CSS size and rasters at its scale through page
//! zoom (this CEF's OSR ignores the device scale factor). DevTools device
//! metrics pin that layout size, so the OSR surface can be larger than the
//! board without the page seeing a resize. A detail request re-rasters part
//! of the board at any scale with the DevTools viewport override: a
//! compositor transform, so nothing relayouts and text cannot shift.
//!
//! Every request has a `seq` from the app. Frames carry the newest `seq`
//! Chromium has acknowledged (the renderer applied it before answering), so
//! the app knows which picture each frame is.

use std::cell::RefCell;
use std::collections::HashMap;

use cef::{BrowserHost, DictionaryValue, ImplDictionaryValue};

use crate::cef::agent::{cdp, dict, set_bool, set_f64, set_int};
use crate::engine::{BoardDetail, BoardSize, TabId};

/// A board tab's state on the helper.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoardView {
    pub size: BoardSize,
    /// The newest request: `seq` and what it shows.
    pub want: (u32, Option<BoardDetail>),
    /// The newest request Chromium has applied. Frames carry it.
    pub tag: u32,
}

impl BoardView {
    pub fn new(size: BoardSize) -> Self {
        let scale = if size.scale.is_finite() && size.scale > 0.0 {
            size.scale
        } else {
            1.0
        };
        let mut size = BoardSize {
            width: size.width.max(1),
            height: size.height.max(1),
            scale,
            surface: size.surface,
        };
        let (w, h) = size.pixels();
        size.surface = (size.surface.0.max(w), size.surface.1.max(h));
        Self {
            size,
            want: (0, None),
            tag: 0,
        }
    }

    /// OSR view size in px.
    pub fn surface(&self) -> (u32, u32) {
        self.size.surface
    }

    /// Chromium zoom level for the raster scale (`factor = 1.2^level`).
    pub fn zoom_level(&self) -> f64 {
        self.size.scale.ln() / 1.2_f64.ln()
    }

    /// Board CSS px → view px for input. Input is not moved by the viewport
    /// override, so this holds in detail mode too.
    pub fn to_view(&self, v: i32) -> i32 {
        (f64::from(v) * self.size.scale).round() as i32
    }
}

/// `Emulation.setDeviceMetricsOverride` for a board: layout pinned to the
/// board, plus the viewport override when `detail` is set.
fn override_params(size: &BoardSize, detail: Option<Viewport>) -> DictionaryValue {
    let (w, h) = size.pixels();
    let mut params = dict();
    set_int(&mut params, "width", w as i32);
    set_int(&mut params, "height", h as i32);
    set_f64(&mut params, "deviceScaleFactor", 0.0);
    set_bool(&mut params, "mobile", false);
    set_bool(&mut params, "dontSetVisibleSize", true);
    if let Some(v) = detail {
        let mut vp = dict();
        set_f64(&mut vp, "x", v.x);
        set_f64(&mut vp, "y", v.y);
        set_f64(&mut vp, "width", v.width);
        set_f64(&mut vp, "height", v.height);
        set_f64(&mut vp, "scale", v.scale);
        let key = cef::CefString::from("viewport");
        let _ = params.set_dictionary(Some(&key), Some(&mut vp));
    }
    params
}

/// The DevTools viewport for a detail: document coordinates in zoomed px
/// (`scroll` is the page's CSS scroll offset), scale relative to the board's
/// raster scale. Chromium drops an override whose `x` is negative.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Viewport {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
}

fn viewport(size: &BoardSize, d: &BoardDetail, scroll: (f64, f64)) -> Viewport {
    let z = size.scale;
    let rel = d.scale / z;
    Viewport {
        x: ((d.x + scroll.0) * z).max(0.0),
        y: ((d.y + scroll.1) * z).max(0.0),
        width: f64::from(size.surface.0) / rel,
        height: f64::from(size.surface.1) / rel,
        scale: rel,
    }
}

/// CSS scroll offset from a `Page.getLayoutMetrics` result.
fn scroll_of(json: &str) -> (f64, f64) {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let vp = &v["cssVisualViewport"];
    (
        vp["pageX"].as_f64().unwrap_or(0.0),
        vp["pageY"].as_f64().unwrap_or(0.0),
    )
}

#[derive(Clone, Copy, Debug)]
enum Step {
    /// Scroll offset asked for; the override follows.
    Metrics,
    /// Override sent; its answer makes `seq` the frames' tag.
    Override,
}

#[derive(Clone, Copy, Debug)]
struct Job {
    tab: TabId,
    seq: u32,
    step: Step,
}

thread_local! {
    static JOBS: RefCell<HashMap<(i32, i32), Job>> = RefCell::new(HashMap::new());
}

fn put(browser_id: i32, message_id: i32, job: Job) {
    if message_id <= 0 {
        tracing::warn!(?job, "board devtools call not submitted");
        return;
    }
    JOBS.with(|j| j.borrow_mut().insert((browser_id, message_id), job));
}

/// Put `view.want` in force: the pinned layout alone, or (after reading the
/// scroll offset) with its detail viewport.
pub(crate) fn request(host: &BrowserHost, browser_id: i32, tab: TabId, view: &BoardView) {
    let (seq, detail) = view.want;
    match detail {
        None => {
            let mut params = override_params(&view.size, None);
            let mid = cdp(
                host,
                "Emulation.setDeviceMetricsOverride",
                Some(&mut params),
            );
            put(
                browser_id,
                mid,
                Job {
                    tab,
                    seq,
                    step: Step::Override,
                },
            );
        }
        Some(_) => {
            let mid = cdp(host, "Page.getLayoutMetrics", None);
            put(
                browser_id,
                mid,
                Job {
                    tab,
                    seq,
                    step: Step::Metrics,
                },
            );
        }
    }
}

/// What a board's DevTools answer means for its tab.
pub(crate) enum Answer {
    /// Not a board call.
    NotOurs,
    Done,
    /// The override for `seq` is in force: tag frames with it.
    Applied {
        tab: TabId,
        seq: u32,
    },
    /// Scroll offset for request `seq`: send its override if still current.
    Scroll {
        tab: TabId,
        seq: u32,
        scroll: (f64, f64),
    },
}

pub(crate) fn on_result(browser_id: i32, message_id: i32, success: bool, json: &str) -> Answer {
    let Some(job) = JOBS.with(|j| j.borrow_mut().remove(&(browser_id, message_id))) else {
        return Answer::NotOurs;
    };
    if !success {
        tracing::warn!(?job, result = json, "board devtools call failed");
        return Answer::Done;
    }
    match job.step {
        Step::Metrics => Answer::Scroll {
            tab: job.tab,
            seq: job.seq,
            scroll: scroll_of(json),
        },
        Step::Override => Answer::Applied {
            tab: job.tab,
            seq: job.seq,
        },
    }
}

/// Send the detail override for `view.want` (the scroll offset is in).
pub(crate) fn send_detail(
    host: &BrowserHost,
    browser_id: i32,
    tab: TabId,
    view: &BoardView,
    scroll: (f64, f64),
) {
    let (seq, Some(d)) = view.want else {
        return;
    };
    let mut params = override_params(&view.size, Some(viewport(&view.size, &d, scroll)));
    let mid = cdp(
        host,
        "Emulation.setDeviceMetricsOverride",
        Some(&mut params),
    );
    put(
        browser_id,
        mid,
        Job {
            tab,
            seq,
            step: Step::Override,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(w: u32, h: u32, scale: f64, surface: (u32, u32)) -> BoardSize {
        BoardSize {
            width: w,
            height: h,
            scale,
            surface,
        }
    }

    #[test]
    fn surface_is_at_least_the_board() {
        let v = BoardView::new(size(390, 844, 2.0, (2560, 1440)));
        assert_eq!(v.surface(), (2560, 1688));
        assert_eq!(v.size.pixels(), (780, 1688));
        let v = BoardView::new(size(10, 10, f64::NAN, (0, 0)));
        assert_eq!(v.size.scale, 1.0);
        assert_eq!(v.surface(), (10, 10));
    }

    #[test]
    fn viewport_is_document_px_relative_to_the_raster() {
        let s = size(1440, 900, 2.0, (2880, 1800));
        let d = BoardDetail {
            x: 100.0,
            y: 50.0,
            scale: 8.0,
        };
        let v = viewport(&s, &d, (0.0, 10.0));
        assert_eq!((v.x, v.y), (200.0, 120.0));
        assert_eq!(v.scale, 4.0);
        assert_eq!((v.width, v.height), (720.0, 450.0));
        // Never negative: Chromium would drop the override.
        let d = BoardDetail {
            x: -0.2,
            y: 0.0,
            scale: 3.0,
        };
        assert_eq!(viewport(&s, &d, (0.0, 0.0)).x, 0.0);
    }

    #[test]
    fn scroll_reads_the_visual_viewport() {
        let json = r#"{"cssVisualViewport":{"pageX":12.5,"pageY":300,"clientWidth":1440}}"#;
        assert_eq!(scroll_of(json), (12.5, 300.0));
        assert_eq!(scroll_of("{}"), (0.0, 0.0));
    }

    #[test]
    fn input_maps_through_the_raster_scale() {
        let v = BoardView::new(size(100, 100, 2.0, (0, 0)));
        assert_eq!(v.to_view(13), 26);
        assert!((v.zoom_level() - 2.0_f64.ln() / 1.2_f64.ln()).abs() < 1e-12);
    }
}
