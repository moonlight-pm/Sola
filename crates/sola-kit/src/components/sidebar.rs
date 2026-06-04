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

use iced::widget::{
    Container, Space, button, column, container, mouse_area, row, stack, text,
};
use iced::{Background, Border, Color, Element, Length, Padding, Theme};

use crate::components::style::{RADIUS_SM, SPACE_XS};
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

/// A group of sidebar rows with an optional uppercase header label.
/// Unlabeled sections render as a plain item group (useful for a top
/// "Welcome" entry that sits above the first headed section).
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
pub const SIDEBAR_WIDTH: f32 = 200.0;

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
    let mut col = column![].spacing(SPACE_XS).padding(Padding::from([8, 6]));
    for (i, section) in sections.into_iter().enumerate() {
        if i > 0 {
            col = col.push(Space::new().height(Length::Fixed(12.0)));
        }
        if let Some(label) = section.label {
            col = col.push(section_header(label));
        }
        for item in section.items {
            // `sidebar()` never enables reorder, so `render_item` takes
            // the plain `button(..).on_press(item.message)` path — byte-
            // for-byte the prior `sidebar_item` behaviour. `index`/`n` are
            // only read on the reorder path, so the values are irrelevant.
            col = col.push(render_item(item, None, 0, 0));
        }
    }
    container(col)
        .style(style)
        .width(Length::Fixed(SIDEBAR_WIDTH))
        .height(Length::Fill)
}

fn section_header<'a, Message: 'a>(label: String) -> Element<'a, Message> {
    container(
        text(label.to_uppercase())
            .font(fonts::chrome())
            .size(11)
            .style(|theme: &Theme| {
                let p = theme.extended_palette();
                iced::widget::text::Style { color: Some(p.secondary.base.text) }
            }),
    )
    .padding(Padding::from([6, 10]))
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
    /// `Some((from_index, start_y))` while a drag is active; drives the
    /// drop-target highlight. `None` when idle.
    pub active: Option<(usize, f32)>,
    /// Current cursor-y during the gesture (used with [`panel_drop_index`]
    /// to compute the highlighted drop slot).
    pub cursor_y: f32,
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
    n: usize,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let SidebarItem { label, active, message, shortcut, on_close, secondary } = item;

    // ── Plain path (no reorder) — preserves the exact prior look. ──
    let Some(reorder) = reorder else {
        // Build the inner content: label + optional secondary + hint.
        let content = item_content(&label, secondary.as_deref(), shortcut);
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
    // Highlight this row when it is the live drop target (the slot the
    // cursor currently hovers over) and not the row being dragged from.
    let is_drop_target = match reorder.active {
        Some((from, _)) => {
            let slot =
                panel_drop_index(reorder.cursor_y, PANEL_HEADER_H, PANEL_ROW_H, n);
            slot == index && from != index
        }
        None => false,
    };

    let content = item_content(&label, secondary.as_deref(), shortcut);
    let pressable = mouse_area(
        container(content)
            .width(Length::Fill)
            .padding(Padding::from([6, 10]))
            .style(move |theme: &Theme| {
                row_container_style(theme, active, is_drop_target)
            }),
    )
    .on_press((reorder.on_press)(index));

    if let Some(close_msg) = on_close {
        row![pressable, close_button(close_msg)]
            .spacing(SPACE_XS)
            .align_y(iced::Alignment::Center)
            .into()
    } else {
        pressable.into()
    }
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

/// Dim trailing text (secondary label / shortcut hint).
fn dim_label<'a, Message: 'a>(s: &str) -> Element<'a, Message> {
    text(s.to_string())
        .font(fonts::ui())
        .size(12)
        .style(|theme: &Theme| {
            let p = theme.extended_palette();
            iced::widget::text::Style { color: Some(p.secondary.base.text) }
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
    resize: Option<(f32, bool, Message)>,
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
    /// `on_divider_press` begins the resize gesture.
    pub fn resizable(mut self, width: f32, dragging: bool, on_divider_press: Message) -> Self {
        self.resize = Some((width, dragging, on_divider_press));
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

        // Total item count across all sections — the `n` passed to the
        // drop-index geometry so the highlighted slot clamps correctly.
        let total_items: usize = sections.iter().map(|s| s.items.len()).sum();

        // Item rows. A global row index drives the reorder geometry and
        // the collapsed fallback number, counted across all sections.
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
                    col = col.push(render_item(item, reorder_ref, row_index, total_items));
                }
                row_index += 1;
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
            Some((w, _, _)) if !collapsed => *w,
            _ if collapsed => 36.0,
            _ => SIDEBAR_WIDTH,
        };

        let panel = container(col)
            .style(style)
            .width(Length::Fixed(width))
            .height(Length::Fill);

        // No resize → just the column.
        let Some((_, dragging, on_press)) = resize else {
            return panel.into();
        };

        // A thin divider mouse_area to the right of the column.
        let divider = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .style(divider_style)
                .width(Length::Fixed(6.0))
                .height(Length::Fill),
        )
        .interaction(iced::mouse::Interaction::ResizingColumn)
        .on_press(on_press);

        let body: Element<'a, Message, Theme> =
            row![panel, divider].height(Length::Fill).into();

        if dragging {
            // Full-window transparent overlay so a fast drag keeps the
            // resize cursor / capture (iced has no pointer capture).
            stack![
                body,
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .interaction(iced::mouse::Interaction::ResizingColumn),
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
                .style(move |theme: &Theme| row_container_style(theme, active, false)),
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
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        // No border — the sidebar track is a flat raised panel; the
        // divider/zoning chrome around it carries any separating line.
        border: Border::default(),
        ..container::Style::default()
    }
}

/// Thin resize divider fill — picks up the panel's stronger background so
/// it reads as a hairline groove between the column and its neighbour.
fn divider_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.strong.color)),
        border: Border::default(),
        ..container::Style::default()
    }
}

/// Background style for a row rendered as a non-pressable `container`
/// (the reorder path). Drop-target highlight wins over active; otherwise
/// hover-less transparent (the `mouse_area` doesn't expose a hover
/// status, so resting rows stay flat).
fn row_container_style(theme: &Theme, active: bool, drop_target: bool) -> container::Style {
    let p = theme.extended_palette();
    let bg = if drop_target {
        Some(Background::Color(p.primary.weak.color))
    } else if active {
        Some(Background::Color(p.background.strong.color))
    } else {
        None
    };
    container::Style {
        background: bg,
        text_color: Some(p.background.base.text),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_SM.into(),
        },
        ..container::Style::default()
    }
}

/// Style fn for an individual sidebar row. Exposed so consumers
/// building custom row widgets (e.g. with leading icons) can match the
/// kit's visual language.
pub fn item_style(theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let p = theme.extended_palette();
    let bg = if active {
        p.background.strong.color
    } else {
        match status {
            button::Status::Hovered => p.background.strong.color,
            _ => Color::TRANSPARENT,
        }
    };
    let text_color = if active {
        p.primary.base.color
    } else {
        p.background.base.text
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_SM.into(),
        },
        shadow: Default::default(),
        snap: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
