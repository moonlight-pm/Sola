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
        if let Some(slot) = find_slot_mut(&mut root, "demo") {
            slot.children.push(renumber(fragment, &mut next));
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
        if let Some(id) = el.data_id.as_deref() {
            if toggles.get(id).copied().unwrap_or(false) {
                if el.classes.iter().any(|c| c == "toggle") {
                    add_class(el, "toggle-on");
                } else {
                    add_class(el, "check-on");
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
    if let Some(el) = find_id_mut(root, id) {
        add_class(el, "is-focused");
    }
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
    if el.data_slot.as_deref() == Some(name) {
        return Some(el);
    }
    for c in &mut el.children {
        if let Some(hit) = find_slot_mut(c, name) {
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

fn add_class(el: &mut Elem, class: &str) {
    if !el.classes.iter().any(|c| c == class) {
        el.classes.push(class.to_string());
    }
}

fn clone_fresh(el: &Elem, next: &mut u32) -> Elem {
    renumber(el.clone(), next)
}

fn renumber(mut el: Elem, next: &mut u32) -> Elem {
    el.uid = *next;
    *next += 1;
    el.children = el
        .children
        .into_iter()
        .map(|c| renumber(c, next))
        .collect();
    el
}

fn max_uid(el: &Elem) -> u32 {
    el.children
        .iter()
        .map(max_uid)
        .max()
        .unwrap_or(el.uid)
        .max(el.uid)
}
