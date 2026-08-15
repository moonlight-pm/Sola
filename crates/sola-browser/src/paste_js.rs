//! Page ↔ chrome clipboard helpers (JS inject + selection extract).

/// Console prefix for a page copy (JSON string payload).
pub const COPY_PREFIX: &str = "__sola_clipboard_copy__";

/// Console prefix for a ⌘-click hit-test (`JSON.stringify` of the href).
pub const LINK_HIT_PREFIX: &str = "__sola_linkhit__";

/// Install-once hook so in-page Copy buttons (`navigator.clipboard.writeText`,
/// `clipboard.write`, `execCommand('copy')`) report text through
/// [`COPY_PREFIX`]. The helper has no Wayland seat — Chromium's own
/// clipboard never reaches iced without this.
pub fn clipboard_bridge_script() -> String {
    format!(
        r#"(function(){{
  if (window.__sola_clip_hook) return;
  window.__sola_clip_hook = 1;
  function report(t){{
    if (t==null) return;
    t=String(t);
    if (!t) return;
    try{{ console.info('{prefix}'+JSON.stringify(t)); }}catch(e){{}}
  }}
  try{{
    var clip=navigator.clipboard;
    if (clip){{
      if (clip.writeText){{
        var owt=clip.writeText.bind(clip);
        clip.writeText=function(text){{ report(text); return owt(text); }};
      }}
      if (clip.write){{
        var ow=clip.write.bind(clip);
        clip.write=function(items){{
          try{{
            var it=items&&items[0];
            if (it&&it.getType){{
              it.getType('text/plain').then(function(b){{return b.text();}}).then(report).catch(function(){{}});
            }}
          }}catch(e){{}}
          return ow(items);
        }};
      }}
    }}
  }}catch(e){{}}
  function onCopy(e){{
    try{{
      var t=(e.clipboardData&&e.clipboardData.getData('text/plain'))||'';
      if (!t&&window.getSelection) t=window.getSelection().toString();
      report(t);
    }}catch(err){{}}
  }}
  document.addEventListener('copy', onCopy, true);
  document.addEventListener('copy', onCopy, false);
}})();"#,
        prefix = COPY_PREFIX
    )
}

/// IIFE that reports the current selection via `console.info`.
pub fn copy_selection_script() -> String {
    format!(
        r#"(function(){{
  var t='';
  var el=document.activeElement;
  var tag=el&&(el.tagName||'').toLowerCase();
  if(el&&(tag==='input'||tag==='textarea')&&typeof el.selectionStart==='number'){{
    t=(el.value||'').slice(el.selectionStart,el.selectionEnd);
  }}else if(window.getSelection){{
    t=window.getSelection().toString();
  }}
  try{{ console.info('{prefix}'+JSON.stringify(t)); }}catch(e){{}}
  return t;
}})();"#,
        prefix = COPY_PREFIX
    )
}

/// IIFE that inserts `text` at the caret of the focused input / textarea /
/// contenteditable. Values are JSON-string-literal escaped.
pub fn paste_into_focused_script(text: &str) -> String {
    let t = js_string_literal(text);
    format!(
        r#"(function(){{
  var t={t};
  var el=document.activeElement;
  if(!el) return false;
  var tag=(el.tagName||'').toLowerCase();
  if(tag==='input'||tag==='textarea'){{
    var start=typeof el.selectionStart==='number'?el.selectionStart: (el.value||'').length;
    var end=typeof el.selectionEnd==='number'?el.selectionEnd:start;
    var v=el.value||'';
    var proto=(tag==='textarea'?window.HTMLTextAreaElement:window.HTMLInputElement);
    proto=proto&&proto.prototype;
    var desc=proto&&Object.getOwnPropertyDescriptor(proto,'value');
    var next=v.slice(0,start)+t+v.slice(end);
    if(desc&&desc.set) desc.set.call(el,next); else el.value=next;
    try{{ el.selectionStart=el.selectionEnd=start+t.length; }}catch(e){{}}
    el.dispatchEvent(new Event('input',{{bubbles:true}}));
    el.dispatchEvent(new Event('change',{{bubbles:true}}));
    return true;
  }}
  if(el.isContentEditable){{
    try{{ return !!document.execCommand('insertText',false,t); }}catch(e){{}}
  }}
  return false;
}})();"#
    )
}

/// IIFE: href under view-pixel `(x, y)` in the main document (walks
/// iframes when same-origin). Reports via `console.info`.
pub fn link_hit_script(x: i32, y: i32) -> String {
    format!(
        r#"(function(x,y){{
  function hrefAt(doc,x,y){{
    if(!doc) return '';
    var el=doc.elementFromPoint(x,y);
    if(!el) return '';
    var n=el;
    while(n){{
      var tag=(n.tagName||'').toUpperCase();
      if(tag==='A'||tag==='AREA'){{ return n.href||''; }}
      n=n.parentElement;
    }}
    var tag=(el.tagName||'').toUpperCase();
    if((tag==='IFRAME'||tag==='FRAME')&&el.contentDocument){{
      try{{
        var r=el.getBoundingClientRect();
        return hrefAt(el.contentDocument,x-r.left,y-r.top);
      }}catch(e){{}}
    }}
    return '';
  }}
  var href=hrefAt(document,x,y);
  try{{ console.info('{prefix}'+JSON.stringify(href)); }}catch(e){{}}
  return href;
}})({x},{y});"#,
        prefix = LINK_HIT_PREFIX
    )
}

/// Parse a `JSON.stringify`'d string (`"hello\n"` → `hello` + newline).
pub fn parse_js_json_string(raw: &str) -> String {
    let s = raw.trim();
    let Some(inner) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) else {
        return raw.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

fn js_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_quotes() {
        let s = paste_into_focused_script(r#"a"b"#);
        assert!(s.contains(r#"a\"b"#));
        assert!(!s.contains('\0'));
    }

    #[test]
    fn copy_script_uses_prefix() {
        let s = copy_selection_script();
        assert!(s.contains(COPY_PREFIX));
        assert!(s.contains("getSelection"));
    }

    #[test]
    fn clipboard_bridge_hooks_write_text() {
        let s = clipboard_bridge_script();
        assert!(s.contains(COPY_PREFIX));
        assert!(s.contains("writeText"));
        assert!(s.contains("__sola_clip_hook"));
        assert!(s.contains("addEventListener('copy'"));
    }

    #[test]
    fn parse_json_string_roundtrip() {
        assert_eq!(parse_js_json_string(r#""hello""#), "hello");
        assert_eq!(parse_js_json_string(r#""a\"b""#), "a\"b");
        assert_eq!(parse_js_json_string(r#""a\nb""#), "a\nb");
        assert_eq!(parse_js_json_string("bare"), "bare");
    }

    #[test]
    fn link_hit_script_embeds_coords_and_prefix() {
        let s = link_hit_script(12, 34);
        assert!(s.contains(LINK_HIT_PREFIX));
        assert!(s.contains("12,34"));
        assert!(s.contains("elementFromPoint"));
    }
}
