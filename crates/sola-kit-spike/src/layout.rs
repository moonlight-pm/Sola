//! Taffy flex layout from computed CSS.

use std::collections::HashMap;

use taffy::prelude::*;

use crate::css::{Computed, Len, Rgba, Sheet, compute};
use crate::dom::Elem;
use crate::paint::Fonts;

pub struct PaintItem {
    pub uid: u32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub bg: Option<Rgba>,
    pub bg2: Option<Rgba>,
    pub gradient: u8,
    pub border: [Option<(f32, Rgba)>; 4],
    pub radius: f32,
    pub text: Option<TextRun>,
    pub data_id: Option<String>,
    pub data_kind: Option<String>,
    pub data_surface: Option<String>,
    pub data_input: Option<String>,
    pub data_action: Option<String>,
    pub classes: Vec<String>,
    pub clip: Option<(f32, f32, f32, f32)>,
    pub overflow_scroll: bool,
    pub content_h: f32,
    pub color: Option<Rgba>,
    pub hidden: bool,
    pub pad: [f32; 4],
    pub text_align_center: bool,
    pub z: i32,
    pub wrap: bool,
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

struct TextCtx {
    text: String,
    size: f32,
    weight: u16,
    family: String,
    pad: [f32; 4],
    wrap: bool,
}

fn line_height(c: &Computed) -> f32 {
    c.font_size.unwrap_or(12.0) * 1.3
}

fn measure_text(
    known: Size<Option<f32>>,
    avail: Size<AvailableSpace>,
    ctx: Option<&mut TextCtx>,
    fonts: &mut Fonts,
) -> Size<f32> {
    let Some(ctx) = ctx else {
        return Size::ZERO;
    };
    let pad_x = ctx.pad[1] + ctx.pad[3];
    let pad_y = ctx.pad[0] + ctx.pad[2];
    let lh = ctx.size * 1.3;
    let inner = |w: f32| (w - pad_x).max(1.0);
    let avail_w = match known.width {
        Some(w) => inner(w),
        None => match avail.width {
            AvailableSpace::Definite(w) => inner(w),
            AvailableSpace::MaxContent | AvailableSpace::MinContent => 2000.0,
        },
    };
    let tw = fonts.measure_width(&ctx.text, ctx.size, ctx.weight, &ctx.family);
    if ctx.wrap {
        let lines = (tw / avail_w).ceil().max(1.0);
        Size {
            width: known.width.unwrap_or_else(|| match avail.width {
                AvailableSpace::Definite(w) => w,
                _ => (tw + pad_x).min(avail_w + pad_x),
            }),
            height: known.height.unwrap_or(lines * lh + pad_y),
        }
    } else {
        Size {
            width: known.width.unwrap_or(tw + pad_x),
            height: known.height.unwrap_or(lh + pad_y),
        }
    }
}

fn to_style(c: &Computed, el: &Elem, fonts: &mut Fonts) -> Style {
    let leaf_text = !el.text.is_empty() && el.children.is_empty();
    let fs = c.font_size.unwrap_or(12.0);
    let weight = c.font_weight.unwrap_or(400);
    let family = c.font_family.as_deref().unwrap_or("SF Pro Text");
    let mut height = dim(c.height);
    // Wrapping leaves leave height Auto so Taffy can measure against the
    // stretched width. Non-wrapping labels get a one-line box.
    if c.height.is_none() && leaf_text && !c.wrap {
        height = length(line_height(c) + c.padding[0] + c.padding[2]);
    }
    // Taffy BorderBox: width includes padding + border. Text is not a
    // flex child, so intrinsic size must be measured or the box is 0.
    let mut width = dim(c.width);
    let mut min_width = dim(c.min_width);
    if c.width.is_none() && c.flex_grow == 0.0 && leaf_text && !c.wrap {
        let tw = fonts.measure_width(&el.text, fs, weight, family);
        let bw = c.border[1].map(|b| b.0).unwrap_or(0.0) + c.border[3].map(|b| b.0).unwrap_or(0.0);
        let box_w = (tw + c.padding[1] + c.padding[3] + bw).max(1.0);
        width = length(box_w);
        min_width = length(box_w);
    }
    Style {
        display: Display::Flex,
        flex_direction: if c.column {
            FlexDirection::Column
        } else {
            FlexDirection::Row
        },
        size: Size { width, height },
        min_size: Size {
            width: if (c.overflow_hidden || c.overflow_scroll) && c.min_width.is_none() {
                length(0.0)
            } else {
                min_width
            },
            height: if (c.overflow_hidden || c.overflow_scroll) && c.min_height.is_none() {
                length(0.0)
            } else {
                dim(c.min_height)
            },
        },
        max_size: Size {
            width: dim(c.max_width),
            height: dim(c.max_height),
        },
        flex_grow: c.flex_grow,
        flex_shrink: c.flex_shrink,
        flex_basis: match c.flex_basis {
            Some(Len::Px(v)) => length(v),
            Some(Len::Percent(p)) => percent(p),
            Some(Len::Auto) | None => Dimension::Auto,
        },
        gap: Size {
            width: lp(c.gap),
            height: lp(c.gap),
        },
        justify_content: Some(if c.justify_between {
            JustifyContent::SpaceBetween
        } else if c.justify_center {
            JustifyContent::Center
        } else {
            JustifyContent::Start
        }),
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
        align_items: Some(if c.align_center {
            AlignItems::Center
        } else {
            AlignItems::Stretch
        }),
        position: if c.absolute {
            Position::Absolute
        } else {
            Position::Relative
        },
        inset: Rect {
            top: inset(c.top),
            right: LengthPercentageAuto::Auto,
            bottom: LengthPercentageAuto::Auto,
            left: inset(c.left),
        },
        ..Default::default()
    }
}

fn inset(len: Option<Len>) -> LengthPercentageAuto {
    match len {
        Some(Len::Px(v)) => LengthPercentageAuto::Length(v),
        Some(Len::Percent(p)) => LengthPercentageAuto::Percent(p),
        Some(Len::Auto) | None => LengthPercentageAuto::Auto,
    }
}

fn build<'a>(
    tree: &mut TaffyTree<TextCtx>,
    el: &'a Elem,
    ancestors: &mut Vec<&'a Elem>,
    sheet: &Sheet,
    hover: Option<u32>,
    fonts: &mut Fonts,
) -> Built {
    let computed = compute(el, ancestors, sheet, hover);
    ancestors.push(el);
    let kids: Vec<Built> = el
        .children
        .iter()
        .filter(|c| !c.has_class("is-hidden"))
        .map(|c| build(tree, c, ancestors, sheet, hover, fonts))
        .collect();
    ancestors.pop();
    let child_ids: Vec<NodeId> = kids.iter().map(|k| k.id).collect();
    let style = to_style(&computed, el, fonts);
    let leaf_text = !el.text.is_empty() && el.children.is_empty();
    let id = if child_ids.is_empty() {
        if leaf_text && computed.wrap {
            tree.new_leaf_with_context(
                style,
                TextCtx {
                    text: el.text.clone(),
                    size: computed.font_size.unwrap_or(12.0),
                    weight: computed.font_weight.unwrap_or(400),
                    family: computed
                        .font_family
                        .clone()
                        .unwrap_or_else(|| "SF Pro Text".into()),
                    pad: computed.padding,
                    wrap: true,
                },
            )
            .unwrap()
        } else {
            tree.new_leaf(style).unwrap()
        }
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

pub fn intersect_clip(
    a: Option<(f32, f32, f32, f32)>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> Option<(f32, f32, f32, f32)> {
    intersect(a, x, y, w, h)
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
    tree: &TaffyTree<TextCtx>,
    built: &Built,
    ox: f32,
    oy: f32,
    clip: Option<(f32, f32, f32, f32)>,
    parent_z: i32,
    scrolls: &std::collections::HashMap<String, f32>,
    out: &mut Vec<PaintItem>,
) {
    if built.el.has_class("is-hidden") {
        return;
    }
    let layout = tree.layout(built.id).unwrap();
    let x = ox + layout.location.x;
    let y = oy + layout.location.y + built.computed.translate_y;
    let w = layout.size.width;
    let h = layout.size.height;
    let z = built.computed.z.max(parent_z);
    let clip = if z > 0 {
        None
    } else if built.computed.overflow_hidden {
        intersect(clip, x, y, w, h)
    } else {
        clip
    };
    let mut content_h = h;
    for kid in &built.kids {
        let kl = tree.layout(kid.id).unwrap();
        content_h = content_h.max(kl.location.y + kl.size.height + layout.padding.bottom);
    }
    let scroll = built
        .el
        .data_id
        .as_deref()
        .and_then(|id| scrolls.get(id).copied())
        .unwrap_or(0.0);
    let child_oy = if built.computed.overflow_scroll {
        y - scroll
    } else {
        y
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
        bg2: built.computed.bg2,
        gradient: built.computed.gradient,
        border: built.computed.border,
        radius: built.computed.radius,
        text,
        data_id: built.el.data_id.clone(),
        data_kind: built.el.data_kind.clone(),
        data_surface: built.el.data_surface.clone(),
        data_input: built.el.data_input.clone(),
        data_action: built.el.data_action.clone(),
        classes: built.el.classes.clone(),
        clip,
        overflow_scroll: built.computed.overflow_scroll,
        content_h,
        color: built.computed.color,
        hidden: built.el.has_class("is-origin"),
        text_align_center: built.computed.text_align_center,
        z,
        wrap: built.computed.wrap,
        pad: [
            layout.padding.top,
            layout.padding.right,
            layout.padding.bottom,
            layout.padding.left,
        ],
    });
    for kid in &built.kids {
        collect(tree, kid, x, child_oy, clip, z, scrolls, out);
    }
}

pub fn layout_tree(
    root: &Elem,
    sheet: &Sheet,
    hover: Option<u32>,
    vw: f32,
    vh: f32,
    fonts: &mut Fonts,
    scrolls: &std::collections::HashMap<String, f32>,
) -> Vec<PaintItem> {
    let mut tree: TaffyTree<TextCtx> = TaffyTree::new();
    let mut ancestors = Vec::new();
    let built = build(&mut tree, root, &mut ancestors, sheet, hover, fonts);
    let mut root_style = tree.style(built.id).unwrap().clone();
    root_style.size = Size {
        width: length(vw),
        height: length(vh),
    };
    let _ = tree.set_style(built.id, root_style);
    let _ = tree.compute_layout_with_measure(
        built.id,
        Size {
            width: AvailableSpace::Definite(vw),
            height: AvailableSpace::Definite(vh),
        },
        |known, avail, _, ctx, _style| measure_text(known, avail, ctx, fonts),
    );
    let mut items = Vec::new();
    collect(&tree, &built, 0.0, 0.0, None, 0, scrolls, &mut items);
    items.sort_by_key(|i| i.z);
    items
}

pub fn point_in_item(i: &PaintItem, x: f32, y: f32) -> bool {
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
        .find(|i| point_in_item(i, x, y) && i.data_action.is_some())
        .or_else(|| {
            items
                .iter()
                .rev()
                .find(|i| point_in_item(i, x, y) && i.data_id.is_some())
        })
}

pub fn hover_at(items: &[PaintItem], x: f32, y: f32) -> Option<u32> {
    hit_test(items, x, y).map(|i| i.uid).or_else(|| {
        items
            .iter()
            .rev()
            .find(|i| point_in_item(i, x, y))
            .map(|i| i.uid)
    })
}

/// Graphite hover / etch fills. Applied after layout so a pointer move can
/// restyle without rebuilding the tree or rerastering glyphs.
const HOVER_FILL: Rgba = Rgba::rgb(0x1e, 0x25, 0x33);
const ETCH_LIP: Rgba = Rgba::rgb(0x20, 0x25, 0x2f);
const ETCH_WELL: Rgba = Rgba::rgb(0x0e, 0x12, 0x1b);

fn has_class(item: &PaintItem, class: &str) -> bool {
    item.classes.iter().any(|c| c == class)
}

/// Patch list-etch / log / toolbar hover onto already-laid-out items.
pub fn apply_pointer_hover(items: &mut [PaintItem], hover: Option<u32>) {
    let hit = hover.and_then(|uid| {
        items
            .iter()
            .find(|i| i.uid == uid)
            .map(|i| (i.x + 1.0, i.y + 1.0, i.uid))
    });
    let hovered_row = hit.and_then(|(x, y, uid)| {
        items
            .iter()
            .rev()
            .find(|i| (has_class(i, "log-row") || has_class(i, "row")) && point_in_item(i, x, y))
            .map(|i| i.uid)
            .or(Some(uid))
    });
    let rows: Vec<(u32, f32, f32, f32, f32, bool, bool, bool)> = items
        .iter()
        .filter(|i| has_class(i, "log-row") || has_class(i, "row"))
        .map(|i| {
            (
                i.uid,
                i.x,
                i.y,
                i.w,
                i.h,
                has_class(i, "is-active"),
                has_class(i, "is-header"),
                has_class(i, "log-row"),
            )
        })
        .collect();
    for item in items.iter_mut() {
        if has_class(item, "log-row") {
            let on = hovered_row == Some(item.uid);
            let active = has_class(item, "is-active");
            item.bg = if active || on { Some(HOVER_FILL) } else { None };
            continue;
        }
        if has_class(item, "toolbar-btn") || has_class(item, "menu-item") {
            let on = hover == Some(item.uid) || hovered_row == Some(item.uid);
            item.bg = if on { Some(HOVER_FILL) } else { None };
            continue;
        }
        if has_class(item, "row") {
            if has_class(item, "is-header") {
                item.bg = None;
            } else if has_class(item, "is-active") {
                item.bg = Some(ETCH_LIP);
            } else {
                item.bg = None;
            }
            continue;
        }
        if has_class(item, "etch") {
            let cx = item.x + item.w * 0.5;
            let cy = item.y + item.h * 0.5;
            let parent = rows
                .iter()
                .rev()
                .find(|r| cx >= r.1 && cy >= r.2 && cx < r.1 + r.3 && cy < r.2 + r.4);
            let Some(p) = parent else {
                continue;
            };
            item.bg = if p.6 {
                None
            } else if p.5 {
                Some(ETCH_WELL)
            } else if hovered_row == Some(p.0) {
                Some(HOVER_FILL)
            } else {
                None
            };
            continue;
        }
        if has_class(item, "sb-thumb") {
            let on = hover == Some(item.uid);
            item.bg = Some(if on {
                Rgba::rgb(0xa1, 0xad, 0xc7)
            } else {
                SCROLL_THUMB
            });
        }
    }
}

const SCROLL_THUMB: Rgba = Rgba {
    r: 0x8b,
    g: 0x94,
    b: 0xa8,
    a: 0xb0,
};
const SCROLL_TRACK: Rgba = Rgba {
    r: 0x00,
    g: 0x00,
    b: 0x00,
    a: 0x00,
};

/// Overlay scrollbar width (CSS px). Wide enough to grab; inset from the
/// log/rail splitter so the resize hit band does not steal the thumb.
pub const SCROLLBAR_W: f32 = 12.0;
const SCROLLBAR_PAD: f32 = 6.0;
const THUMB_MIN: f32 = 32.0;

/// Overlay thumb for a scroll pane. `None` when content fits.
pub fn scrollbar_thumb(pane_h: f32, content_h: f32, scroll: f32) -> Option<(f32, f32, f32)> {
    if content_h <= pane_h + 1.0 {
        return None;
    }
    let track_h = (pane_h - SCROLLBAR_PAD * 2.0).max(THUMB_MIN);
    let thumb_h = ((pane_h / content_h) * track_h).clamp(THUMB_MIN, track_h);
    let max = (content_h - pane_h).max(0.0);
    let t = if max < 0.5 {
        0.0
    } else {
        (scroll / max).clamp(0.0, 1.0)
    };
    let thumb_y = SCROLLBAR_PAD + t * (track_h - thumb_h);
    Some((thumb_y, thumb_h, max))
}

fn overlay_fill(
    uid: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    bg: Rgba,
    radius: f32,
    z: i32,
    action: &str,
    id: &str,
    class: &str,
) -> PaintItem {
    PaintItem {
        uid,
        x,
        y,
        w,
        h,
        bg: Some(bg),
        bg2: None,
        gradient: 0,
        border: [None, None, None, None],
        radius,
        text: None,
        data_id: Some(id.into()),
        data_kind: None,
        data_surface: None,
        data_input: None,
        data_action: Some(action.into()),
        classes: vec![class.into()],
        clip: None,
        overflow_scroll: false,
        content_h: h,
        color: None,
        hidden: false,
        pad: [0.0; 4],
        text_align_center: false,
        z,
        wrap: false,
    }
}

/// Overlay track + thumb on every overflowing scroll pane. Hit-test wins
/// because these are appended last (higher z).
pub fn append_scrollbars(items: &mut Vec<PaintItem>, scrolls: &HashMap<String, f32>) {
    let panes: Vec<(String, f32, f32, f32, f32, f32)> = items
        .iter()
        .filter(|i| i.overflow_scroll && i.data_id.is_some())
        .filter(|i| i.content_h > i.h + 1.0)
        .map(|i| (i.data_id.clone().unwrap(), i.x, i.y, i.w, i.h, i.content_h))
        .collect();
    let mut uid = 800_000u32;
    for (id, x, y, w, h, content_h) in panes {
        let scroll = scrolls.get(&id).copied().unwrap_or(0.0);
        let Some((thumb_y, thumb_h, _)) = scrollbar_thumb(h, content_h, scroll) else {
            continue;
        };
        let track_x = x + w - SCROLLBAR_W - SCROLLBAR_PAD;
        uid += 1;
        items.push(overlay_fill(
            uid,
            track_x,
            y,
            SCROLLBAR_W,
            h,
            SCROLL_TRACK,
            0.0,
            20,
            "scroll-track",
            &id,
            "sb-track",
        ));
        uid += 1;
        items.push(overlay_fill(
            uid,
            track_x,
            y + thumb_y,
            SCROLLBAR_W,
            thumb_h,
            SCROLL_THUMB,
            3.5,
            21,
            "scroll-thumb",
            &id,
            "sb-thumb",
        ));
    }
}

#[cfg(test)]
mod scrollbar_tests {
    use super::*;

    #[test]
    fn thumb_hides_when_content_fits() {
        assert!(scrollbar_thumb(200.0, 180.0, 0.0).is_none());
    }

    #[test]
    fn thumb_moves_with_scroll() {
        let (y0, h0, max) = scrollbar_thumb(200.0, 1000.0, 0.0).unwrap();
        let (y1, h1, _) = scrollbar_thumb(200.0, 1000.0, max).unwrap();
        assert!((h0 - h1).abs() < 0.01);
        assert!(y1 > y0 + 50.0);
        assert!(max > 700.0);
    }
}
