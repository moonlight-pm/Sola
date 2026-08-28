//! Tiny CSS subset: compound + one descendant, tag/class/:hover/:not.

use crate::dom::Elem;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
    pub fn to_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Len {
    Auto,
    Px(f32),
    Percent(f32),
}

#[derive(Clone, Debug, Default)]
pub struct Computed {
    pub display_flex: bool,
    pub column: bool,
    pub width: Option<Len>,
    pub height: Option<Len>,
    pub min_width: Option<Len>,
    pub min_height: Option<Len>,
    pub max_width: Option<Len>,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub overflow_hidden: bool,
    pub overflow_scroll: bool,
    pub align_center: bool,
    pub justify_center: bool,
    pub justify_between: bool,
    pub text_align_center: bool,
    pub padding: [f32; 4], // t r b l
    pub margin: [f32; 4],
    pub gap: f32,
    pub bg: Option<Rgba>,
    pub bg2: Option<Rgba>,
    pub gradient: u8,
    pub color: Option<Rgba>,
    pub font_size: Option<f32>,
    pub font_weight: Option<u16>,
    pub font_family: Option<String>,
    /// Per-side borders `[top, right, bottom, left]`.
    pub border: [Option<(f32, Rgba)>; 4],
    pub radius: f32,
    pub translate_y: f32,
    pub absolute: bool,
    pub top: Option<Len>,
    pub left: Option<Len>,
    pub z: i32,
    pub wrap: bool,
}

#[derive(Clone, Debug)]
struct Compound {
    tag: Option<String>,
    classes: Vec<String>,
    hover: bool,
    not_classes: Vec<String>,
}

#[derive(Clone, Debug)]
struct Selector {
    ancestor: Option<Compound>,
    subject: Compound,
}

#[derive(Clone, Debug)]
struct Rule {
    selector: Selector,
    decls: Vec<(String, String)>,
}

pub struct Sheet {
    rules: Vec<Rule>,
    pub vars: std::collections::HashMap<String, String>,
}

pub fn parse_sheet(css: &str) -> Sheet {
    let mut rules = Vec::new();
    let mut vars = std::collections::HashMap::new();
    let stripped = strip_comments(css);
    for chunk in stripped.split('}') {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        let Some((sel, body)) = chunk.split_once('{') else {
            continue;
        };
        let decls = parse_decls(body);
        if sel.trim() == ":root" {
            for (k, v) in decls {
                if k.starts_with("--") {
                    vars.insert(k, v);
                }
            }
            continue;
        }
        for sel in sel.split(',') {
            let Some(selector) = parse_selector(sel.trim()) else {
                continue;
            };
            rules.push(Rule {
                selector,
                decls: decls.clone(),
            });
        }
    }
    Sheet { rules, vars }
}

fn resolve(val: &str, vars: &std::collections::HashMap<String, String>) -> String {
    let v = val.trim();
    if let Some(inner) = v.strip_prefix("var(").and_then(|s| s.strip_suffix(')')) {
        let name = inner.trim();
        if let Some(found) = vars.get(name) {
            return found.clone();
        }
    }
    v.to_string()
}

fn strip_comments(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(x) = chars.next() {
                if x == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_selector(raw: &str) -> Option<Selector> {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    if parts.len() == 1 {
        Some(Selector {
            ancestor: None,
            subject: parse_compound(parts[0])?,
        })
    } else {
        Some(Selector {
            ancestor: Some(parse_compound(parts[0])?),
            subject: parse_compound(parts[parts.len() - 1])?,
        })
    }
}

fn parse_compound(raw: &str) -> Option<Compound> {
    let mut tag = None;
    let mut classes = Vec::new();
    let mut hover = false;
    let mut not_classes = Vec::new();
    let mut rest = raw;
    if let Some(i) = rest.find(':') {
        let (head, pseudo) = rest.split_at(i);
        rest = head;
        if pseudo.contains(":hover") {
            hover = true;
        }
        if let Some(start) = pseudo.find(":not(") {
            let inner = &pseudo[start + 5..];
            if let Some(end) = inner.find(')') {
                let n = inner[..end].trim();
                if let Some(c) = n.strip_prefix('.') {
                    not_classes.push(c.to_string());
                }
            }
        }
    }
    if rest.is_empty() {
        return Some(Compound {
            tag,
            classes,
            hover,
            not_classes,
        });
    }
    let mut buf = String::new();
    let mut chars = rest.chars().peekable();
    if chars.peek().is_some_and(|c| *c != '.') {
        while let Some(c) = chars.peek().copied() {
            if c == '.' {
                break;
            }
            buf.push(c);
            chars.next();
        }
        if !buf.is_empty() {
            tag = Some(buf.clone());
        }
        buf.clear();
    }
    while let Some(c) = chars.next() {
        if c == '.' {
            if !buf.is_empty() {
                classes.push(std::mem::take(&mut buf));
            }
            continue;
        }
        buf.push(c);
    }
    if !buf.is_empty() {
        classes.push(buf);
    }
    Some(Compound {
        tag,
        classes,
        hover,
        not_classes,
    })
}

fn parse_decls(body: &str) -> Vec<(String, String)> {
    body.split(';')
        .filter_map(|d| {
            let d = d.trim();
            if d.is_empty() {
                return None;
            }
            let (k, v) = d.split_once(':')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

fn compound_matches(c: &Compound, el: &Elem, hovered: bool) -> bool {
    if let Some(tag) = &c.tag {
        if el.tag != *tag {
            return false;
        }
    }
    if c.classes.iter().any(|cl| !el.has_class(cl)) {
        return false;
    }
    if c.not_classes.iter().any(|cl| el.has_class(cl)) {
        return false;
    }
    if c.hover && !hovered {
        return false;
    }
    true
}

fn is_hovered(el: &Elem, hover_uid: Option<u32>) -> bool {
    let Some(h) = hover_uid else {
        return false;
    };
    el.uid == h || el.children.iter().any(|c| is_hovered(c, hover_uid))
}

fn rule_matches(rule: &Rule, el: &Elem, parent: Option<&Elem>, hover_uid: Option<u32>) -> bool {
    let self_hover = is_hovered(el, hover_uid);
    let parent_hover = parent.is_some_and(|p| is_hovered(p, hover_uid));
    if let Some(anc) = &rule.selector.ancestor {
        let Some(p) = parent else {
            return false;
        };
        if !compound_matches(anc, p, parent_hover) {
            return false;
        }
        compound_matches(&rule.selector.subject, el, self_hover)
    } else {
        compound_matches(&rule.selector.subject, el, self_hover)
    }
}

pub(crate) fn parse_color(v: &str) -> Option<Rgba> {
    let v = v.trim();
    if !v.starts_with('#') {
        return None;
    }
    let h = v.trim_start_matches('#');
    match h.len() {
        3 => {
            let r = u8::from_str_radix(&h[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&h[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&h[2..3].repeat(2), 16).ok()?;
            Some(Rgba::rgb(r, g, b))
        }
        6 => {
            let n = u32::from_str_radix(h, 16).ok()?;
            Some(Rgba::rgb(
                ((n >> 16) & 0xff) as u8,
                ((n >> 8) & 0xff) as u8,
                (n & 0xff) as u8,
            ))
        }
        8 => {
            let n = u32::from_str_radix(h, 16).ok()?;
            Some(Rgba {
                r: ((n >> 24) & 0xff) as u8,
                g: ((n >> 16) & 0xff) as u8,
                b: ((n >> 8) & 0xff) as u8,
                a: (n & 0xff) as u8,
            })
        }
        _ => None,
    }
}

fn parse_linear(v: &str) -> Option<(u8, Rgba, Rgba)> {
    let v = v.trim();
    let inner = v
        .strip_prefix("linear-gradient(")?
        .strip_suffix(')')?
        .trim();
    let mut parts = inner.splitn(3, ',');
    let angle = parts.next()?.trim();
    let a = parse_color(parts.next()?.trim())?;
    let b = parse_color(parts.next()?.trim())?;
    let mode = if angle.contains("135") { 2 } else { 1 };
    Some((mode, a, b))
}

fn parse_border(val: &str) -> Option<(f32, Rgba)> {
    let mut w = 1.0;
    let mut col = None;
    for part in val.split_whitespace() {
        if part == "solid" || part == "none" {
            if part == "none" {
                return None;
            }
            continue;
        }
        if let Some(px) = parse_px(part) {
            w = px;
        } else if let Some(c) = parse_color(part) {
            col = Some(c);
        }
    }
    Some((w, col.unwrap_or(Rgba::rgb(0, 0, 0))))
}

fn parse_px(v: &str) -> Option<f32> {
    let v = v.trim();
    if v == "0" {
        return Some(0.0);
    }
    v.strip_suffix("px")?.parse().ok()
}

fn parse_len(v: &str) -> Option<Len> {
    let v = v.trim();
    if v == "auto" {
        return Some(Len::Auto);
    }
    if let Some(p) = v.strip_suffix('%') {
        return Some(Len::Percent(p.parse::<f32>().ok()? / 100.0));
    }
    Some(Len::Px(parse_px(v)?))
}

fn apply_decl(c: &mut Computed, key: &str, val: &str) {
    match key {
        "display" => c.display_flex = val.trim() == "flex",
        "flex-direction" => c.column = val.trim() == "column",
        "width" => c.width = parse_len(val),
        "height" => c.height = parse_len(val),
        "min-width" => c.min_width = parse_len(val),
        "max-width" => c.max_width = parse_len(val),
        "position" => c.absolute = val.trim() == "absolute",
        "top" => c.top = parse_len(val),
        "left" => c.left = parse_len(val),
        "z-index" => c.z = val.trim().parse().unwrap_or(0),
        "white-space" => c.wrap = val.trim() == "normal",
        "flex-grow" => c.flex_grow = val.trim().parse().unwrap_or(0.0),
        "flex-shrink" => c.flex_shrink = val.trim().parse().unwrap_or(1.0),
        "align-items" => c.align_center = val.trim() == "center",
        "text-align" => c.text_align_center = val.trim() == "center",
        "justify-content" => {
            let v = val.trim();
            c.justify_center = v == "center";
            c.justify_between = v == "space-between";
        },
        "min-height" => c.min_height = parse_len(val),
        "overflow" => {
            let v = val.trim();
            c.overflow_hidden = v == "hidden" || v == "scroll" || v == "auto";
            c.overflow_scroll = v == "scroll" || v == "auto";
        }
        "background" | "background-color" => {
            if let Some((mode, a, b)) = parse_linear(val) {
                c.bg = Some(a);
                c.bg2 = Some(b);
                c.gradient = mode;
            } else {
                c.bg = parse_color(val);
                c.bg2 = None;
                c.gradient = 0;
            }
        }
        "color" => c.color = parse_color(val),
        "font-size" => c.font_size = parse_px(val),
        "font-weight" => c.font_weight = val.trim().parse().ok(),
        "font-family" => {
            c.font_family = val
                .split(',')
                .next()
                .map(|s| s.trim().trim_matches('"').to_string());
        }
        "border-radius" => c.radius = parse_px(val).unwrap_or(0.0),
        "gap" => c.gap = parse_px(val).unwrap_or(0.0),
        "padding" => {
            if let Some(p) = parse_px(val) {
                c.padding = [p, p, p, p];
            }
        }
        "padding-top" => c.padding[0] = parse_px(val).unwrap_or(0.0),
        "padding-right" => c.padding[1] = parse_px(val).unwrap_or(0.0),
        "padding-bottom" => c.padding[2] = parse_px(val).unwrap_or(0.0),
        "padding-left" => c.padding[3] = parse_px(val).unwrap_or(0.0),
        "margin" => {
            if let Some(p) = parse_px(val) {
                c.margin = [p, p, p, p];
            }
        }
        "margin-top" => c.margin[0] = parse_px(val).unwrap_or(0.0),
        "margin-right" => c.margin[1] = parse_px(val).unwrap_or(0.0),
        "margin-bottom" => c.margin[2] = parse_px(val).unwrap_or(0.0),
        "margin-left" => c.margin[3] = parse_px(val).unwrap_or(0.0),
        "border" => {
            let b = parse_border(val);
            c.border = [b, b, b, b];
        }
        "border-top" => c.border[0] = parse_border(val),
        "border-right" => c.border[1] = parse_border(val),
        "border-bottom" => c.border[2] = parse_border(val),
        "border-left" => c.border[3] = parse_border(val),
        "transform" => {
            if let Some(inner) = val
                .trim()
                .strip_prefix("translateY(")
                .and_then(|s| s.strip_suffix(')'))
            {
                c.translate_y = parse_px(inner).unwrap_or(0.0);
            }
        }
        _ => {}
    }
}

fn apply_inline(
    c: &mut Computed,
    style_attr: &str,
    vars: &std::collections::HashMap<String, String>,
) {
    for (k, v) in parse_decls(style_attr) {
        apply_decl(c, &k, &resolve(&v, vars));
    }
}

pub fn compute(
    el: &Elem,
    parent: Option<&Elem>,
    sheet: &Sheet,
    hover_uid: Option<u32>,
) -> Computed {
    let mut c = Computed {
        display_flex: true,
        flex_shrink: 1.0,
        ..Computed::default()
    };
    for rule in &sheet.rules {
        if rule_matches(rule, el, parent, hover_uid) {
            for (k, v) in &rule.decls {
                apply_decl(&mut c, k, &resolve(v, &sheet.vars));
            }
        }
    }
    if let Some(inline) = &el.style_attr {
        apply_inline(&mut c, inline, &sheet.vars);
    }
    c
}
