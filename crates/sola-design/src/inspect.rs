//! `solactl design inspect`: what an artboard actually rendered.
//!
//! One page script answers the questions an agent cannot get from the
//! files: the viewport and document size (is content cut off below the
//! artboard?), the box and computed styles of an element, and elements that
//! overflow the viewport or are clipped by `overflow: hidden`.

use crate::select::{PATH_FN, json_string};

/// Content height for `height = "auto"`: the body's scroll height, or the
/// root box when body margins add to it. One formula for [`SETTLE`] and
/// [`WATCH_HEIGHT`].
macro_rules! content_height {
    () => {
        "Math.ceil(Math.max(
    document.body ? document.body.scrollHeight : 0,
    document.documentElement.getBoundingClientRect().height))"
    };
}

/// Run once a board has loaded: wait for web fonts and two frames (so the
/// layout is final), then report the content height and every local
/// resource the page used — `file://` or Design's `*.localhost` origin — to
/// watch files that live outside `design/`.
pub const SETTLE: &str = concat!(
    r#"(async () => {
  await document.fonts.ready;
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  const urls = new Set([location.href]);
  for (const e of performance.getEntriesByType('resource')) urls.add(e.name);
  for (const s of document.styleSheets) {
    if (s.href) urls.add(s.href);
    try {
      for (const r of s.cssRules) if (r.styleSheet && r.styleSheet.href) urls.add(r.styleSheet.href);
    } catch (_) {}
  }
  const height = "#,
    content_height!(),
    r#";
  const local = (u) => {
    try {
      return u.startsWith('file://') || new URL(u).hostname.endsWith('.localhost');
    } catch (_) {
      return false;
    }
  };
  const nav = performance.getEntriesByType('navigation')[0];
  const status = (nav && nav.responseStatus) || 0;
  return { height, resources: [...urls].filter(local), status, url: location.href };
})()"#
);

/// A function of `known` (call it as `WATCH_HEIGHT(<px>)`): resolves with
/// `{height, reflow}` once the content height differs from `known`, measured
/// 150 ms after boxes stop changing (the root, body, and body's children,
/// observed once per document). `reflow`: the viewport changed since the
/// page last measured — the board was resized, not new content. Design asks
/// again after each answer; a navigation fails the wait.
pub const WATCH_HEIGHT: &str = concat!(
    r#"(async (known) => {
  await document.fonts.ready;
  const w = window.__solaDesignHeight || (window.__solaDesignHeight = (() => {
    const s = { waiting: [], timer: 0, viewport: innerHeight };
    const check = () => requestAnimationFrame(() => {
      const height = "#,
    content_height!(),
    r#";
      const reflow = innerHeight !== s.viewport;
      s.viewport = innerHeight;
      s.waiting = s.waiting.filter(([k, done]) => k === height || (done({ height, reflow }), false));
    });
    s.poke = () => {
      clearTimeout(s.timer);
      s.timer = setTimeout(check, 150);
    };
    const boxes = new ResizeObserver(s.poke);
    const observe = () => {
      boxes.observe(document.documentElement);
      if (!document.body) return;
      boxes.observe(document.body);
      for (const el of document.body.children) boxes.observe(el);
    };
    observe();
    if (document.body) {
      new MutationObserver(() => { observe(); s.poke(); }).observe(document.body, { childList: true });
    }
    return s;
  })());
  return await new Promise((done) => {
    w.waiting.push([known, done]);
    w.poke();
  });
})"#
);

/// Report for `selector` (or just the page when `None`). `all`: one compact
/// entry (selector path, tag, box, a little text) per match instead of the
/// first match's styles and the issue scan.
pub fn script(selector: Option<&str>, all: bool) -> String {
    let sel = selector.map_or_else(|| "null".to_string(), json_string);
    let body = if all { ALL_BODY } else { BODY };
    format!("(() => {{ {PATH_FN} const sel = {sel}; {body} }})()")
}

const ALL_BODY: &str = r#"
const doc = document.documentElement;
let elements = [], error = null;
try {
  elements = [...document.querySelectorAll(sel || 'body > *')].slice(0, 300).map((el) => {
    const r = el.getBoundingClientRect();
    return {
      selector: cssPath(el),
      tag: el.localName,
      rect: { x: r.x, y: r.y, width: r.width, height: r.height },
      text: (el.innerText || '').trim().slice(0, 60),
    };
  });
} catch (e) {
  error = String((e && e.message) || e);
}
return {
  viewport: { width: innerWidth, height: innerHeight, scale: devicePixelRatio },
  document: { width: doc.scrollWidth, height: doc.scrollHeight },
  selector: sel,
  matches: elements.length,
  elements,
  error,
};
"#;

/// Computed styles worth reporting; the rest is noise for layout work.
const BODY: &str = r#"
const STYLES = ['display', 'position', 'box-sizing', 'width', 'height', 'margin', 'padding',
  'border', 'border-radius', 'color', 'background-color', 'font-family', 'font-size',
  'font-weight', 'line-height', 'letter-spacing', 'text-align', 'white-space', 'gap',
  'flex-direction', 'justify-content', 'align-items', 'grid-template-columns', 'overflow',
  'opacity', 'box-shadow', 'z-index'];
const box = (r) => ({ x: r.x, y: r.y, width: r.width, height: r.height });
let element = null, matches = 0, error = null;
if (sel) {
  try {
    const all = document.querySelectorAll(sel);
    matches = all.length;
    const el = all[0];
    if (el) {
      const cs = getComputedStyle(el);
      const styles = {};
      for (const k of STYLES) styles[k] = cs.getPropertyValue(k);
      element = {
        selector: cssPath(el),
        tag: el.localName,
        text: (el.innerText || '').trim().slice(0, 200),
        rect: box(el.getBoundingClientRect()),
        styles,
      };
    }
  } catch (e) {
    error = String((e && e.message) || e);
  }
}
const vw = innerWidth, vh = innerHeight;
const issues = [];
const flagged = new Set();
const under = (e) => {
  for (let p = e.parentElement; p; p = p.parentElement) if (flagged.has(p)) return true;
  return false;
};
for (const e of document.body ? document.body.querySelectorAll('*') : []) {
  if (issues.length >= 25) break;
  const r = e.getBoundingClientRect();
  if (r.width === 0 || r.height === 0) continue;
  const cs = getComputedStyle(e);
  let kind = null;
  if (r.right > vw + 1 || r.left < -1) kind = 'overflows-viewport';
  else if (/hidden|clip/.test(cs.overflowX) && e.scrollWidth > e.clientWidth + 1) kind = 'clipped-x';
  else if (/hidden|clip/.test(cs.overflowY) && e.scrollHeight > e.clientHeight + 1) kind = 'clipped-y';
  if (!kind || under(e)) continue;
  flagged.add(e);
  issues.push({ kind, selector: cssPath(e), rect: box(r) });
}
const doc = document.documentElement;
return {
  url: location.href,
  title: document.title,
  viewport: { width: vw, height: vh, scale: devicePixelRatio },
  document: { width: doc.scrollWidth, height: doc.scrollHeight },
  cut_off_below: doc.scrollHeight > vh + 1,
  selector: sel,
  matches,
  element,
  error,
  issues,
};
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_the_selector_as_a_string_literal() {
        let s = script(Some("main > .card[data-x=\"1\"]"), false);
        assert!(s.contains(r#"const sel = "main > .card[data-x=\"1\"]";"#), "{s}");
        assert!(s.contains("const cssPath"));
        assert!(s.starts_with("(() => {") && s.ends_with("})()"));
        assert!(script(None, false).contains("const sel = null;"));
    }

    #[test]
    fn all_mode_lists_matches_compactly() {
        let s = script(Some("section"), true);
        assert!(s.contains("querySelectorAll(sel || 'body > *')"));
        assert!(!s.contains("STYLES"));
        assert!(SETTLE.contains("document.fonts.ready"));
    }
}
