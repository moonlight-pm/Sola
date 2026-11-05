//! A page's videos, for the desk: sola-river leaves the desk's opacity off
//! them, and a watched one holds the idle fade (`Topic::WindowVideos`).
//!
//! [`SCRIPT`] runs in every page's top frame from its start. It is handed a
//! native function (the page never sees it) and calls it with the videos in
//! view whenever they change: `[[x0, y0, x1, y1, watched], …]` in the
//! view's pixels (CSS pixels × `devicePixelRatio`; CEF paints at 1×). A
//! video counts when it shows a frame and is at least [`SHARE`] of the
//! view, so the page's preview thumbnails do not; it is watched when it
//! plays with sound and at least half of it is in view. The chrome places
//! them in the page's window ([`in_window`]).
//!
//! Videos in a frame of another page (an embedded player) are not seen yet.

use serde::{Deserialize, Serialize};
use sola_bus::topics::WindowVideo;

/// A video counts when it is at least this share of the view's area.
pub const SHARE: f64 = 0.15;

/// The watcher: a function of the native `post`, evaluated at the top
/// frame's start. It keeps its own references to what it uses, taken
/// before the page's scripts run.
pub const SCRIPT: &str = r#"(function (post) {
  'use strict';
  if (window.top !== window) return;
  var SHARE = __SHARE__;
  var raf = window.requestAnimationFrame.bind(window);
  var stringify = JSON.stringify;
  var videos = document.getElementsByTagName('video');
  var last = null;
  var queued = false;

  function measure() {
    queued = false;
    var out = [];
    var playing = false;
    if (document.visibilityState === 'visible') {
      var vw = window.innerWidth, vh = window.innerHeight;
      var dpr = window.devicePixelRatio || 1;
      for (var i = 0; i < videos.length; i++) {
        var v = videos[i];
        if (v.readyState < 2 || !v.videoWidth) continue;
        if (v.checkVisibility && !v.checkVisibility({ opacityProperty: true, visibilityProperty: true })) continue;
        var r = v.getBoundingClientRect();
        if (r.width * r.height < SHARE * vw * vh) continue;
        var x0 = Math.max(r.left, 0), y0 = Math.max(r.top, 0);
        var x1 = Math.min(r.right, vw), y1 = Math.min(r.bottom, vh);
        if (x1 <= x0 || y1 <= y0) continue;
        var plays = !v.paused && !v.ended;
        playing = playing || plays;
        var watched = plays && !v.muted && v.volume > 0
          && 2 * (x1 - x0) * (y1 - y0) >= r.width * r.height;
        out.push([Math.ceil(x0 * dpr), Math.ceil(y0 * dpr),
                  Math.floor(x1 * dpr), Math.floor(y1 * dpr), watched ? 1 : 0]);
      }
    }
    var json = stringify(out);
    if (json !== last) {
      last = json;
      try { post(json); } catch (e) {}
    }
    // While one plays, follow it every frame (scrolling, the player moving).
    if (playing) soon();
  }

  function soon() {
    if (queued) return;
    queued = true;
    raf(measure);
  }

  ['loadeddata', 'play', 'playing', 'pause', 'ended', 'emptied', 'volumechange',
   'resize', 'seeked'].forEach(function (t) {
    window.addEventListener(t, soon, true);
  });
  window.addEventListener('scroll', soon, { capture: true, passive: true });
  document.addEventListener('fullscreenchange', soon);
  // Hidden, no frames come: say so at once.
  document.addEventListener('visibilitychange', measure);
  // A video moved or removed with nothing else to tell.
  setInterval(function () {
    if (videos.length || last !== '[]') soon();
  }, 500);
  // A new document has none yet: the last page's go.
  measure();
})"#;

/// [`SCRIPT`] ready to evaluate.
pub fn script() -> String {
    SCRIPT.replace("__SHARE__", &SHARE.to_string())
}

/// One video in view, in the view's pixels: its corners, and whether it is
/// being watched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageVideo {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
    pub watched: bool,
}

/// The watcher's report. `None`: not one.
pub fn parse(json: &str) -> Option<Vec<PageVideo>> {
    let rows: Vec<[i64; 5]> = serde_json::from_str(json).ok()?;
    let clamp = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    Some(
        rows.into_iter()
            .map(|[x0, y0, x1, y1, watched]| PageVideo {
                x0: clamp(x0),
                y0: clamp(y0),
                x1: clamp(x1),
                y1: clamp(y1),
                watched: watched != 0,
            })
            .filter(|v| v.x1 > v.x0 && v.y1 > v.y0)
            .collect(),
    )
}

/// The page's videos in its window: the page view sits at `origin` (the
/// window's logical pixels) and paints `scale` view pixels to one. Each is
/// rounded inward, so no edge of the page around a video goes opaque.
pub fn in_window(videos: &[PageVideo], origin: (f32, f32), scale: f32) -> Vec<WindowVideo> {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    videos
        .iter()
        .filter_map(|v| {
            let x0 = (origin.0 + v.x0 as f32 / scale).ceil() as i32;
            let y0 = (origin.1 + v.y0 as f32 / scale).ceil() as i32;
            let x1 = (origin.0 + v.x1 as f32 / scale).floor() as i32;
            let y1 = (origin.1 + v.y1 as f32 / scale).floor() as i32;
            (x1 > x0 && y1 > y0).then(|| WindowVideo {
                x: x0,
                y: y0,
                w: (x1 - x0) as u32,
                h: (y1 - y0) as u32,
                watched: v.watched,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_watchers_report_reads() {
        let videos = parse("[[0,96,2560,1536,1],[10,10,10,50,0],[5,6,7,8,0]]").unwrap();
        assert_eq!(
            videos,
            vec![
                PageVideo { x0: 0, y0: 96, x1: 2560, y1: 1536, watched: true },
                PageVideo { x0: 5, y0: 6, x1: 7, y1: 8, watched: false },
            ]
        );
        assert_eq!(parse("[]"), Some(Vec::new()));
        assert_eq!(parse("not json"), None);
    }

    #[test]
    fn a_video_is_placed_in_its_window_rounded_inward() {
        let v = PageVideo { x0: 1, y0: 0, x1: 2561, y1: 1441, watched: true };
        // The page under a 40 px bar, at 2× view pixels.
        let placed = in_window(&[v], (0.0, 41.0), 2.0);
        assert_eq!(placed, vec![WindowVideo { x: 1, y: 41, w: 1279, h: 720, watched: true }]);
        // Nothing left after rounding: none.
        let sliver = PageVideo { x0: 1, y0: 0, x1: 2, y1: 100, watched: false };
        assert!(in_window(&[sliver], (0.0, 0.0), 2.0).is_empty());
    }

    #[test]
    fn the_script_has_its_share() {
        let s = script();
        assert!(s.contains("var SHARE = 0.15;"));
        assert!(!s.contains("__SHARE__"));
    }
}
