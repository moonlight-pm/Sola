//! Expand `assets/kit.html`: nav rows + per-page demo into slots.

use crate::dom::{Elem, parse_html};

pub struct RowSpec {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub classes: Vec<String>,
}

pub fn expand(
    html: &str,
    rows: &[RowSpec],
    title: &str,
    heading: &str,
    lede: &str,
    demo: &str,
    theme: &str,
    theme_open: bool,
) -> Elem {
    let mut root = parse_html(html);
    if !rows.is_empty() {
        let tpl = take_template(&mut root, "row");
        if let Some(tpl) = tpl {
            let mut next = max_uid(&root) + 1;
            if let Some(slot) = find_slot_mut(&mut root, "nav") {
                for spec in rows {
                    let mut row = clone_fresh(&tpl, &mut next);
                    row.data_template = None;
                    if spec.kind == "item" {
                        row.data_id = Some(spec.id.clone());
                    } else {
                        row.data_id = None;
                    }
                    row.data_kind = Some(spec.kind.clone());
                    for c in &spec.classes {
                        add_class(&mut row, c);
                    }
                    apply_bind(&mut row, "label", &spec.label);
                    slot.children.push(row);
                }
            } else {
                tracing::warn!("HTML missing data-slot=nav");
            }
        } else {
            tracing::warn!("HTML missing data-template=row");
        }
    }
    apply_bind(&mut root, "title", title);
    apply_bind(&mut root, "heading", heading);
    apply_bind(&mut root, "lede", lede);
    apply_bind(&mut root, "theme", theme);
    if let Some(menu) = find_slot_mut(&mut root, "theme-menu") {
        if !theme_open {
            add_class(menu, "is-hidden");
        } else {
            for row in &mut menu.children {
                if row.data_id.as_deref() == Some(theme) {
                    add_class(row, "is-active");
                }
            }
        }
    }
    if !demo.is_empty() {
        let fragment = parse_html(demo);
        let mut next = max_uid(&root) + 1;
        if let Some(slot) = find_slot_any_mut(&mut root, &["demo", "panel"]) {
            slot.children.push(renumber(fragment, &mut next));
        } else {
            tracing::warn!("HTML missing data-slot=demo");
        }
    }
    root
}

pub fn apply_split(root: &mut Elem, frac: f32) {
    if let Some(slot) = find_slot_mut(root, "split-left") {
        let pct = (frac.clamp(0.2, 0.8) * 100.0).round();
        slot.style_attr = Some(format!("width:{pct}%;flex-grow:0"));
    }
}

pub fn apply_split_h(root: &mut Elem, frac: f32) {
    if let Some(slot) = find_slot_mut(root, "split-top") {
        let pct = (frac.clamp(0.2, 0.8) * 100.0).round();
        slot.style_attr = Some(format!("height:{pct}%;flex-grow:0"));
    }
}

pub fn apply_fields(root: &mut Elem, fields: &std::collections::HashMap<String, String>) {
    for (k, v) in fields {
        apply_bind(root, k, v);
    }
}

pub fn apply_toggles(root: &mut Elem, toggles: &std::collections::HashMap<String, bool>) {
    walk_toggles(root, toggles);
}

fn walk_toggles(el: &mut Elem, toggles: &std::collections::HashMap<String, bool>) {
    if el.data_action.as_deref() == Some("toggle") {
        let id = el.data_id.clone();
        if let Some(id) = id {
            let on = toggles.get(&id).copied().unwrap_or(false);
            if on {
                if el.classes.iter().any(|c| c == "toggle") {
                    add_class(el, "toggle-on");
                } else {
                    add_class(el, "check-on");
                }
            } else {
                for c in &mut el.children {
                    if c.data_kind.as_deref() == Some("icon") {
                        add_class(c, "is-hidden");
                    }
                }
            }
        }
    }
    for c in &mut el.children {
        walk_toggles(c, toggles);
    }
}

pub fn hide_slot(root: &mut Elem, name: &str, hide: bool) {
    if hide {
        if let Some(slot) = find_slot_mut(root, name) {
            add_class(slot, "is-hidden");
        }
    }
}

pub fn apply_focus(root: &mut Elem, id: Option<&str>) {
    let Some(id) = id else {
        return;
    };
    walk_mut(root, &mut |el| {
        if el.data_id.as_deref() == Some(id) {
            add_class(el, "is-focused");
        }
    });
}

fn find_id_mut<'a>(el: &'a mut Elem, id: &str) -> Option<&'a mut Elem> {
    if el.data_id.as_deref() == Some(id) {
        return Some(el);
    }
    for c in &mut el.children {
        if let Some(hit) = find_id_mut(c, id) {
            return Some(hit);
        }
    }
    None
}

fn take_template(el: &mut Elem, name: &str) -> Option<Elem> {
    if let Some(i) = el
        .children
        .iter()
        .position(|c| c.data_template.as_deref() == Some(name))
    {
        return Some(el.children.remove(i));
    }
    for c in &mut el.children {
        if let Some(t) = take_template(c, name) {
            return Some(t);
        }
    }
    None
}

fn find_slot_mut<'a>(el: &'a mut Elem, name: &str) -> Option<&'a mut Elem> {
    find_slot_any_mut(el, &[name])
}

fn find_slot_any_mut<'a>(el: &'a mut Elem, names: &[&str]) -> Option<&'a mut Elem> {
    if el.data_slot.as_deref().is_some_and(|s| names.contains(&s)) {
        return Some(el);
    }
    for c in &mut el.children {
        if let Some(hit) = find_slot_any_mut(c, names) {
            return Some(hit);
        }
    }
    None
}

fn apply_bind(el: &mut Elem, key: &str, value: &str) {
    if el.data_bind.as_deref() == Some(key) {
        el.text = value.to_string();
    }
    for c in &mut el.children {
        apply_bind(c, key, value);
    }
}

pub fn add_class(el: &mut Elem, class: &str) {
    if !el.classes.iter().any(|c| c == class) {
        el.classes.push(class.to_string());
    }
}

pub fn remove_class(el: &mut Elem, class: &str) {
    el.classes.retain(|c| c != class);
}

pub fn node(
    next: &mut u32,
    classes: &[&str],
    action: Option<&str>,
    id: Option<&str>,
    text: &str,
) -> Elem {
    let uid = *next;
    *next += 1;
    Elem {
        uid,
        tag: "div".into(),
        classes: classes.iter().map(|s| (*s).to_string()).collect(),
        data_id: id.map(|s| s.to_string()),
        data_kind: None,
        data_surface: None,
        data_input: None,
        data_template: None,
        data_slot: None,
        data_bind: None,
        data_action: action.map(|s| s.to_string()),
        style_attr: None,
        text: text.to_string(),
        children: Vec::new(),
    }
}

pub fn fill_slot(root: &mut Elem, name: &str, kids: Vec<Elem>) {
    if let Some(slot) = find_slot_mut(root, name) {
        slot.children = kids;
    }
}

pub fn next_uid(root: &Elem) -> u32 {
    max_uid(root) + 1
}

/// Replace the element that carries `data-slot=name` with `replacement`.
/// Kit components (sidebar, …) own their root markup; apps only leave a slot.
pub fn replace_slot(root: &mut Elem, name: &str, replacement: Elem) -> bool {
    if root.data_slot.as_deref() == Some(name) {
        *root = replacement;
        return true;
    }
    fn walk(el: &mut Elem, name: &str, replacement: &mut Option<Elem>) -> bool {
        if let Some(i) = el
            .children
            .iter()
            .position(|c| c.data_slot.as_deref() == Some(name))
        {
            if let Some(rep) = replacement.take() {
                el.children[i] = rep;
                return true;
            }
            return false;
        }
        for c in &mut el.children {
            if walk(c, name, replacement) {
                return true;
            }
        }
        false
    }
    walk(root, name, &mut Some(replacement))
}

pub fn tagged(
    next: &mut u32,
    tag: &str,
    classes: &[&str],
    action: Option<&str>,
    id: Option<&str>,
    text: &str,
) -> Elem {
    let mut el = node(next, classes, action, id, text);
    el.tag = tag.into();
    el
}

pub fn set_style(root: &mut Elem, id: &str, style: &str) {
    if let Some(el) = find_id_mut(root, id) {
        el.style_attr = Some(style.to_string());
    }
}

pub fn walk_mut(el: &mut Elem, f: &mut impl FnMut(&mut Elem)) {
    f(el);
    for c in &mut el.children {
        walk_mut(c, f);
    }
}

pub fn apply_active_id(root: &mut Elem, id: &str) {
    walk_mut(root, &mut |el| {
        if el.data_id.as_deref() == Some(id) {
            add_class(el, "is-active");
        }
    });
}

pub fn apply_enamel(root: &mut Elem) {
    walk_mut(root, &mut |el| {
        if el.classes.iter().any(|c| c == "enamel") {
            let seed = el.data_id.clone().unwrap_or_else(|| "seed-default".into());
            el.style_attr = Some(crate::palette::enamel_style(&seed));
        }
    });
}

pub fn hide_if(root: &mut Elem, pred: impl Fn(&Elem) -> bool) {
    walk_mut(root, &mut |el| {
        if pred(el) {
            add_class(el, "is-hidden");
        }
    });
}

pub fn apply_placeholder(root: &mut Elem, id: &str, empty: bool, placeholder: &str) {
    if let Some(el) = find_id_mut(root, id) {
        if empty {
            el.text = placeholder.to_string();
            add_class(el, "is-placeholder");
        }
    }
}

fn clone_fresh(el: &Elem, next: &mut u32) -> Elem {
    renumber(el.clone(), next)
}

fn renumber(mut el: Elem, next: &mut u32) -> Elem {
    el.uid = *next;
    *next += 1;
    el.children = el.children.into_iter().map(|c| renumber(c, next)).collect();
    el
}

pub fn fill_atoms(root: &mut Elem, tiles: &[(String, String, String)]) {
    if tiles.is_empty() {
        hide_slot(root, "atoms", true);
        return;
    }
    let mut next = max_uid(root) + 1;
    let mut kids = Vec::new();
    kids.push(node(&mut next, &["t-sub"], None, None, "This page's atoms"));
    kids.push(node(
        &mut next,
        &["t-caption", "wrap"],
        None,
        None,
        "Click a swatch after New Theme. Save lives in the header.",
    ));
    let mut row = node(&mut next, &["seed-row"], None, None, "");
    for (i, (id, name, hex)) in tiles.iter().enumerate() {
        if i > 0 && i % 5 == 0 {
            kids.push(std::mem::replace(
                &mut row,
                node(&mut next, &["seed-row"], None, None, ""),
            ));
        }
        let mut atom = node(&mut next, &["atom"], None, None, "");
        let mut sw = node(&mut next, &["swatch-lg"], Some("edit-atom"), Some(id), "");
        sw.style_attr = Some(format!("background:{hex}"));
        atom.children.push(sw);
        atom.children
            .push(node(&mut next, &["atom-name"], None, None, name));
        atom.children
            .push(node(&mut next, &["atom-hex"], None, None, hex));
        row.children.push(atom);
    }
    if !row.children.is_empty() {
        kids.push(row);
    }
    fill_slot(root, "atoms", kids);
}

fn max_uid(el: &Elem) -> u32 {
    el.children
        .iter()
        .map(max_uid)
        .max()
        .unwrap_or(el.uid)
        .max(el.uid)
}
