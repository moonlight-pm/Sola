//! Taffy flex layout from computed CSS.

use taffy::prelude::*;

use crate::css::{Computed, Len, Rgba, Sheet, compute};
use crate::dom::Elem;

pub struct PaintItem {
    pub uid: u32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub bg: Option<Rgba>,
    pub border: Option<(f32, Rgba)>,
    pub radius: f32,
    pub text: Option<TextRun>,
    pub data_id: Option<String>,
    pub data_kind: Option<String>,
    pub data_surface: Option<String>,
    pub data_input: Option<String>,
    pub classes: Vec<String>,
    pub clip: Option<(f32, f32, f32, f32)>,
    pub overflow_scroll: bool,
    pub hidden: bool,
    pub pad: [f32; 4],
}

pub struct TextRun {
    pub text: String,
    pub color: Rgba,
    pub size: f32,
    pub weight: u16,
    pub family: String,
}

struct Built {
    id: NodeId,
    computed: Computed,
    el: Elem,
    kids: Vec<Built>,
}

fn dim(len: Option<Len>) -> Dimension {
    match len {
        Some(Len::Px(v)) => length(v),
        Some(Len::Percent(p)) => percent(p),
        Some(Len::Auto) | None => Dimension::Auto,
    }
}

fn lp(v: f32) -> LengthPercentage {
    length(v)
}

fn lpa(v: f32) -> LengthPercentageAuto {
    LengthPercentageAuto::Length(v)
}

fn to_style(c: &Computed) -> Style {
    Style {
        display: Display::Flex,
        flex_direction: if c.column {
            FlexDirection::Column
        } else {
            FlexDirection::Row
        },
        size: Size {
            width: dim(c.width),
            height: dim(c.height),
        },
        min_size: Size {
            width: dim(c.min_width),
            height: dim(c.min_height),
        },
        flex_grow: c.flex_grow,
        flex_shrink: c.flex_shrink,
        gap: Size {
            width: lp(c.gap),
            height: lp(c.gap),
        },
        padding: Rect {
            top: lp(c.padding[0]),
            right: lp(c.padding[1]),
            bottom: lp(c.padding[2]),
            left: lp(c.padding[3]),
        },
        margin: Rect {
            top: lpa(c.margin[0]),
            right: lpa(c.margin[1]),
            bottom: lpa(c.margin[2]),
            left: lpa(c.margin[3]),
        },
        align_items: Some(AlignItems::Stretch),
        ..Default::default()
    }
}

fn build(
    tree: &mut TaffyTree<()>,
    el: &Elem,
    parent: Option<&Elem>,
    sheet: &Sheet,
    hover: Option<u32>,
) -> Built {
    let computed = compute(el, parent, sheet, hover);
    let kids: Vec<Built> = el
        .children
        .iter()
        .map(|c| build(tree, c, Some(el), sheet, hover))
        .collect();
    let child_ids: Vec<NodeId> = kids.iter().map(|k| k.id).collect();
    let style = to_style(&computed);
    let id = if child_ids.is_empty() {
        tree.new_leaf(style).unwrap()
    } else {
        tree.new_with_children(style, &child_ids).unwrap()
    };
    Built {
        id,
        computed,
        el: el.clone(),
        kids,
    }
}

fn intersect(
    a: Option<(f32, f32, f32, f32)>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> Option<(f32, f32, f32, f32)> {
    let (cx, cy, cw, ch) = match a {
        Some(c) => c,
        None => return Some((x, y, w, h)),
    };
    let x0 = x.max(cx);
    let y0 = y.max(cy);
    let x1 = (x + w).min(cx + cw);
    let y1 = (y + h).min(cy + ch);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some((x0, y0, x1 - x0, y1 - y0))
}

fn collect(
    tree: &TaffyTree<()>,
    built: &Built,
    ox: f32,
    oy: f32,
    clip: Option<(f32, f32, f32, f32)>,
    out: &mut Vec<PaintItem>,
) {
    let layout = tree.layout(built.id).unwrap();
    let x = ox + layout.location.x;
    let y = oy + layout.location.y + built.computed.translate_y;
    let w = layout.size.width;
    let h = layout.size.height;
    let clip = if built.computed.overflow_hidden {
        intersect(clip, x, y, w, h)
    } else {
        clip
    };
    let text = if !built.el.text.is_empty() {
        Some(TextRun {
            text: built.el.text.clone(),
            color: built.computed.color.unwrap_or(Rgba::rgb(0xe9, 0xec, 0xf2)),
            size: built.computed.font_size.unwrap_or(12.0),
            weight: built.computed.font_weight.unwrap_or(400),
            family: built
                .computed
                .font_family
                .clone()
                .unwrap_or_else(|| "SF Pro Text".into()),
        })
    } else {
        None
    };
    out.push(PaintItem {
        uid: built.el.uid,
        x,
        y,
        w,
        h,
        bg: built.computed.bg,
        border: built.computed.border,
        radius: built.computed.radius,
        text,
        data_id: built.el.data_id.clone(),
        data_kind: built.el.data_kind.clone(),
        data_surface: built.el.data_surface.clone(),
        data_input: built.el.data_input.clone(),
        classes: built.el.classes.clone(),
        clip,
        overflow_scroll: built.computed.overflow_scroll,
        hidden: built.el.has_class("is-origin"),
        pad: [
            layout.padding.top,
            layout.padding.right,
            layout.padding.bottom,
            layout.padding.left,
        ],
    });
    for kid in &built.kids {
        collect(tree, kid, x, y, clip, out);
    }
}

pub fn layout_tree(
    root: &Elem,
    sheet: &Sheet,
    hover: Option<u32>,
    vw: f32,
    vh: f32,
) -> Vec<PaintItem> {
    let mut tree: TaffyTree<()> = TaffyTree::new();
    let built = build(&mut tree, root, None, sheet, hover);
    let mut root_style = tree.style(built.id).unwrap().clone();
    root_style.size = Size {
        width: length(vw),
        height: length(vh),
    };
    let _ = tree.set_style(built.id, root_style);
    let _ = tree.compute_layout(
        built.id,
        Size {
            width: AvailableSpace::Definite(vw),
            height: AvailableSpace::Definite(vh),
        },
    );
    let mut items = Vec::new();
    collect(&tree, &built, 0.0, 0.0, None, &mut items);
    items
}

fn point_in_item(i: &PaintItem, x: f32, y: f32) -> bool {
    if x < i.x || x >= i.x + i.w || y < i.y || y >= i.y + i.h {
        return false;
    }
    if let Some((cx, cy, cw, ch)) = i.clip {
        if x < cx || x >= cx + cw || y < cy || y >= cy + ch {
            return false;
        }
    }
    true
}

pub fn hit_test(items: &[PaintItem], x: f32, y: f32) -> Option<&PaintItem> {
    items
        .iter()
        .rev()
        .find(|i| i.data_id.is_some() && point_in_item(i, x, y))
}

pub fn hover_at(items: &[PaintItem], x: f32, y: f32) -> Option<u32> {
    hit_test(items, x, y)
        .map(|i| i.uid)
        .or_else(|| {
            items
                .iter()
                .rev()
                .find(|i| point_in_item(i, x, y))
                .map(|i| i.uid)
        })
}
