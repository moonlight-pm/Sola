//! Page ↔ chrome clipboard helpers (JS inject + selection extract).

/// Console prefix for a page copy (JSON string payload).
pub const COPY_PREFIX: &str = "__sola_clipboard_copy__";

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
    fn parse_json_string_roundtrip() {
        assert_eq!(parse_js_json_string(r#""hello""#), "hello");
        assert_eq!(parse_js_json_string(r#""a\"b""#), "a\"b");
        assert_eq!(parse_js_json_string(r#""a\nb""#), "a\nb");
        assert_eq!(parse_js_json_string("bare"), "bare");
    }
}
