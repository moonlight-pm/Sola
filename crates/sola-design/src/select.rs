//! Select mode: what the human pointed at, so an agent can act on "this".
//!
//! A click runs [`pick_script`] in that board (`AgentOp::Eval`). The page
//! answers with the element under the point; iced draws its outline.

use serde::Deserialize;

/// Element rect in the board's viewport (CSS px).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// What the page reports for the picked element.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Picked {
    pub selector: String,
    pub tag: String,
    #[serde(default)]
    pub text: String,
    pub rect: Rect,
    #[serde(default)]
    pub html: String,
}

/// The held selection: the picked element plus where it lives.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub canvas: crate::projects::CanvasRef,
    pub artboard: String,
    pub tab: u64,
    pub picked: Picked,
}

impl Selection {
    /// Breadcrumb for the canvas bar: `artboard › selector`.
    pub fn label(&self) -> String {
        format!("{} › {}", self.artboard, self.picked.selector)
    }

    /// `solactl design selection` payload.
    pub fn to_json(&self) -> serde_json::Value {
        let r = self.picked.rect;
        serde_json::json!({
            "project": self.canvas.project,
            "canvas": self.canvas.canvas,
            "artboard": self.artboard,
            "selector": self.picked.selector,
            "tag": self.picked.tag,
            "text": self.picked.text,
            "rect": {"x": r.x, "y": r.y, "width": r.width, "height": r.height},
            "html": self.picked.html,
        })
    }
}

/// Element at board-local CSS point `(x, y)`.
pub fn pick_script(x: f32, y: f32) -> String {
    format!("(() => {{ {PATH_FN} {PICK_FN} return pick(document.elementFromPoint({x}, {y})); }})()")
}

/// Re-find a selection after its board reloaded. A selector that no longer
/// parses answers `null` like a missing element.
pub fn requery_script(selector: &str) -> String {
    let sel = json_string(selector);
    format!(
        "(() => {{ {PATH_FN} {PICK_FN} try {{ return pick(document.querySelector({sel})); }} catch (e) {{ return null; }} }})()"
    )
}

/// `null` (nothing there) → `Ok(None)`.
pub fn parse(value: Option<&serde_json::Value>) -> Result<Option<Picked>, String> {
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(v) => serde_json::from_value(v.clone())
            .map(Some)
            .map_err(|e| format!("pick result: {e}")),
    }
}

pub(crate) fn json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// `cssPath(el)`: climbs to the nearest `#id` (or `body`), with
/// `:nth-of-type` only where siblings share a tag. Shared by page scripts.
pub(crate) const PATH_FN: &str = r#"const cssPath = (el) => {
  const seg = (e) => {
    if (e.id) return '#' + CSS.escape(e.id);
    let s = e.localName + [...e.classList].slice(0, 3).map((c) => '.' + CSS.escape(c)).join('');
    const p = e.parentElement;
    if (p) {
      const same = [...p.children].filter((c) => c.localName === e.localName);
      if (same.length > 1) s += ':nth-of-type(' + (same.indexOf(e) + 1) + ')';
    }
    return s;
  };
  const parts = [];
  for (let e = el; e && e.nodeType === 1; e = e.parentElement) {
    parts.unshift(seg(e));
    if (e.id || e === document.body) break;
  }
  return parts.join(' > ');
};"#;

/// `pick(el)` → `{selector, tag, text, rect, html}` or `null`.
const PICK_FN: &str = r#"const pick = (el) => {
  if (!el || el.nodeType !== 1) return null;
  const r = el.getBoundingClientRect();
  const html = el.outerHTML || '';
  return {
    selector: cssPath(el),
    tag: el.localName,
    text: (el.innerText || '').trim().slice(0, 200),
    rect: { x: r.x, y: r.y, width: r.width, height: r.height },
    html: html.length > 2000 ? html.slice(0, 2000) + '…' : html,
  };
};"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_pick() {
        let v = serde_json::json!({
            "selector": "body > main > button.primary",
            "tag": "button",
            "text": "Save",
            "rect": {"x": 10.0, "y": 20.5, "width": 80.0, "height": 32.0},
            "html": "<button class=\"primary\">Save</button>"
        });
        let p = parse(Some(&v)).unwrap().unwrap();
        assert_eq!(p.tag, "button");
        assert_eq!(p.rect.y, 20.5);
    }

    #[test]
    fn null_is_nothing_and_junk_is_an_error() {
        assert_eq!(parse(None).unwrap(), None);
        assert_eq!(parse(Some(&serde_json::Value::Null)).unwrap(), None);
        assert!(parse(Some(&serde_json::json!({"tag": 1}))).is_err());
    }

    #[test]
    fn scripts_embed_inputs_safely() {
        assert!(pick_script(12.0, 7.5).contains("pick(document.elementFromPoint(12, 7.5))"));
        let q = requery_script("a[title=\"x\"]");
        assert!(q.contains(r#"document.querySelector("a[title=\"x\"]")"#), "{q}");
        assert!(q.starts_with("(() => {") && q.ends_with("})()"));
    }

    #[test]
    fn selection_json_has_the_agent_fields() {
        let sel = Selection {
            canvas: crate::projects::CanvasRef {
                project: "proj-x".into(),
                canvas: "home".into(),
            },
            artboard: "phone".into(),
            tab: 4,
            picked: Picked {
                selector: "#save".into(),
                tag: "button".into(),
                text: "Save".into(),
                rect: Rect {
                    x: 1.0,
                    y: 2.0,
                    width: 3.0,
                    height: 4.0,
                },
                html: "<button id=\"save\">Save</button>".into(),
            },
        };
        let j = sel.to_json();
        assert_eq!(j["artboard"], "phone");
        assert_eq!(j["selector"], "#save");
        assert_eq!(j["rect"]["height"], 4.0);
        assert!(j.get("tab").is_none());
    }
}
