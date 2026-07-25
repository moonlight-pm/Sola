//! Vertical sidebar — column of selectable items grouped into
//! optionally-labeled sections.
//!
//! Pattern: caller builds a `Vec<SidebarSection<_>>` (each holding its
//! own `Vec<SidebarItem<_>>`) from its own state and hands it to
//! `sidebar(sections)`. The component is parent-controlled (no internal
//! selection state) so the consumer's update fn stays the single source
//! of truth for which item is active.
//!
//! For richer panels — collapse/expand, drag-to-resize, drag-reorder,
//! per-item shortcut hints / close buttons / secondary labels — use the
//! opt-in [`SidebarPanel`] builder. It is strictly additive: `sidebar()`
//! and the `SidebarItem`/`SidebarSection` constructors keep their exact
//! prior behaviour, so existing consumers compile and render unchanged.
//!
//! Style fns read from `theme.extended_palette()` only — the kit's
//! atom→slot bindings live in [`crate::theme::build_theme`]. To
//! restyle the sidebar globally, edit that mapping; this file should
//! never see a raw `hex::*`.

use std::collections::HashMap;

use iced::widget::text::Wrapping;
use iced::widget::{
    Container, Space, button, column, container, float, mouse_area, row, stack, text,
};
use iced::{
    Animation, Background, Border, Color, Element, Length, Padding, Shadow, Theme, Vector,
    animation::Easing, mouse, time::Instant, widget::float as float_widget,
};

use crate::components::style::{RADIUS_MD, RADIUS_SM, SPACE_MD, SPACE_SM, SPACE_XS, alpha};
use crate::fonts;

/// One row in the sidebar. `active` flips on the visual state; `message`
/// is what the parent receives when the row is clicked.
///
/// The `shortcut` / `on_close` / `secondary` fields are opt-in extras
/// consumed by [`SidebarPanel`]; plain [`sidebar`] ignores them (they
/// default to `None`, so existing `::new().active()` callers behave
/// exactly as before).
pub struct SidebarItem<Message> {
    pub label: String,
    pub active: bool,
    pub message: Message,
    /// Right-aligned dim shortcut hint (e.g. the `1`..=`9` access key).
    /// Rendered by [`SidebarPanel`]; ignored by plain [`sidebar`].
    pub shortcut: Option<u8>,
    /// When set, [`SidebarPanel`] renders a trailing `×` button that
    /// emits this message. Ignored by plain [`sidebar`].
    pub on_close: Option<Message>,
    /// Dim trailing label (e.g. an unread count). Rendered by
    /// [`SidebarPanel`]; ignored by plain [`sidebar`].
    pub secondary: Option<String>,
}

impl<Message> SidebarItem<Message> {
    pub fn new(label: impl Into<String>, message: Message) -> Self {
        Self {
            label: label.into(),
            active: false,
            message,
            shortcut: None,
            on_close: None,
            secondary: None,
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Attach a right-aligned dim shortcut hint (consumed by
    /// [`SidebarPanel`]).
    pub fn shortcut(mut self, n: u8) -> Self {
        self.shortcut = Some(n);
        self
    }

    /// Attach a trailing `×` close button emitting `msg` (consumed by
    /// [`SidebarPanel`]).
    pub fn on_close(mut self, msg: Message) -> Self {
        self.on_close = Some(msg);
        self
    }

    /// Attach a dim trailing secondary label (consumed by
    /// [`SidebarPanel`]).
    pub fn secondary(mut self, label: impl Into<String>) -> Self {
        self.secondary = Some(label.into());
        self
    }
}

/// A group of sidebar rows with an optional section header label.
/// Labels render in title case as provided (macOS sidebar group style —
/// not forced uppercase). Unlabeled sections render as a plain item
/// group (useful for a top "Welcome" entry above the first headed section).
pub struct SidebarSection<Message> {
    pub label: Option<String>,
    pub items: Vec<SidebarItem<Message>>,
}

impl<Message> SidebarSection<Message> {
    pub fn new(label: impl Into<String>, items: Vec<SidebarItem<Message>>) -> Self {
        Self { label: Some(label.into()), items }
    }

    pub fn unlabeled(items: Vec<SidebarItem<Message>>) -> Self {
        Self { label: None, items }
    }
}

/// Default sidebar width — matches the storybook's nav column. Public
/// so consumers can lay out alongside it (`width = Fill - SIDEBAR_WIDTH`).
pub const SIDEBAR_WIDTH: f32 = 220.0;

/// Build the sidebar panel from its sections. Returns a `Container` so
/// callers can override the kit defaults (a fixed [`SIDEBAR_WIDTH`] wide,
/// full height) by chaining `.width(..)` / `.height(..)` before dropping
/// it into a layout.
pub fn sidebar<'a, Message>(
    sections: Vec<SidebarSection<Message>>,
) -> Container<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    sidebar_with_header(None::<Element<'a, Message, Theme>>, sections)
}

/// Like [`sidebar`], with an optional leading header (brand, search, …)
/// stacked above the section list. Used by the storybook brand block.
pub fn sidebar_with_header<'a, Message>(
    header: Option<Element<'a, Message, Theme>>,
    sections: Vec<SidebarSection<Message>>,
) -> Container<'a, Message, Theme>
where
    Message: Clone + 'a,
{
    let mut col = column![].spacing(SPACE_XS).padding(Padding::from([12, 10]));
    if let Some(header) = header {
        col = col.push(header);
        col = col.push(Space::new().height(Length::Fixed(8.0)));
    }
    for (i, section) in sections.into_iter().enumerate() {
        if i > 0 {
            col = col.push(Space::new().height(Length::Fixed(10.0)));
        }
        if let Some(label) = section.label {
            col = col.push(section_header(label));
        }
        for item in section.items {
            // `sidebar()` never enables reorder, so `render_item` takes
            // the plain `button(..).on_press(item.message)` path. `index`
            // is only read on the reorder path.
            col = col.push(render_item(item, None, 0));
        }
    }
    container(col)
        .style(style)
        .width(Length::Fixed(SIDEBAR_WIDTH))
        .height(Length::Fill)
}


/// One tab in [`vertical_tabs`].
pub struct TabDescriptor<Message> {
    pub label: String,
    pub active: bool,
    pub on_activate: Message,
    pub on_close: Message,
}

impl<Message> TabDescriptor<Message> {
    pub fn new(
        label: impl Into<String>,
        active: bool,
        on_activate: Message,
        on_close: Message,
    ) -> Self {
        Self { label: label.into(), active, on_activate, on_close }
    }
}

/// Size variant for [`vertical_tabs_sized`]. `Normal` reproduces the
/// historical density; `Large` is the roomier browser-chrome variant.
/// This is the kit's canonical size-variant pattern — copy it for other
/// components that grow a size knob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabSize {
    #[default]
    Normal,
    Large,
}

/// Resolved per-size metrics. Values are deliberate, not derived.
struct TabMetrics {
    row_pad_v: u16,
    row_pad_h: u16,
    font: u32,
    close: u32,
    gap: f32,
}

impl TabSize {
    fn metrics(self) -> TabMetrics {
        match self {
            TabSize::Normal => TabMetrics { row_pad_v: 6, row_pad_h: 10, font: 13, close: 15, gap: SPACE_XS },
            TabSize::Large => TabMetrics { row_pad_v: 10, row_pad_h: 12, font: 14, close: 17, gap: SPACE_SM },
        }
    }
}

/// Vertical browser-style tab column at the default ([`TabSize::Normal`])
/// density. Thin wrapper over [`vertical_tabs_sized`].
pub fn vertical_tabs<'a, Message, FHover>(
    tabs: Vec<TabDescriptor<Message>>,
    hovered: Option<usize>,
    on_hover: FHover,
) -> Container<'a, Message, Theme>
where
    Message: Clone + 'a,
    FHover: Fn(Option<usize>) -> Message + 'a,
{
    vertical_tabs_sized(tabs, hovered, on_hover, TabSize::Normal)
}

/// Size-parameterized vertical tab column. Each row is a single-line label
/// (`Wrapping::None` — the caller truncates to control where the ellipsis
/// falls) with the active row highlighted. The close `×` *floats* over the
/// row's right edge (drawn on top via a `stack`) and only appears while that
/// row is hovered — the caller tracks hover by row index via `hovered` +
/// the `on_hover` callback. `TabSize::Large` is the roomier browser-chrome
/// density. Returns a full-size `Container` (kit sidebar styling) so the
/// caller sizes the column — typically behind a draggable divider — by
/// chaining `.width(..)`.
pub fn vertical_tabs_sized<'a, Message, FHover>(
    tabs: Vec<TabDescriptor<Message>>,
    hovered: Option<usize>,
    on_hover: FHover,
    size: TabSize,
) -> Container<'a, Message, Theme>
where
    Message: Clone + 'a,
    FHover: Fn(Option<usize>) -> Message + 'a,
{
    let m = size.metrics();
    let mut col = column![].spacing(m.gap).padding(Padding::from([8, 6]));
    for (i, tab) in tabs.into_iter().enumerate() {
        let TabDescriptor { label, active, on_activate, on_close } = tab;

        let activate = button(
            text(label)
                .font(fonts::ui())
                .size(m.font)
                .wrapping(Wrapping::None),
        )
        .style(move |t, status| item_style(t, status, active))
        .padding(Padding::from([m.row_pad_v, m.row_pad_h]))
        .width(Length::Fill)
        .on_press(on_activate);

        // The close button floats over the row's right edge (a `stack`
        // layer on top of the label), revealed only while this row is
        // hovered — never a second cell that steals label width.
        let row_el: Element<'a, Message> = if hovered == Some(i) {
            let close = button(text("×").font(fonts::ui()).size(m.close))
                .style(|t, status| item_style(t, status, false))
                .padding(Padding::from([0, 7]))
                .on_press(on_close);
            stack![
                activate,
                container(close)
                    .align_x(iced::alignment::Horizontal::Right)
                    .align_y(iced::alignment::Vertical::Center)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(Padding::from([0, 4])),
            ]
            .into()
        } else {
            activate.into()
        };

        col = col.push(
            mouse_area(row_el)
                .on_enter(on_hover(Some(i)))
                .on_exit(on_hover(None)),
        );
    }

    container(col).style(style).height(Length::Fill).width(Length::Fill)
}

fn section_header<'a, Message: 'a>(label: String) -> Element<'a, Message> {
    // Uppercase tracked section labels — graphite tool UI (sola-kit-ds).
    container(
        text(label.to_uppercase())
            .font(fonts::ui_medium())
            .size(10)
            .style(|theme: &Theme| {
                let p = theme.extended_palette();
                iced::widget::text::Style {
                    color: Some(p.secondary.base.text),
                }
            }),
    )
    .padding(Padding {
        top: SPACE_SM + 2.0,  // 6
        bottom: SPACE_SM + 1.0,
        left: SPACE_MD + 2.0, // 10
        right: SPACE_MD + 2.0,
    })
    .into()
}

// ───────────────────────── Panel geometry / helpers ─────────────────────────
//
// Pure functions copied verbatim from sola-terminal's tested sidebar
// (renamed with a `panel_` prefix) so any kit consumer can drive
// resize / reorder gestures without re-deriving the maths. The unit
// tests below are the terminal's, ported here to keep them green.

/// Minimum panel width in logical pixels (resize clamp).
pub const PANEL_W_MIN: f32 = 80.0;
/// Maximum panel width in logical pixels (resize clamp).
pub const PANEL_W_MAX: f32 = 250.0;
/// Default panel width — same as [`SIDEBAR_WIDTH`].
pub const PANEL_W_DEFAULT: f32 = SIDEBAR_WIDTH;
/// Height of each panel row (px). Used by [`panel_drop_index`].
pub const PANEL_ROW_H: f32 = 32.0;
/// Height of the toggle-button header row (px). Used as the list-top
/// offset for [`panel_drop_index`].
pub const PANEL_HEADER_H: f32 = 32.0;
/// Movement threshold (px) below which a press-then-release is a click,
/// not a completed reorder drag.
pub const PANEL_REORDER_THRESHOLD: f32 = 5.0;
/// Sibling glide duration while a row is mid-reorder.
pub const PANEL_REORDER_ANIM_MS: u64 = 180;
/// Subtle lift scale applied to the row under the cursor during reorder.
pub const PANEL_REORDER_LIFT_SCALE: f32 = 1.02;
/// Vertical pitch of one panel row including the column gap — used when
/// siblings slide to open a drop slot.
pub const PANEL_ROW_STRIDE: f32 = PANEL_ROW_H + SPACE_XS;

/// Compute the new panel width from a drag gesture.
///
/// The panel is on the LEFT, so it grows as the cursor moves right (away
/// from the panel) and shrinks as it moves left. Anchor-relative so
/// there is no drift when the cursor re-enters the clamped range after
/// exceeding it:
///
///   `new_width = anchor_width + (cursor_x - anchor_x)`
///
/// Result is clamped to `[PANEL_W_MIN, PANEL_W_MAX]`. Pure, so it is
/// unit-tested without an iced runtime.
pub fn panel_dragged_width(anchor_x: f32, anchor_w: f32, cursor_x: f32) -> f32 {
    let desired = anchor_w + (cursor_x - anchor_x);
    desired.clamp(PANEL_W_MIN, PANEL_W_MAX)
}

/// Return the slot index (0-based, "insert before slot k") the cursor is
/// hovering over, given the top-y of the row list and the row height.
///
/// Formula: `floor((cursor_y - list_top) / row_h)`, clamped to
/// `0..=(n-1)` (returns 0 for `n == 0`).
pub fn panel_drop_index(cursor_y: f32, list_top: f32, row_h: f32, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let rel = cursor_y - list_top;
    if rel < 0.0 {
        return 0;
    }
    let slot = (rel / row_h).floor() as usize;
    slot.min(n - 1)
}

/// Anchor-relative drop slot: the row that was grabbed (`from`) shifted by
/// the number of whole row-heights the cursor has travelled since the
/// press (`cursor_y - start_y`), clamped to `0..=n-1` (returns 0 for
/// `n == 0`).
///
/// Unlike [`panel_drop_index`], this needs no knowledge of the list's
/// absolute top-y, so it stays correct when the panel is nested far down
/// the window (e.g. the storybook demo sitting inside a card). Pure, so it
/// is unit-tested without an iced runtime.
pub fn panel_drop_index_relative(
    from: usize,
    start_y: f32,
    cursor_y: f32,
    row_h: f32,
    n: usize,
) -> usize {
    if n == 0 {
        return 0;
    }
    let delta = ((cursor_y - start_y) / row_h).round() as i64;
    let to = from as i64 + delta;
    to.clamp(0, n as i64 - 1) as usize
}

/// Target vertical offset (px) for the row at `index` while the item at
/// `from` is provisionally over drop slot `to`.
///
/// The dragged row itself always returns `0.0` (it follows the pointer
/// via a separate translate). Other rows between `from` and `to` shift by
/// one [`PANEL_ROW_STRIDE`] so a gap opens at the drop slot.
///
/// Pure — unit-tested without an iced runtime.
pub fn panel_sibling_offset(from: usize, to: usize, index: usize) -> f32 {
    if index == from || from == to {
        return 0.0;
    }
    if from < to {
        // Dragging down: rows in (from, to] slide up into the vacated slot.
        if index > from && index <= to {
            -PANEL_ROW_STRIDE
        } else {
            0.0
        }
    } else {
        // Dragging up: rows in [to, from) slide down.
        if index >= to && index < from {
            PANEL_ROW_STRIDE
        } else {
            0.0
        }
    }
}

/// Live per-row offset animations for a sidebar reorder gesture.
///
/// Owned by the app (terminal / storybook). Call [`Self::sync`] on each
/// cursor move and animation tick while a drag is live; [`Self::clear`]
/// on release. [`SidebarPanel`] samples offsets at view time.
#[derive(Debug, Clone, Default)]
pub struct ReorderAnim {
    rows: Vec<Animation<f32>>,
}

impl ReorderAnim {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop all row animations (gesture ended).
    pub fn clear(&mut self) {
        self.rows.clear();
    }

    /// True while any sibling offset is still in flight.
    pub fn is_animating(&self, at: Instant) -> bool {
        self.rows.iter().any(|a| a.is_animating(at))
    }

    /// Ensure `n` row animations and retarget each non-dragged row toward
    /// the offset for provisional drop slot `to`.
    pub fn sync(&mut self, from: usize, to: usize, n: usize, at: Instant) {
        while self.rows.len() < n {
            self.rows.push(
                Animation::new(0.0)
                    .duration(std::time::Duration::from_millis(PANEL_REORDER_ANIM_MS))
                    .easing(Easing::EaseOut),
            );
        }
        if self.rows.len() > n {
            self.rows.truncate(n);
        }
        for i in 0..n {
            let target = panel_sibling_offset(from, to, i);
            if self.rows[i].value() != target {
                self.rows[i].go_mut(target, at);
            }
        }
    }

    /// Interpolated Y offset for `index` at `at` (0 when unknown).
    pub fn offset(&self, index: usize, at: Instant) -> f32 {
        self.rows
            .get(index)
            .map(|a| a.interpolate_with(|v| v, at))
            .unwrap_or(0.0)
    }
}

/// Move the item at `from` to `to` in `order`, returning the new order.
/// Both indices are clamped into range; `from == to` returns the order
/// unchanged. Pure "remove then insert", consistent with
/// [`panel_drop_index`]'s slot model.
pub fn panel_reordered(order: &[String], from: usize, to: usize) -> Vec<String> {
    if order.is_empty() {
        return Vec::new();
    }
    let from = from.min(order.len() - 1);
    let to = to.min(order.len() - 1);
    let mut v: Vec<String> = order.to_vec();
    let item = v.remove(from);
    v.insert(to, item);
    v
}

/// Given a new ordering of ids, return the `(id, new_ordinal)` pairs for
/// ids whose ordinal changed. The new ordinal is the 0-based position in
/// `new_order`; pairs are emitted only when `new_ordinal != current`.
pub fn panel_renumber_changed(
    ordinals: &HashMap<String, u32>,
    new_order: &[String],
) -> Vec<(String, u32)> {
    new_order
        .iter()
        .enumerate()
        .filter_map(|(k, id)| {
            let new_ordinal = k as u32;
            let cur = ordinals.get(id).copied().unwrap_or(u32::MAX);
            if cur != new_ordinal {
                Some((id.clone(), new_ordinal))
            } else {
                None
            }
        })
        .collect()
}

// ───────────────────────────── Reorder config ───────────────────────────────

/// Reorder wiring handed to [`SidebarPanel::reorderable`].
///
/// The `'a` lifetime ties the boxed `on_press` closure to the same scope
/// as the rest of the built `Element` (it borrows the consumer's state to
/// produce a message per row index).
pub struct ReorderCfg<'a, Message> {
    /// Maps a pressed row's index → the message that begins the gesture
    /// (the consumer's `ReorderStart(usize)`).
    pub on_press: Box<dyn Fn(usize) -> Message + 'a>,
    /// `Some((from_index, start_y))` once the gesture is a real drag;
    /// `None` during a not-yet-moved press or when idle. Drives the
    /// live-reorder preview (and the grabbing cursor once past the
    /// movement threshold). Consumers should keep this `None` until
    /// [`PANEL_REORDER_THRESHOLD`] so a plain click never shuffles rows.
    pub active: Option<(usize, f32)>,
    /// Current cursor-y during the gesture (used with
    /// [`panel_drop_index_relative`] to place the dragged row in the
    /// live-reorder preview).
    pub cursor_y: f32,
    /// Sibling glide animations. When `None`, offsets snap instantly.
    /// Consumers that want the glide keep a [`ReorderAnim`] and pass it
    /// here after calling [`ReorderAnim::sync`].
    pub anim: Option<&'a ReorderAnim>,
}

// ──────────────────────────── Shared item render ────────────────────────────

/// Render one item row, shared by [`sidebar`] and [`SidebarPanel::build`].
///
/// When `reorder` is `None` this is the legacy path — a plain
/// `button(label).on_press(item.message)` — so `sidebar()` is behaviour-
/// identical to the old `sidebar_item`.
///
/// When `reorder` is `Some`, the press-bearing widget is a *non-pressable*
/// `container` wrapped in a `mouse_area` (an inner pressable `button`
/// would `capture_event` the press and the reorder gesture would never
/// fire). The `×` close button — which IS pressable — therefore sits
/// OUTSIDE that `mouse_area`, as a sibling in the row.
fn render_item<'a, Message>(
    item: SidebarItem<Message>,
    reorder: Option<&ReorderCfg<'a, Message>>,
    index: usize,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let SidebarItem { label, active, message, shortcut, on_close, secondary } = item;

    // ── Plain path (no reorder) — preserves the exact prior look. ──
    let Some(reorder) = reorder else {
        // Label + optional secondary/hint, with left accent bar when active.
        let content = row_with_active_bar(
            item_content(&label, secondary.as_deref(), shortcut),
            active,
        );
        let btn = button(content)
            .style(move |t, status| item_style(t, status, active))
            .padding(Padding::from([6, 10]))
            .width(Length::Fill)
            .on_press(message);
        // A close button, if requested, sits beside the row button.
        if let Some(close_msg) = on_close {
            return row![btn, close_button(close_msg)]
                .spacing(SPACE_XS)
                .align_y(iced::Alignment::Center)
                .into();
        }
        return btn.into();
    };

    // ── Reorder-enabled path. ──
    // Live-reorder chrome is active only while `reorder.active` is `Some` —
    // consumers populate that after the movement threshold so a plain press
    // leaves the strip alone. While active, [`SidebarPanel::build`] lifts
    // the grabbed row under the cursor and glides siblings into the gap.
    // `index` is the item's *stable* (pre-drag) index in the consumer's
    // order — used for press messages and for the grabbing cursor.
    let is_dragged = matches!(reorder.active, Some((from, _)) if from == index);

    let content = row_with_active_bar(
        item_content(&label, secondary.as_deref(), shortcut),
        active,
    );
    let pressable = mouse_area(
        container(content)
            .width(Length::Fill)
            .padding(Padding::from([6, 10]))
            .style(move |theme: &Theme| row_container_style(theme, active)),
    )
    // Pointer at rest; grabbing while this row is the one in flight.
    .interaction(if is_dragged {
        mouse::Interaction::Grabbing
    } else {
        mouse::Interaction::Pointer
    })
    .on_press((reorder.on_press)(index));

    let row_el: Element<'a, Message> = if let Some(close_msg) = on_close {
        row![pressable, close_button(close_msg)]
            .spacing(SPACE_XS)
            .align_y(iced::Alignment::Center)
            .into()
    } else {
        pressable.into()
    };

    // Motion is applied by the caller via [`with_reorder_motion`] so the
    // drag path can pass pointer-relative dy for the lifted row.
    row_el
}

/// Apply vertical motion (and optional lift chrome) for a reorder row.
fn with_reorder_motion<'a, Message>(
    el: Element<'a, Message>,
    dy: f32,
    lifted: bool,
) -> Element<'a, Message>
where
    Message: 'a,
{
    if dy == 0.0 && !lifted {
        return el;
    }
    let mut f = float(el).translate(move |_, _| Vector::new(0.0, dy));
    if lifted {
        f = f.scale(PANEL_REORDER_LIFT_SCALE).style(|_| float_widget::Style {
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                offset: Vector::new(0.0, 2.0),
                blur_radius: 8.0,
            },
            shadow_border_radius: RADIUS_SM.into(),
        });
    }
    f.into()
}

/// The label + optional secondary + optional shortcut hint, laid out in
/// a row. `collapsed_number` callers use [`collapsed_content`] instead.
fn item_content<'a, Message: 'a>(
    label: &str,
    secondary: Option<&str>,
    shortcut: Option<u8>,
) -> Element<'a, Message> {
    let mut r = row![text(label.to_string()).font(fonts::ui()).size(13)]
        .spacing(SPACE_XS)
        .align_y(iced::Alignment::Center);
    // Spacer pushes secondary/hint to the right.
    r = r.push(Space::new().width(Length::Fill));
    if let Some(sec) = secondary {
        r = r.push(dim_label(sec));
    }
    if let Some(n) = shortcut {
        r = r.push(dim_label(&n.to_string()));
    }
    r.width(Length::Fill).into()
}

/// Collapsed-row content: just the shortcut number (or index+1), centred.
fn collapsed_content<'a, Message: 'a>(number: u8) -> Element<'a, Message> {
    container(text(number.to_string()).font(fonts::ui()).size(13))
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
}

fn dim_label<'a, Message: 'a>(s: &str) -> Element<'a, Message> {
    text(s.to_string())
        .font(fonts::ui())
        .size(12)
        .style(|theme: &Theme| {
            let p = theme.extended_palette();
            // Dim = the base foreground at reduced alpha, so the hint reads as
            // a muted accent to the label regardless of theme (secondary.base.text
            // is not reliably dim — it renders ~white in the active theme).
            let c = p.background.base.text;
            iced::widget::text::Style {
                color: Some(iced::Color { a: 0.45, ..c }),
            }
        })
        .into()
}

/// Trailing `×` close button. Pressable, so it must sit OUTSIDE any
/// reorder `mouse_area` (see [`render_item`]).
fn close_button<'a, Message: Clone + 'a>(msg: Message) -> Element<'a, Message> {
    button(text("×").font(fonts::ui()).size(14))
        .style(|t, status| item_style(t, status, false))
        .padding(Padding::from([2, 8]))
        .on_press(msg)
        .into()
}

// ─────────────────────────────── SidebarPanel ───────────────────────────────

/// Opt-in richer sidebar: collapse/expand, drag-to-resize, drag-reorder,
/// per-item shortcut hints / close buttons / secondary labels, plus an
/// optional footer.
///
/// The APP owns the cursor-move/release subscription (mirroring
/// sola-monitor's `DividerPress` + global listener pattern); this builder
/// only renders the divider `mouse_area` and the full-window overlay that
/// keeps the resize cursor while `dragging`. Returns an `Element` (a
/// `row!`/`stack!`), not a `Container`, so it composes directly.
pub struct SidebarPanel<'a, Message> {
    sections: Vec<SidebarSection<Message>>,
    collapse: Option<(bool, Message)>,
    /// `(width, dragging, on_press, colors)` — `colors` is `None` for
    /// theme-default divider chrome.
    resize: Option<(f32, bool, Message, Option<crate::components::DividerColors>)>,
    reorder: Option<ReorderCfg<'a, Message>>,
    footer: Option<Element<'a, Message, Theme>>,
}

impl<'a, Message> SidebarPanel<'a, Message>
where
    Message: Clone + 'a,
{
    pub fn new(sections: Vec<SidebarSection<Message>>) -> Self {
        Self {
            sections,
            collapse: None,
            resize: None,
            reorder: None,
            footer: None,
        }
    }

    /// Render a toggle header (»/«) emitting `on_toggle`. `collapsed`
    /// controls both the glyph and the narrow (icon-only) layout.
    pub fn collapsible(mut self, collapsed: bool, on_toggle: Message) -> Self {
        self.collapse = Some((collapsed, on_toggle));
        self
    }

    /// Render a drag divider on the right edge. `width` is the current
    /// column width; `dragging` toggles the full-window overlay;
    /// `on_divider_press` begins the resize gesture. Divider colours
    /// use the theme default (canvas | border | canvas); prefer
    /// [`Self::resizable_with`] when the adjacent surfaces differ.
    pub fn resizable(mut self, width: f32, dragging: bool, on_divider_press: Message) -> Self {
        self.resize = Some((width, dragging, on_divider_press, None));
        self
    }

    /// Like [`Self::resizable`], but with explicit **a | line | b**
    /// divider colours so the hit strip matches the panel and its
    /// neighbour (e.g. raised sidebar | terminal canvas).
    pub fn resizable_with(
        mut self,
        width: f32,
        dragging: bool,
        on_divider_press: Message,
        colors: crate::components::DividerColors,
    ) -> Self {
        self.resize = Some((width, dragging, on_divider_press, Some(colors)));
        self
    }

    /// Enable drag-to-reorder using the supplied [`ReorderCfg`].
    pub fn reorderable(mut self, cfg: ReorderCfg<'a, Message>) -> Self {
        self.reorder = Some(cfg);
        self
    }

    /// A footer element pinned below the rows (hidden when collapsed).
    pub fn footer(mut self, el: Element<'a, Message, Theme>) -> Self {
        self.footer = Some(el);
        self
    }

    pub fn build(self) -> Element<'a, Message, Theme> {
        let SidebarPanel { sections, collapse, resize, reorder, footer } = self;

        let collapsed = collapse.as_ref().map(|(c, _)| *c).unwrap_or(false);
        let reorder_ref = reorder.as_ref();

        let mut col = column![].spacing(SPACE_XS).padding(Padding::from([8, 6]));

        // Toggle header.
        if let Some((_, on_toggle)) = &collapse {
            let glyph = if collapsed { "»" } else { "«" };
            col = col.push(
                button(text(glyph).font(fonts::ui()).size(13))
                    .style(|t, status| item_style(t, status, false))
                    .padding(Padding::from([6, 10]))
                    .width(Length::Fill)
                    .on_press(on_toggle.clone()),
            );
        }

        // Total item count across all sections — clamps the drop slot.
        let total_items: usize = sections.iter().map(|s| s.items.len()).sum();
        let dragging = reorder_ref.and_then(|r| r.active);

        if let Some((from, start_y)) = dragging {
            // Live preview: keep the *original* order for layout so the
            // grabbed row can float with the pointer while siblings glide
            // into a gap at the provisional drop slot. Section headers are
            // omitted (they fight a mid-drag shuffle).
            let cursor_y = reorder_ref.map(|r| r.cursor_y).unwrap_or(0.0);
            let to = panel_drop_index_relative(
                from,
                start_y,
                cursor_y,
                PANEL_ROW_H,
                total_items,
            );
            let now = Instant::now();
            let anim = reorder_ref.and_then(|r| r.anim);
            let mut flat: Vec<(usize, SidebarItem<Message>)> = Vec::with_capacity(total_items);
            let mut row_index = 0usize;
            for section in sections {
                for item in section.items {
                    flat.push((row_index, item));
                    row_index += 1;
                }
            }
            for (stable_index, item) in flat {
                let is_dragged = stable_index == from;
                let dy = if is_dragged {
                    cursor_y - start_y
                } else if let Some(anim) = anim {
                    anim.offset(stable_index, now)
                } else {
                    panel_sibling_offset(from, to, stable_index)
                };
                let row_el = if collapsed {
                    collapsed_row(&item, stable_index, reorder_ref)
                } else {
                    render_item(item, reorder_ref, stable_index)
                };
                col = col.push(with_reorder_motion(row_el, dy, is_dragged));
            }
        } else {
            // At rest: original sectioned layout (headers + gaps).
            let mut row_index = 0usize;
            for (si, section) in sections.into_iter().enumerate() {
                if si > 0 && !collapsed {
                    col = col.push(Space::new().height(Length::Fixed(12.0)));
                }
                if let Some(label) = section.label {
                    if !collapsed {
                        col = col.push(section_header(label));
                    }
                }
                for item in section.items {
                    if collapsed {
                        col = col.push(collapsed_row(&item, row_index, reorder_ref));
                    } else {
                        col = col.push(render_item(item, reorder_ref, row_index));
                    }
                    row_index += 1;
                }
            }
        }

        // Footer (hidden when collapsed).
        if let Some(footer) = footer {
            if !collapsed {
                col = col.push(Space::new().height(Length::Fill));
                col = col.push(footer);
            }
        }

        let width = match &resize {
            Some((w, _, _, _)) if !collapsed => *w,
            _ if collapsed => 36.0,
            _ => SIDEBAR_WIDTH,
        };

        let panel = container(col)
            .style(style)
            .width(Length::Fixed(width))
            .height(Length::Fill);

        // Gesture flags captured before we move `resize` into the divider.
        let resize_dragging = resize.as_ref().is_some_and(|(_, d, _, _)| *d);
        // `active` is only set after the movement threshold, so this is a
        // real drag (not a plain press).
        let reorder_dragging = reorder_ref.is_some_and(|r| r.active.is_some());

        // Compose optional resize divider — same three-band hit strip as
        // kit `split` / terminal pane dividers.
        let body: Element<'a, Message, Theme> = match resize {
            Some((_, _, on_press, colors)) => {
                let divider = match colors {
                    Some(c) => crate::components::vertical_divider_with(on_press, c),
                    None => crate::components::vertical_divider(on_press),
                };
                row![panel, divider].height(Length::Fill).into()
            }
            None => panel.into(),
        };

        // Full-window transparent overlay while a gesture is live so a fast
        // drag keeps the right cursor (iced has no pointer capture). Resize
        // and reorder are mutually exclusive in practice (different press
        // targets); resize wins if both somehow fire.
        if resize_dragging {
            stack![
                body,
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .interaction(iced::mouse::Interaction::ResizingColumn),
            ]
            .into()
        } else if reorder_dragging {
            stack![
                body,
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .interaction(iced::mouse::Interaction::Grabbing),
            ]
            .into()
        } else {
            body
        }
    }
}

/// Collapsed (icon-only) row: shows just the shortcut number (or
/// `index + 1`), pressable via the same reorder/select mouse_area when
/// reorder is enabled, else a plain button.
fn collapsed_row<'a, Message>(
    item: &SidebarItem<Message>,
    index: usize,
    reorder: Option<&ReorderCfg<'a, Message>>,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let number = item.shortcut.unwrap_or((index + 1) as u8);
    let active = item.active;
    match reorder {
        Some(cfg) => mouse_area(
            container(collapsed_content::<Message>(number))
                .width(Length::Fill)
                .padding(Padding::from([6, 4]))
                .style(move |theme: &Theme| row_container_style(theme, active)),
        )
        .on_press((cfg.on_press)(index))
        .into(),
        None => button(collapsed_content::<Message>(number))
            .style(move |t, status| item_style(t, status, active))
            .padding(Padding::from([6, 4]))
            .width(Length::Fill)
            .on_press(item.message.clone())
            .into(),
    }
}

pub fn style(theme: &Theme) -> container::Style {
    let _p = theme.extended_palette();
    // OD `--material-sidebar`: cool #121722 (slightly off pure raised).
    // Full outline is intentionally off — the storybook / shell draws a
    // single right hairline separator against the content column.
    let material = Color::from_rgb(0.071, 0.090, 0.133); // #121722
    container::Style {
        background: Some(Background::Color(material)),
        border: Border::default(),
        ..container::Style::default()
    }
}

/// Background style for a row rendered as a non-pressable `container`
/// (the reorder path). Selected rows use the dedicated
/// [`crate::theme::selection`] highlight (matching [`item_style`]);
/// everything else is flat. Mid-drag lift (scale + shadow) is applied
/// by [`with_reorder_motion`], not here.
fn row_container_style(theme: &Theme, active: bool) -> container::Style {
    let p = theme.extended_palette();
    let bg = active.then(|| Background::Color(crate::theme::selection()));
    container::Style {
        background: bg,
        text_color: Some(p.background.base.text),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_MD.into(),
        },
        ..container::Style::default()
    }
}

/// Style fn for an individual sidebar row. Exposed so consumers
/// building custom row widgets (e.g. with leading icons) can match the
/// kit's visual language.
///
/// Active = quiet selection wash + rounded corners. The **left accent
/// bar** is drawn in the row content ([`row_with_active_bar`]), not as a
/// full focus-ring border.
pub fn item_style(theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let p = theme.extended_palette();
    if active {
        return button::Style {
            background: Some(Background::Color(crate::theme::selection())),
            text_color: p.background.base.text,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: RADIUS_MD.into(),
            },
            shadow: Default::default(),
            snap: false,
        };
    }
    let bg = match status {
        button::Status::Hovered => alpha(p.background.strong.color, 0.70),
        _ => Color::TRANSPARENT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: p.background.base.text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_MD.into(),
        },
        shadow: Default::default(),
        snap: false,
    }
}

/// Prefix `content` with a 2px accent bar when `active` (OD left inset).
fn row_with_active_bar<'a, Message: 'a>(
    content: Element<'a, Message, Theme>,
    active: bool,
) -> Element<'a, Message, Theme> {
    if !active {
        return content;
    }
    let bar = container(Space::new().width(Length::Fixed(2.0)).height(Length::Fixed(14.0)))
        .style(|theme: &Theme| {
            let accent = theme.extended_palette().primary.base.color;
            container::Style {
                background: Some(Background::Color(Color { a: 0.85, ..accent })),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 1.0.into(),
                },
                ..container::Style::default()
            }
        });
    row![bar, content]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_size_metrics_are_stable() {
        let n = TabSize::Normal.metrics();
        assert_eq!((n.row_pad_v, n.row_pad_h, n.font, n.close), (6, 10, 13, 15));
        assert_eq!(n.gap, SPACE_XS);

        let l = TabSize::Large.metrics();
        assert_eq!((l.row_pad_v, l.row_pad_h, l.font, l.close), (10, 12, 14, 17));
        assert_eq!(l.gap, SPACE_SM);

        assert_eq!(TabSize::default(), TabSize::Normal);
    }

    fn sv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // --- panel_dragged_width ---

    #[test]
    fn dragged_width_widens_on_right_drag() {
        let w = panel_dragged_width(200.0, 120.0, 250.0);
        assert_eq!(w, 170.0);
    }

    #[test]
    fn dragged_width_narrows_on_left_drag() {
        let w = panel_dragged_width(200.0, 160.0, 160.0);
        assert_eq!(w, 120.0);
    }

    #[test]
    fn dragged_width_clamps_min() {
        let w = panel_dragged_width(200.0, 100.0, 0.0);
        assert_eq!(w, PANEL_W_MIN);
    }

    #[test]
    fn dragged_width_clamps_max() {
        let w = panel_dragged_width(200.0, 200.0, 600.0);
        assert_eq!(w, PANEL_W_MAX);
    }

    #[test]
    fn dragged_width_no_movement() {
        let w = panel_dragged_width(200.0, 160.0, 200.0);
        assert_eq!(w, 160.0);
    }

    // --- panel_sibling_offset ---

    #[test]
    fn sibling_offset_drag_down_shifts_intervening_rows_up() {
        // from 0 → to 2: rows 1 and 2 slide up
        assert_eq!(panel_sibling_offset(0, 2, 0), 0.0);
        assert_eq!(panel_sibling_offset(0, 2, 1), -PANEL_ROW_STRIDE);
        assert_eq!(panel_sibling_offset(0, 2, 2), -PANEL_ROW_STRIDE);
        assert_eq!(panel_sibling_offset(0, 2, 3), 0.0);
    }

    #[test]
    fn sibling_offset_drag_up_shifts_intervening_rows_down() {
        // from 2 → to 0: rows 0 and 1 slide down
        assert_eq!(panel_sibling_offset(2, 0, 0), PANEL_ROW_STRIDE);
        assert_eq!(panel_sibling_offset(2, 0, 1), PANEL_ROW_STRIDE);
        assert_eq!(panel_sibling_offset(2, 0, 2), 0.0);
        assert_eq!(panel_sibling_offset(2, 0, 3), 0.0);
    }

    #[test]
    fn sibling_offset_same_slot_is_zero() {
        assert_eq!(panel_sibling_offset(1, 1, 0), 0.0);
        assert_eq!(panel_sibling_offset(1, 1, 1), 0.0);
        assert_eq!(panel_sibling_offset(1, 1, 2), 0.0);
    }

    // --- panel_reordered ---

    #[test]
    fn reordered_move_down() {
        let result = panel_reordered(&sv(&["a", "b", "c"]), 0, 2);
        assert_eq!(result, sv(&["b", "c", "a"]));
    }

    #[test]
    fn reordered_move_up() {
        let result = panel_reordered(&sv(&["a", "b", "c"]), 2, 0);
        assert_eq!(result, sv(&["c", "a", "b"]));
    }

    #[test]
    fn reordered_noop_same_index() {
        let result = panel_reordered(&sv(&["a", "b", "c"]), 1, 1);
        assert_eq!(result, sv(&["a", "b", "c"]));
    }

    #[test]
    fn reordered_clamps_to_out_of_range() {
        let result = panel_reordered(&sv(&["a", "b", "c"]), 0, 999);
        assert_eq!(result, sv(&["b", "c", "a"]));
    }

    #[test]
    fn reordered_from_clamps_out_of_range() {
        let result = panel_reordered(&sv(&["a", "b", "c"]), 999, 0);
        assert_eq!(result, sv(&["c", "a", "b"]));
    }

    #[test]
    fn reordered_empty_slice() {
        let result = panel_reordered(&[], 0, 0);
        assert!(result.is_empty());
    }

    // --- panel_drop_index ---

    #[test]
    fn drop_index_slot_zero() {
        let idx = panel_drop_index(PANEL_HEADER_H, PANEL_HEADER_H, PANEL_ROW_H, 3);
        assert_eq!(idx, 0);
    }

    #[test]
    fn drop_index_middle_slot() {
        let idx =
            panel_drop_index(PANEL_HEADER_H + PANEL_ROW_H * 1.5, PANEL_HEADER_H, PANEL_ROW_H, 3);
        assert_eq!(idx, 1);
    }

    #[test]
    fn drop_index_past_end_clamps() {
        let idx =
            panel_drop_index(PANEL_HEADER_H + PANEL_ROW_H * 100.0, PANEL_HEADER_H, PANEL_ROW_H, 3);
        assert_eq!(idx, 2);
    }

    #[test]
    fn drop_index_above_list_clamps_to_zero() {
        let idx = panel_drop_index(0.0, PANEL_HEADER_H, PANEL_ROW_H, 3);
        assert_eq!(idx, 0);
    }

    // --- panel_drop_index_relative ---

    #[test]
    fn drop_index_relative_no_movement_stays_put() {
        let to = panel_drop_index_relative(2, 100.0, 100.0, PANEL_ROW_H, 5);
        assert_eq!(to, 2);
    }

    #[test]
    fn drop_index_relative_down_one_row() {
        let to = panel_drop_index_relative(1, 100.0, 100.0 + PANEL_ROW_H, PANEL_ROW_H, 5);
        assert_eq!(to, 2);
    }

    #[test]
    fn drop_index_relative_up_two_rows() {
        let to = panel_drop_index_relative(3, 100.0, 100.0 - 2.0 * PANEL_ROW_H, PANEL_ROW_H, 5);
        assert_eq!(to, 1);
    }

    #[test]
    fn drop_index_relative_rounds_to_nearest_row() {
        // 0.6 of a row down rounds to a full row…
        let to = panel_drop_index_relative(0, 0.0, PANEL_ROW_H * 0.6, PANEL_ROW_H, 5);
        assert_eq!(to, 1);
        // …0.4 of a row down rounds back to the same row.
        let to = panel_drop_index_relative(0, 0.0, PANEL_ROW_H * 0.4, PANEL_ROW_H, 5);
        assert_eq!(to, 0);
    }

    #[test]
    fn drop_index_relative_clamps_below_zero() {
        let to = panel_drop_index_relative(0, 100.0, -500.0, PANEL_ROW_H, 5);
        assert_eq!(to, 0);
    }

    #[test]
    fn drop_index_relative_clamps_past_end() {
        let to = panel_drop_index_relative(4, 0.0, 10_000.0, PANEL_ROW_H, 5);
        assert_eq!(to, 4);
    }

    #[test]
    fn drop_index_relative_empty_is_zero() {
        let to = panel_drop_index_relative(0, 0.0, 100.0, PANEL_ROW_H, 0);
        assert_eq!(to, 0);
    }

    // --- panel_renumber_changed ---

    fn ordinal_map(pairs: &[(&str, u32)]) -> HashMap<String, u32> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn renumber_changed_detects_changed_pairs() {
        let ordinals = ordinal_map(&[("a", 0), ("b", 1), ("c", 2)]);
        let new_order = sv(&["b", "c", "a"]);
        let changed = panel_renumber_changed(&ordinals, &new_order);
        assert_eq!(changed.len(), 3);
        assert!(changed.contains(&("b".to_string(), 0)));
        assert!(changed.contains(&("c".to_string(), 1)));
        assert!(changed.contains(&("a".to_string(), 2)));
    }

    #[test]
    fn renumber_changed_no_changes_when_same_order() {
        let ordinals = ordinal_map(&[("a", 0), ("b", 1), ("c", 2)]);
        let new_order = sv(&["a", "b", "c"]);
        let changed = panel_renumber_changed(&ordinals, &new_order);
        assert!(changed.is_empty());
    }

    #[test]
    fn renumber_changed_adjacent_swap_only_two_changed() {
        let ordinals = ordinal_map(&[("a", 0), ("b", 1), ("c", 2)]);
        let new_order = sv(&["a", "c", "b"]);
        let changed = panel_renumber_changed(&ordinals, &new_order);
        assert_eq!(changed.len(), 2);
        assert!(changed.contains(&("c".to_string(), 1)));
        assert!(changed.contains(&("b".to_string(), 2)));
    }
}
