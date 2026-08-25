//! Expand `assets/sidebar.html`: clone `data-template="row"` into `data-slot="rows"`.

use crate::dom::{Elem, parse_html};

pub struct RowSpec {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub classes: Vec<String>,
    pub translate_y: Option<i32>,
}

pub fn expand(
    html: &str,
    rows: &[RowSpec],
    title: &str,
    query: &str,
    focused: bool,
    dragging: bool,
) -> Elem {
    let mut root = parse_html(html);
    let tpl = take_template(&mut root, "row");
    if let Some(tpl) = tpl {
        let mut next = max_uid(&root) + 1;
        if let Some(slot) = find_slot_mut(&mut root, "rows") {
            for spec in rows {
                let mut row = clone_fresh(&tpl, &mut next);
                row.data_template = None;
                row.data_id = Some(spec.id.clone());
                row.data_kind = Some(spec.kind.clone());
                for c in &spec.classes {
                    add_class(&mut row, c);
                }
                if let Some(dy) = spec.translate_y {
                    if dy != 0 {
                        row.style_attr = Some(format!("transform: translateY({dy}px)"));
                    }
                }
                apply_bind(&mut row, "label", &spec.label);
                slot.children.push(row);
            }
        } else {
            tracing::warn!("HTML has data-template=row but no data-slot=rows");
        }
    } else {
        tracing::warn!("HTML missing data-template=row");
    }
    apply_bind(&mut root, "title", title);
    apply_bind(&mut root, "query", query);
    if focused {
        if let Some(el) = find_data_input_mut(&mut root) {
            add_class(el, "is-focused");
        }
    }
    if dragging {
        if let Some(el) = find_slot_mut(&mut root, "strip") {
            add_class(el, "is-dragging");
        } else if let Some(el) = find_class_mut(&mut root, "strip") {
            add_class(el, "is-dragging");
        }
    }
    root
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

fn find_class_mut<'a>(el: &'a mut Elem, name: &str) -> Option<&'a mut Elem> {
    if el.classes.iter().any(|c| c == name) {
        return Some(el);
    }
    for c in &mut el.children {
        if let Some(hit) = find_class_mut(c, name) {
            return Some(hit);
        }
    }
    None
}

fn find_data_input_mut(el: &mut Elem) -> Option<&mut Elem> {
    if el.data_input.is_some() {
        return Some(el);
    }
    for c in &mut el.children {
        if let Some(hit) = find_data_input_mut(c) {
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
    let uid = *next;
    *next += 1;
    Elem {
        uid,
        tag: el.tag.clone(),
        classes: el.classes.clone(),
        data_id: el.data_id.clone(),
        data_kind: el.data_kind.clone(),
        data_surface: el.data_surface.clone(),
        data_input: el.data_input.clone(),
        data_template: el.data_template.clone(),
        data_slot: el.data_slot.clone(),
        data_bind: el.data_bind.clone(),
        style_attr: el.style_attr.clone(),
        text: el.text.clone(),
        children: el.children.iter().map(|c| clone_fresh(c, next)).collect(),
    }
}

fn max_uid(el: &Elem) -> u32 {
    el.children.iter().map(max_uid).max().unwrap_or(el.uid).max(el.uid)
}
