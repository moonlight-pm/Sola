//! `solactl design inspect`: what an artboard actually rendered.
//!
//! One page script answers the questions an agent cannot get from the
//! files: the viewport and document size (is content cut off below the
//! artboard?), the box and computed styles of an element, and elements that
//! overflow the viewport or are clipped by `overflow: hidden`.

use crate::select::{PATH_FN, json_string};

/// Content height of the page, for `height = "auto"` artboards. The body's
/// scroll height (shrinks with content) or the root box (body margins),
/// whichever is taller.
pub const MEASURE_HEIGHT: &str = "Math.ceil(Math.max(document.body ? document.body.scrollHeight : 0, \
     document.documentElement.getBoundingClientRect().height))";

/// Report for `selector` (or just the page when `None`).
pub fn script(selector: Option<&str>) -> String {
    let sel = selector.map_or_else(|| "null".to_string(), json_string);
    format!("(() => {{ {PATH_FN} const sel = {sel}; {BODY} }})()")
}

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
        let s = script(Some("main > .card[data-x=\"1\"]"));
        assert!(s.contains(r#"const sel = "main > .card[data-x=\"1\"]";"#), "{s}");
        assert!(s.contains("const cssPath"));
        assert!(s.starts_with("(() => {") && s.ends_with("})()"));
        assert!(script(None).contains("const sel = null;"));
    }
}
