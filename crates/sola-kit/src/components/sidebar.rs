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
//! per-item shortcut hints / close buttons / secondary labels, and
//! **section-scoped scroll with overflow chips** — use the opt-in
//! [`SidebarPanel`] builder. It is strictly additive: `sidebar()` and
//! the `SidebarItem`/`SidebarSection` constructors keep their exact
//! prior behaviour, so existing consumers compile and render unchanged.
//!
//! ## Section scroll (no scrollbar)
//!
//! Panel chrome (collapse, [`SidebarPanel::header`], footer) and section
//! labels stay fixed. A section marked [`SidebarSection::fill`] owns the
//! remaining height; its **items** scroll with a hidden scrollbar.
//! Optional [`SidebarPanel::section_scroll`] drives top/bottom chips
//! (`↑ N …` / `↓ N …`) for items fully outside the viewport.
//!
//! Style fns read from `theme.extended_palette()` only — the kit's
//! atom→slot bindings live in [`crate::theme::build_theme`]. To
//! restyle the sidebar globally, edit that mapping; this file should
//! never see a raw `hex::*`.

use std::collections::HashMap;

use iced::widget::scrollable::{Direction, Scrollbar, Viewport};
use iced::widget::text::Wrapping;
use iced::widget::{
    Container, Space, button, column, container, float, mouse_area, row, scrollable, sensor, stack,
    text,
};
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::advanced::Renderer as _;
use iced::{
    Animation, Background, Border, Color, Element, Event, Length, Padding, Rectangle, Shadow, Size,
    Theme, Vector, animation::Easing, mouse, time::Instant, widget::float as float_widget,
};

use crate::components::icon::{icon_handle, icon_svg_colored};
use crate::components::style::{
    linear_bg, mix, mix_white, RADIUS_LG, RADIUS_MD, RADIUS_SM, SPACE_MD, SPACE_SM, SPACE_XS,
    alpha,
};
use crate::fonts;

/// Vertical padding for a standard sidebar row (top+bottom each).
const ITEM_PAD_V: f32 = 10.0;
/// Horizontal padding for a standard sidebar row.
const ITEM_PAD_H: f32 = 12.0;
/// Card chrome: roomier face pad (Overview rule-card density).
const CARD_PAD_V: f32 = 14.0;
const CARD_PAD_H: f32 = 14.0;
/// Default scroll-math height when chrome is [`SidebarItemChrome::Card`]
/// and the caller did not supply [`SidebarItem::height_hint`].
const CARD_HEIGHT_HINT: f32 = 78.0;
/// Gap between title and subtitle lines.
const TITLE_SUB_GAP: f32 = 5.0;

/// Visual chrome for a [`SidebarItem`].
///
/// [`Self::Row`] is the historical packed nav row. [`Self::Card`] is a
/// softer, roomier product surface (session switcher, mailbox cards) —
/// raised idle material, larger radius, more internal pad. Pair cards
/// with non-zero [`SidebarPanel::item_spacing`] (e.g. [`SPACE_MD`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarItemChrome {
    #[default]
    Row,
    Card,
}

/// Leading status mark for a sidebar row (activity / health, not selection).
///
/// Prefer always showing a mark so the title does not shift horizontally
/// when activity starts/stops. `Idle` is the reserved empty slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarIndicator {
    /// Turn in flight (tools, streaming). Ring, not a filled dot.
    Working,
    /// Needs a human (question, permission).
    Waiting,
    /// Turn finished.
    Done,
    /// Session is actively working (generic apps — streaming, recent writes).
    Active,
    /// Present but idle — dim placeholder so layout stays fixed.
    #[default]
    Idle,
}

/// Trailing control under [`SidebarItem::secondary`], shown only while the
/// row is hovered (requires [`SidebarPanel::item_hover`] + [`SidebarItem::id`]).
#[derive(Debug, Clone)]
pub struct SidebarHoverAction<Message> {
    pub message: Message,
    /// Second-step / armed styling (e.g. two-click delete confirm).
    pub armed: bool,
}

/// One row in the sidebar. `active` flips on the visual state; `message`
/// is what the parent receives when the row is clicked.
///
/// The `shortcut` / `on_close` / `secondary` / `subtitle` /
/// `on_double_click` / `indicator` / `hover_action` / `chrome` /
/// `content` fields are opt-in extras consumed by [`SidebarPanel`]
/// (and the shared row renderer used by plain [`sidebar`]). They default
/// to `None` / [`SidebarItemChrome::Row`], so existing `::new().active()`
/// callers behave exactly as before.
///
/// Set [`Self::content`] to supply a fully custom body (session cards,
/// rich mail rows). Outer press/selection/hover-trash chrome still wraps
/// it; label/subtitle/secondary are ignored when content is present
/// (keep a short `label` for collapsed/icon-only modes).
pub struct SidebarItem<'a, Message> {
    pub label: String,
    pub active: bool,
    pub message: Message,
    /// Right-aligned dim shortcut hint (e.g. the `1`..=`9` access key).
    pub shortcut: Option<u8>,
    /// When set, [`SidebarPanel`] renders a trailing `×` button that
    /// emits this message.
    pub on_close: Option<Message>,
    /// Dim trailing label (e.g. relative time `19m`, unread count).
    /// Laid out in a fixed trailing column so it cannot crush the title.
    pub secondary: Option<String>,
    /// Optional second line under the title (e.g. project path). Muted,
    /// mono-friendly size; wraps/clips independently of the title.
    pub subtitle: Option<String>,
    /// Double-click on the row (e.g. rename). Single click still emits
    /// [`Self::message`].
    pub on_double_click: Option<Message>,
    /// Optional leading status dot (activity), independent of selection.
    pub indicator: Option<SidebarIndicator>,
    /// Stable id for [`SidebarPanel::item_hover`] matching.
    pub id: Option<String>,
    /// Control under the secondary label; visible only while this row is
    /// the hovered item (see [`SidebarPanel::item_hover`]).
    pub hover_action: Option<SidebarHoverAction<Message>>,
    /// Row vs card materials / padding. Default is packed nav row.
    pub chrome: SidebarItemChrome,
    /// Custom body — replaces the default title/subtitle/secondary layout.
    pub content: Option<Element<'a, Message, Theme>>,
    /// Scroll-chip / overflow math when body height is not obvious from
    /// label+subtitle (required accuracy for tall custom cards).
    pub height_hint: Option<f32>,
}

impl<'a, Message> SidebarItem<'a, Message> {
    pub fn new(label: impl Into<String>, message: Message) -> Self {
        Self {
            label: label.into(),
            active: false,
            message,
            shortcut: None,
            on_close: None,
            secondary: None,
            subtitle: None,
            on_double_click: None,
            indicator: None,
            id: None,
            hover_action: None,
            chrome: SidebarItemChrome::Row,
            content: None,
            height_hint: None,
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Attach a right-aligned dim shortcut hint.
    pub fn shortcut(mut self, n: u8) -> Self {
        self.shortcut = Some(n);
        self
    }

    /// Attach a trailing `×` close button emitting `msg`.
    pub fn on_close(mut self, msg: Message) -> Self {
        self.on_close = Some(msg);
        self
    }

    /// Attach a dim trailing secondary label (time, count, …).
    pub fn secondary(mut self, label: impl Into<String>) -> Self {
        self.secondary = Some(label.into());
        self
    }

    /// Second line under the title (path, caption, …).
    pub fn subtitle(mut self, label: impl Into<String>) -> Self {
        self.subtitle = Some(label.into());
        self
    }

    /// Message emitted on double-click (rename, open properties, …).
    pub fn on_double_click(mut self, msg: Message) -> Self {
        self.on_double_click = Some(msg);
        self
    }

    /// Leading status dot (e.g. session actively working).
    pub fn indicator(mut self, indicator: SidebarIndicator) -> Self {
        self.indicator = Some(indicator);
        self
    }

    /// Stable id for hover tracking ([`SidebarPanel::item_hover`]).
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Trailing control under the secondary label (hover-only when the
    /// panel has item hover wired).
    pub fn hover_action(mut self, action: SidebarHoverAction<Message>) -> Self {
        self.hover_action = Some(action);
        self
    }

    /// Soft card chrome (more pad, raised idle material, larger radius).
    pub fn chrome(mut self, chrome: SidebarItemChrome) -> Self {
        self.chrome = chrome;
        self
    }

    /// Convenience: [`SidebarItemChrome::Card`].
    pub fn card(mut self) -> Self {
        self.chrome = SidebarItemChrome::Card;
        self
    }

    /// Replace the default title/subtitle body with a custom element.
    /// Outer selection / hover-trash chrome still applies.
    pub fn content(mut self, content: impl Into<Element<'a, Message, Theme>>) -> Self {
        self.content = Some(content.into());
        self
    }

    /// Intrinsic height hint for section scroll math (custom / card rows).
    pub fn height_hint(mut self, h: f32) -> Self {
        self.height_hint = Some(h.max(0.0));
        self
    }
}

/// A group of sidebar rows with an optional section header label.
/// Labels render in title case as provided (macOS sidebar group style —
/// not forced uppercase). Unlabeled sections render as a plain item
/// group (useful for a top "Welcome" entry above the first headed section).
///
/// Mark [`Self::fill`] so the section's **item body** (not the label)
/// takes remaining panel height and scrolls without a scrollbar. Wire
/// [`SidebarPanel::section_scroll`] for `↑ N …` / `↓ N …` overflow chips.
pub struct SidebarSection<'a, Message> {
    pub label: Option<String>,
    pub items: Vec<SidebarItem<'a, Message>>,
    /// When true, this section's item list fills remaining height and
    /// scrolls (hidden bar). At most one fill section is useful; if
    /// several are marked, the first wins the `Fill` slot and others
    /// still get a bounded scroll body.
    pub fill: bool,
}

impl<'a, Message> SidebarSection<'a, Message> {
    pub fn new(label: impl Into<String>, items: Vec<SidebarItem<'a, Message>>) -> Self {
        Self {
            label: Some(label.into()),
            items,
            fill: false,
        }
    }

    pub fn unlabeled(items: Vec<SidebarItem<'a, Message>>) -> Self {
        Self {
            label: None,
            items,
            fill: false,
        }
    }

    /// This section's item body fills remaining panel height and scrolls
    /// without a visible scrollbar. Pair with
    /// [`SidebarPanel::section_scroll`] for overflow chips.
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }
}

/// Live viewport snapshot for a fill section's scroll body.
///
/// Owned by the app; updated via [`SidebarPanel::section_scroll`]'s
/// `on_scroll` callback (fed from iced's `scrollable::on_scroll`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SectionScroll {
    /// Absolute Y content offset (px scrolled down from top).
    pub offset_y: f32,
    /// Visible viewport height (px).
    pub viewport_h: f32,
    /// Full content height (px).
    pub content_h: f32,
}

impl SectionScroll {
    /// Capture geometry from an iced [`Viewport`] (legacy scrollable path).
    pub fn from_viewport(vp: &Viewport) -> Self {
        Self {
            offset_y: vp.absolute_offset().y,
            viewport_h: vp.bounds().height,
            content_h: vp.content_bounds().height,
        }
    }

    /// True when content is taller than the viewport (with a 1px slack).
    pub fn overflows(&self) -> bool {
        self.viewport_h > 0.0 && self.content_h > self.viewport_h + 1.0
    }

    /// Maximum scroll offset (0 when content fits).
    pub fn max_offset(&self) -> f32 {
        (self.content_h - self.viewport_h).max(0.0)
    }

    /// Clamp [`Self::offset_y`] into the valid range.
    pub fn clamped(mut self) -> Self {
        let max = self.max_offset();
        self.offset_y = self.offset_y.clamp(0.0, max);
        self
    }

    /// Apply a mouse-wheel delta (same sign convention as iced's scrollable).
    pub fn wheel(mut self, delta: mouse::ScrollDelta) -> Self {
        let dy = match delta {
            mouse::ScrollDelta::Lines { y, .. } => -y * 60.0,
            mouse::ScrollDelta::Pixels { y, .. } => -y,
        };
        self.offset_y += dy;
        self.clamped()
    }

    /// Jump to the top of the list (`offset_y = 0`).
    pub fn jump_top(mut self) -> Self {
        self.offset_y = 0.0;
        self
    }

    /// Jump to the bottom of the list (`offset_y = max`).
    pub fn jump_bottom(mut self) -> Self {
        self.offset_y = self.max_offset();
        self
    }

    /// Update measured viewport height and re-clamp.
    pub fn with_viewport_h(mut self, viewport_h: f32) -> Self {
        self.viewport_h = viewport_h.max(0.0);
        self.clamped()
    }

    /// Update content height and re-clamp.
    pub fn with_content_h(mut self, content_h: f32) -> Self {
        self.content_h = content_h.max(0.0);
        self.clamped()
    }
}

/// Intrinsic height of one sidebar row (padding + text), excluding column gap.
fn item_row_height<Message>(item: &SidebarItem<'_, Message>) -> f32 {
    if let Some(h) = item.height_hint {
        return h;
    }
    if item.content.is_some() || item.chrome == SidebarItemChrome::Card {
        return CARD_HEIGHT_HINT;
    }
    let text_h = if item.subtitle.is_some() {
        14.0 + TITLE_SUB_GAP + 11.0
    } else {
        14.0
    };
    // Multi-line secondary (e.g. context KB + age) needs room in scroll math.
    let trail_h = item
        .secondary
        .as_ref()
        .map(|s| s.lines().filter(|l| !l.is_empty()).count().max(1) as f32 * 12.0)
        .unwrap_or(0.0);
    ITEM_PAD_V * 2.0 + text_h.max(trail_h)
}

/// Full scroll content height for a section body (padding + rows + gaps).
pub fn section_content_height<Message>(items: &[SidebarItem<'_, Message>]) -> f32 {
    section_content_height_with_spacing(items, 0.0)
}

/// Like [`section_content_height`], with explicit inter-row spacing.
pub fn section_content_height_with_spacing<Message>(
    items: &[SidebarItem<'_, Message>],
    item_spacing: f32,
) -> f32 {
    let pad_v = 8.0; // matches body column padding [4, 8]
    if items.is_empty() {
        return pad_v;
    }
    let rows: f32 = items.iter().map(item_row_height).sum();
    let gaps = item_spacing * items.len().saturating_sub(1) as f32;
    pad_v + rows + gaps
}

/// Count items fully above / fully below the viewport, assuming roughly
/// equal item heights (`content_h / n_items`).
///
/// Pure — unit-tested without an iced runtime. Returns `(above, below)`.
/// Partially visible items count as neither. At the true top/bottom of
/// the scroll range both sides are forced to 0 so chips don't flash on
/// float noise.
pub fn section_overflow_counts(scroll: SectionScroll, n_items: usize) -> (usize, usize) {
    if n_items == 0 || !scroll.overflows() {
        return (0, 0);
    }
    let avg = scroll.content_h / n_items as f32;
    if avg <= 0.0 {
        return (0, 0);
    }
    let max_off = scroll.max_offset();
    // Treat the first/last few px as "parked" so chips hide at rest.
    const EDGE: f32 = 2.0;
    if scroll.offset_y <= EDGE {
        // At top: nothing above; count only what's fully below.
        let first_below =
            ((scroll.offset_y + scroll.viewport_h) / avg).ceil().max(0.0) as usize;
        let below = n_items.saturating_sub(first_below.min(n_items));
        return (0, below);
    }
    if scroll.offset_y >= max_off - EDGE {
        let above = (scroll.offset_y / avg).floor().max(0.0) as usize;
        return (above.min(n_items), 0);
    }
    // Item i occupies [i*avg, (i+1)*avg). Fully above when end ≤ offset.
    let above = (scroll.offset_y / avg).floor().max(0.0) as usize;
    // Fully below when start ≥ offset + viewport.
    let first_below =
        ((scroll.offset_y + scroll.viewport_h) / avg).ceil().max(0.0) as usize;
    let above = above.min(n_items);
    let below = n_items.saturating_sub(first_below.min(n_items));
    (above, below)
}

/// Default sidebar width — matches the storybook's nav column. Public
/// so consumers can lay out alongside it (`width = Fill - SIDEBAR_WIDTH`).
pub const SIDEBAR_WIDTH: f32 = 220.0;

/// Build the sidebar panel from its sections. Returns a `Container` so
/// callers can override the kit defaults (a fixed [`SIDEBAR_WIDTH`] wide,
/// full height) by chaining `.width(..)` / `.height(..)` before dropping
/// it into a layout.
pub fn sidebar<'a, Message>(
    sections: Vec<SidebarSection<'a, Message>>,
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
    sections: Vec<SidebarSection<'a, Message>>,
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
            col = col.push(render_item(item, None, 0, false));
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
/// Vertical pitch of one panel row — used when siblings slide to open a
/// drop slot. Matches packed item spacing (no inter-row gap).
pub const PANEL_ROW_STRIDE: f32 = PANEL_ROW_H;

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
    item: SidebarItem<'a, Message>,
    reorder: Option<&ReorderCfg<'a, Message>>,
    index: usize,
    show_hover_action: bool,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let SidebarItem {
        label,
        active,
        message,
        shortcut,
        on_close,
        secondary,
        subtitle,
        on_double_click,
        indicator,
        id: _,
        hover_action,
        chrome,
        content: custom,
        height_hint: _,
    } = item;

    // Selection is background-only (see `item_style`) — no left accent bar,
    // so title/subtitle stay aligned with idle rows.
    // Custom card bodies own their own padding (session tabs inset a
    // bottom context bar); structured card rows keep kit pad.
    let (pad_v, pad_h) = match (chrome, custom.is_some()) {
        (SidebarItemChrome::Row, _) => (ITEM_PAD_V, ITEM_PAD_H),
        (SidebarItemChrome::Card, true) => (0.0, 0.0),
        (SidebarItemChrome::Card, false) => (CARD_PAD_V, CARD_PAD_H),
    };
    let pad = Padding::from([pad_v, pad_h]);
    let hovered = show_hover_action;

    // Inline hover action only on the reorder + structured path; the plain
    // path overlays trash via stack so layout never shifts.
    let inline_hover = if reorder.is_some() && show_hover_action && custom.is_none() {
        hover_action.clone()
    } else {
        None
    };
    let body: Element<'a, Message> = if let Some(custom) = custom {
        custom
    } else {
        item_content(
            &label,
            subtitle.as_deref(),
            secondary.as_deref(),
            shortcut,
            indicator,
            inline_hover,
        )
    };

    // ── Plain path (no reorder). ──
    let Some(reorder) = reorder else {
        // Hover-action rows: full padded row is the select target (pad is
        // inside the mouse_area so inter-row space is clickable). Trash
        // overlays bottom-right (under the age label) via `stack` — same
        // pattern as [`vertical_tabs`] — so showing it never steals width
        // from the age label or shifts layout (which also broke hover
        // enter/exit when moving across rows).
        let row_el: Element<'a, Message> = if hover_action.is_some() {
            let mut select = mouse_area(
                container(body)
                    .width(Length::Fill)
                    .padding(pad)
                    .style(move |theme: &Theme| {
                        row_container_style(theme, active, chrome, hovered)
                    }),
            )
            .interaction(mouse::Interaction::Pointer)
            .on_press(message);
            if let Some(dbl) = on_double_click {
                select = select.on_double_click(dbl);
            }
            if show_hover_action {
                if let Some(action) = hover_action {
                    // Float trash over the trailing age corner; stack sizes
                    // to the base row so nothing reflows.
                    let trash = container(hover_action_button(action))
                        .align_x(iced::alignment::Horizontal::Right)
                        .align_y(iced::alignment::Vertical::Bottom)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .padding(Padding {
                            top: 0.0,
                            right: (pad_h - 2.0).max(0.0),
                            bottom: (pad_v - 2.0).max(0.0),
                            left: 0.0,
                        });
                    stack![select, trash].into()
                } else {
                    select.into()
                }
            } else {
                select.into()
            }
        } else if let Some(dbl) = on_double_click {
            mouse_area(
                container(body)
                    .width(Length::Fill)
                    .padding(pad)
                    .style(move |theme: &Theme| {
                        row_container_style(theme, active, chrome, false)
                    }),
            )
            .interaction(mouse::Interaction::Pointer)
            .on_press(message)
            .on_double_click(dbl)
            .into()
        } else {
            button(body)
                .style(move |t, status| item_style_chrome(t, status, active, chrome))
                .padding(pad)
                .width(Length::Fill)
                .on_press(message)
                .into()
        };
        if let Some(close_msg) = on_close {
            return row![row_el, close_button(close_msg)]
                .spacing(SPACE_XS)
                .align_y(iced::Alignment::Center)
                .into();
        }
        return row_el;
    };

    // ── Reorder-enabled path. ──
    // Live-reorder chrome is active only while `reorder.active` is `Some` —
    // consumers populate that after the movement threshold so a plain press
    // leaves the strip alone. While active, [`SidebarPanel::build`] lifts
    // the grabbed row under the cursor and glides siblings into the gap.
    // `index` is the item's *stable* (pre-drag) index in the consumer's
    // order — used for press messages and for the grabbing cursor.
    let is_dragged = matches!(reorder.active, Some((from, _)) if from == index);

    let mut pressable = mouse_area(
        container(body)
            .width(Length::Fill)
            .padding(pad)
            .style(move |theme: &Theme| {
                row_container_style(theme, active, chrome, hovered)
            }),
    )
    // Pointer at rest; grabbing while this row is the one in flight.
    .interaction(if is_dragged {
        mouse::Interaction::Grabbing
    } else {
        mouse::Interaction::Pointer
    })
    .on_press((reorder.on_press)(index));
    if let Some(dbl) = on_double_click {
        pressable = pressable.on_double_click(dbl);
    }

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

/// Title + optional subtitle (+ leading indicator). Takes `Fill` width.
///
/// Primary line is the identity row (ui, slightly larger). Subtitle is quieter
/// secondary caption (ui, not mono — session titles / paths both read better).
fn item_text_block<'a, Message: 'a>(
    label: &str,
    subtitle: Option<&str>,
    indicator: Option<SidebarIndicator>,
) -> Element<'a, Message> {
    let title = text(label.to_string())
        .font(fonts::ui())
        .size(14)
        .wrapping(Wrapping::None)
        .width(Length::Fill);

    let mut title_row = row![].spacing(SPACE_SM).align_y(iced::Alignment::Center);
    if let Some(ind) = indicator {
        title_row = title_row.push(status_dot(ind));
    }
    title_row = title_row.push(title);

    let mut text_col = column![title_row]
        .spacing(TITLE_SUB_GAP)
        .width(Length::Fill);
    if let Some(sub) = subtitle {
        // Indent subtitle under the title text when a leading dot is present.
        let sub_pad = if indicator.is_some() { 14.0 } else { 0.0 };
        text_col = text_col.push(
            container(
                text(sub.to_string())
                    .font(fonts::ui())
                    .size(11)
                    .style(|theme: &Theme| {
                        let c = theme.extended_palette().background.base.text;
                        iced::widget::text::Style {
                            color: Some(Color { a: 0.42, ..c }),
                        }
                    })
                    .wrapping(Wrapping::None)
                    .width(Length::Fill),
            )
            .padding(Padding {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: sub_pad,
            })
            .width(Length::Fill),
        );
    }

    container(text_col).width(Length::Fill).clip(true).into()
}

/// Shrink trailing column: age/shortcut (multi-line ok), then optional hover action.
fn item_trailing<'a, Message: Clone + 'a>(
    secondary: Option<&str>,
    shortcut: Option<u8>,
    hover_action: Option<SidebarHoverAction<Message>>,
) -> Element<'a, Message> {
    let mut trailing = column![].spacing(2.0).align_x(iced::Alignment::End);
    if let Some(sec) = secondary {
        // Allow "42k/500k\\n12m" style badges (context + age).
        for line in sec.lines().filter(|l| !l.is_empty()) {
            trailing = trailing.push(dim_label(line));
        }
    }
    if let Some(n) = shortcut {
        trailing = trailing.push(dim_label(&n.to_string()));
    }
    if let Some(action) = hover_action {
        trailing = trailing.push(hover_action_button(action));
    }
    container(trailing)
        .width(Length::Shrink)
        .padding(Padding {
            top: 1.0,
            right: 0.0,
            bottom: 0.0,
            left: 6.0,
        })
        .into()
}

/// Title (+ optional subtitle) with trailing secondary / shortcut /
/// hover action.
///
/// Layout:
/// ```text
/// [ ● title …………………  19m ]
/// [   subtitle ……………  🗑  ]   ← hover action under age when shown
/// ```
fn item_content<'a, Message: Clone + 'a>(
    label: &str,
    subtitle: Option<&str>,
    secondary: Option<&str>,
    shortcut: Option<u8>,
    indicator: Option<SidebarIndicator>,
    hover_action: Option<SidebarHoverAction<Message>>,
) -> Element<'a, Message> {
    let text_box = item_text_block(label, subtitle, indicator);
    let has_trail =
        secondary.is_some() || shortcut.is_some() || hover_action.is_some();
    let mut r = row![text_box]
        .spacing(SPACE_MD)
        .align_y(iced::Alignment::Start)
        .width(Length::Fill);
    if has_trail {
        r = r.push(item_trailing(secondary, shortcut, hover_action));
    }
    r.into()
}

/// Trash (or armed delete) control under the secondary time label.
fn hover_action_button<'a, Message: Clone + 'a>(
    action: SidebarHoverAction<Message>,
) -> Element<'a, Message> {
    let armed = action.armed;
    let handle = icon_handle("lucide/trash-2");
    let color = if armed {
        Color {
            r: 0.92,
            g: 0.32,
            b: 0.32,
            a: 1.0,
        }
    } else {
        Color {
            r: 0.55,
            g: 0.58,
            b: 0.64,
            a: 0.90,
        }
    };
    let glyph = icon_svg_colored(handle, 12, color);
    button(glyph)
        .padding(Padding::from([2, 4]))
        .style(move |theme: &Theme, status| {
            let p = theme.extended_palette();
            let bg = match status {
                button::Status::Hovered if armed => Color {
                    a: 0.22,
                    ..p.danger.base.color
                },
                button::Status::Hovered => Color {
                    a: 0.14,
                    ..p.background.stronger.color
                },
                button::Status::Pressed => Color {
                    a: 0.28,
                    ..if armed {
                        p.danger.base.color
                    } else {
                        p.background.stronger.color
                    }
                },
                _ => Color::TRANSPARENT,
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    radius: RADIUS_SM.into(),
                    ..Default::default()
                },
                text_color: color,
                ..button::Style::default()
            }
        })
        .on_press(action.message)
        .into()
}

fn status_dot<'a, Message: 'a>(indicator: SidebarIndicator) -> Element<'a, Message> {
    let (fill, stroke, stroke_w) = match indicator {
        SidebarIndicator::Working => (
            Color::TRANSPARENT,
            Color {
                r: 0.92,
                g: 0.72,
                b: 0.18,
                a: 1.0,
            },
            1.5,
        ),
        SidebarIndicator::Waiting => (
            Color {
                r: 0.92,
                g: 0.62,
                b: 0.18,
                a: 1.0,
            },
            Color::TRANSPARENT,
            0.0,
        ),
        SidebarIndicator::Done | SidebarIndicator::Active => (
            Color {
                r: 0.24,
                g: 0.81,
                b: 0.56,
                a: 1.0,
            },
            Color::TRANSPARENT,
            0.0,
        ),
        // Quiet placeholder — visible enough to reserve space, not attention.
        SidebarIndicator::Idle => (
            Color {
                r: 0.45,
                g: 0.48,
                b: 0.55,
                a: 0.55,
            },
            Color::TRANSPARENT,
            0.0,
        ),
    };
    container(Space::new().width(6.0).height(6.0))
        .width(Length::Fixed(6.0))
        .height(Length::Fixed(6.0))
        .style(move |_t: &Theme| container::Style {
            background: Some(Background::Color(fill)),
            border: Border {
                radius: 999.0.into(),
                width: stroke_w,
                color: stroke,
            },
            ..container::Style::default()
        })
        .into()
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
/// per-item shortcut hints / close buttons / secondary labels, section-
/// scoped scroll with overflow chips, plus an optional footer.
///
/// The APP owns the cursor-move/release subscription (mirroring
/// sola-monitor's `DividerPress` + global listener pattern); this builder
/// only renders the divider `mouse_area` and the full-window overlay that
/// keeps the resize cursor while `dragging`. Returns an `Element` (a
/// `row!`/`stack!`), not a `Container`, so it composes directly.
pub struct SidebarPanel<'a, Message> {
    sections: Vec<SidebarSection<'a, Message>>,
    collapse: Option<(bool, Message)>,
    /// `(width, dragging, on_press, colors)` — `colors` is `None` for
    /// theme-default divider chrome.
    resize: Option<(f32, bool, Message, Option<crate::components::DividerColors>)>,
    reorder: Option<ReorderCfg<'a, Message>>,
    /// Optional leading content (search field, brand, rename bar).
    /// Stacked above the section list; never scrolls with items.
    header: Option<Element<'a, Message, Theme>>,
    footer: Option<Element<'a, Message, Theme>>,
    /// Viewport snapshot + callback for the fill section's scroll body.
    /// When set, fill sections show `↑ N …` / `↓ N …` overflow chips.
    section_scroll: Option<(SectionScroll, Box<dyn Fn(SectionScroll) -> Message + 'a>)>,
    /// Per-row hover id + callback (for hover-only trailing actions).
    item_hover: Option<(Option<String>, Box<dyn Fn(Option<String>) -> Message + 'a>)>,
    /// Vertical gap between item rows in a section body. Default `0` so the
    /// full band between labels is clickable (nav lists). Pass e.g.
    /// [`SPACE_MD`] for card stacks.
    item_spacing: f32,
}

impl<'a, Message> SidebarPanel<'a, Message>
where
    Message: Clone + 'a,
{
    pub fn new(sections: Vec<SidebarSection<'a, Message>>) -> Self {
        Self {
            sections,
            collapse: None,
            resize: None,
            reorder: None,
            header: None,
            footer: None,
            section_scroll: None,
            item_hover: None,
            item_spacing: 0.0,
        }
    }

    /// Space between consecutive item rows (`0` = packed / fully clickable).
    pub fn item_spacing(mut self, spacing: f32) -> Self {
        self.item_spacing = spacing.max(0.0);
        self
    }

    /// Leading chrome above the section list (filter, brand, …).
    pub fn header(mut self, el: Element<'a, Message, Theme>) -> Self {
        self.header = Some(el);
        self
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

    /// Drive overflow chips on the fill section's scroll body.
    ///
    /// `scroll` is the latest [`SectionScroll`] snapshot (app-owned);
    /// `on_scroll` updates it from iced's viewport callback. Without this,
    /// fill sections still scroll with a hidden bar but never show chips.
    pub fn section_scroll(
        mut self,
        scroll: SectionScroll,
        on_scroll: impl Fn(SectionScroll) -> Message + 'a,
    ) -> Self {
        self.section_scroll = Some((scroll, Box::new(on_scroll)));
        self
    }

    /// Track which item id is hovered so [`SidebarItem::hover_action`] can
    /// appear only on that row. `hovered` is the app-owned id (or `None`);
    /// `on_hover` receives `Some(id)` when the pointer enters a row and
    /// `None` when it leaves the item list entirely.
    ///
    /// Rows only emit **enter** (not exit). A list-level exit clears hover.
    /// Per-row exit races with the next row's enter (order depends on move
    /// direction) and left trash stuck off when sweeping upward.
    pub fn item_hover(
        mut self,
        hovered: Option<String>,
        on_hover: impl Fn(Option<String>) -> Message + 'a,
    ) -> Self {
        self.item_hover = Some((hovered, Box::new(on_hover)));
        self
    }

    pub fn build(self) -> Element<'a, Message, Theme> {
        let SidebarPanel {
            sections,
            collapse,
            resize,
            reorder,
            header,
            footer,
            section_scroll,
            item_hover,
            item_spacing,
        } = self;

        let collapsed = collapse.as_ref().map(|(c, _)| *c).unwrap_or(false);
        let reorder_ref = reorder.as_ref();
        let (scroll_snap, mut on_section_scroll) = match section_scroll {
            Some((snap, cb)) => (snap, Some(cb)),
            None => (SectionScroll::default(), None),
        };
        let (hovered_id, mut on_item_hover) = match item_hover {
            Some((id, cb)) => (id, Some(cb)),
            None => (None, None),
        };

        // Fixed chrome (collapse + header + footer). Section *labels* also
        // stay outside the scroll body; only item lists scroll.
        let mut chrome = column![].spacing(0.0).width(Length::Fill).height(Length::Fill);

        // Toggle header.
        if let Some((_, on_toggle)) = &collapse {
            let glyph = if collapsed { "»" } else { "«" };
            chrome = chrome.push(
                button(text(glyph).font(fonts::ui()).size(13))
                    .style(|t, status| item_style(t, status, false))
                    .padding(Padding::from([6, 10]))
                    .width(Length::Fill)
                    .on_press(on_toggle.clone()),
            );
        }

        if let Some(header) = header {
            if !collapsed {
                chrome = chrome.push(
                    container(header).padding(Padding {
                        top: 10.0,
                        right: 10.0,
                        bottom: 8.0,
                        left: 10.0,
                    }),
                );
            }
        }

        // Total item count across all sections — clamps the drop slot.
        let total_items: usize = sections.iter().map(|s| s.items.len()).sum();
        let n_sections = sections.len();
        let any_explicit_fill = sections.iter().any(|s| s.fill);
        // Auto-fill a lone section so a single long list scrolls without
        // the caller remembering `.fill()`. Multiple sections require an
        // explicit mark so short groups don't steal the Fill slot.
        let auto_fill_single = !any_explicit_fill && n_sections == 1;
        let dragging = reorder_ref.and_then(|r| r.active);

        let sections_el: Element<'a, Message, Theme> = if let Some((from, start_y)) = dragging {
            // Live reorder preview: flatten rows (headers omitted). Scroll
            // body is still bar-less so the gesture matches the rest state.
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
            let mut flat: Vec<(usize, SidebarItem<'a, Message>)> = Vec::with_capacity(total_items);
            let mut row_index = 0usize;
            for section in sections {
                for item in section.items {
                    flat.push((row_index, item));
                    row_index += 1;
                }
            }
            let mut items = column![]
                .spacing(item_spacing)
                .padding(Padding::from([4.0, 8.0]));
            for (stable_index, item) in flat {
                let is_dragged = stable_index == from;
                let dy = if is_dragged {
                    cursor_y - start_y
                } else if let Some(anim) = anim {
                    anim.offset(stable_index, now)
                } else {
                    panel_sibling_offset(from, to, stable_index)
                };
                let show_action = item
                    .id
                    .as_ref()
                    .is_some_and(|id| hovered_id.as_ref() == Some(id));
                let row_el = if collapsed {
                    collapsed_row(&item, stable_index, reorder_ref)
                } else {
                    render_item(item, reorder_ref, stable_index, show_action)
                };
                items = items.push(with_reorder_motion(row_el, dy, is_dragged));
            }
            hidden_scroll(items, None, None).into()
        } else {
            // At rest: section labels sticky; item bodies scroll per-fill.
            let mut sections_col = column![].spacing(0.0).width(Length::Fill).height(Length::Fill);
            let mut row_index = 0usize;
            let mut assigned_fill = false;

            for (si, section) in sections.into_iter().enumerate() {
                if si > 0 && !collapsed {
                    sections_col =
                        sections_col.push(Space::new().height(Length::Fixed(12.0)));
                }

                let n_in_section = section.items.len();
                let content_h =
                    section_content_height_with_spacing(&section.items, item_spacing);
                let wants_fill = !collapsed
                    && (section.fill || auto_fill_single)
                    && !assigned_fill;
                if wants_fill {
                    assigned_fill = true;
                }

                if let Some(label) = section.label {
                    if !collapsed {
                        // Sticky: outside the section's scroll body.
                        sections_col = sections_col.push(section_header(label));
                    }
                }

                let mut body_items = column![]
                    .spacing(item_spacing)
                    .padding(Padding::from([4.0, 8.0]));
                for item in section.items {
                    if collapsed {
                        body_items =
                            body_items.push(collapsed_row(&item, row_index, reorder_ref));
                    } else {
                        let item_id = item.id.clone();
                        let show_action = item_id
                            .as_ref()
                            .is_some_and(|id| hovered_id.as_ref() == Some(id));
                        let mut row_el =
                            render_item(item, reorder_ref, row_index, show_action);
                        // Enter only — list-level exit clears hover so A→B
                        // cannot race (exit A after enter B → stuck None).
                        if let (Some(id), Some(ref mut on_hover)) =
                            (item_id, on_item_hover.as_mut())
                        {
                            row_el = mouse_area(row_el).on_enter(on_hover(Some(id))).into();
                        }
                        body_items = body_items.push(row_el);
                    }
                    row_index += 1;
                }

                if wants_fill {
                    // First fill section owns app-driven scroll + chips.
                    let scroll_cb = on_section_scroll.take();
                    let mut body = fill_section_body(
                        body_items,
                        n_in_section,
                        content_h,
                        scroll_snap,
                        scroll_cb,
                    );
                    if let Some(ref mut on_hover) = on_item_hover {
                        body = mouse_area(body).on_exit(on_hover(None)).into();
                    }
                    sections_col = sections_col.push(body);
                } else {
                    let body: Element<'a, Message> =
                        if let Some(ref mut on_hover) = on_item_hover {
                            mouse_area(body_items)
                                .on_exit(on_hover(None))
                                .into()
                        } else {
                            body_items.into()
                        };
                    sections_col = sections_col.push(body);
                }
            }

            if !assigned_fill && !collapsed {
                // No fill section: keep the whole stack scrollable (hidden
                // bar) so multi-section panels still work when content is
                // tall — labels scroll with items (legacy fallback).
                hidden_scroll(sections_col, None, None).into()
            } else {
                sections_col.into()
            }
        };

        chrome = chrome.push(sections_el);

        // Footer (hidden when collapsed).
        if let Some(footer) = footer {
            if !collapsed {
                chrome = chrome.push(
                    container(footer).padding(Padding::from([8.0, 10.0])),
                );
            }
        }

        let width = match &resize {
            Some((w, _, _, _)) if !collapsed => *w,
            _ if collapsed => 36.0,
            _ => SIDEBAR_WIDTH,
        };

        let panel = container(chrome)
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

/// Scrollable with a zero-width vertical rail — used only for reorder
/// preview / multi-section fallback (not the fill-section path).
fn hidden_scroll<'a, Message: 'a>(
    content: impl Into<Element<'a, Message, Theme>>,
    on_scroll: Option<Box<dyn Fn(SectionScroll) -> Message + 'a>>,
    id: Option<iced::widget::Id>,
) -> scrollable::Scrollable<'a, Message, Theme> {
    let mut s = scrollable(content.into())
        .direction(Direction::Vertical(Scrollbar::hidden()))
        .height(Length::Fill)
        .width(Length::Fill);
    if let Some(id) = id {
        s = s.id(id);
    }
    if let Some(cb) = on_scroll {
        s = s.on_scroll(move |vp| cb(SectionScroll::from_viewport(&vp)));
    }
    s
}

/// Height of a visible overflow chip (`↑ N …` / `↓ N …`). Hidden when N=0.
const OVERFLOW_CHIP_H: f32 = 22.0;

/// Fill section: **app-owned** scroll (no iced `scrollable`).
///
/// Wheel updates [`SectionScroll`]; [`ClipScroll`] lays out *all* rows with
/// an unbounded height (stock containers clamp to the viewport, so only a
/// screenful of rows existed and the visible set changed as you scrolled).
fn fill_section_body<'a, Message: Clone + 'a>(
    items: iced::widget::Column<'a, Message, Theme>,
    n_items: usize,
    content_h: f32,
    scroll: SectionScroll,
    on_scroll: Option<Box<dyn Fn(SectionScroll) -> Message + 'a>>,
) -> Element<'a, Message, Theme> {
    // Prefer measured content_h; keep any larger viewport hint from sensor.
    let mut scroll = scroll.with_content_h(content_h);
    // Until sensor reports a real viewport, assume a tall pane so chips can
    // still show "below" when the list is long.
    if scroll.viewport_h <= 1.0 {
        scroll.viewport_h = 480.0;
    }
    scroll = scroll.clamped();

    let (above, below) = section_overflow_counts(scroll, n_items);
    let offset = scroll.offset_y;

    // Unbounded content layout + clip + translate (see [`ClipScroll`]).
    let clipped = ClipScroll {
        content: items.into(),
        offset_y: offset,
    };

    let (list, on_jump): (Element<'a, Message, Theme>, Option<std::rc::Rc<dyn Fn(SectionScroll) -> Message + 'a>>) =
        if let Some(cb) = on_scroll {
            let cb: std::rc::Rc<dyn Fn(SectionScroll) -> Message + 'a> = std::rc::Rc::from(cb);
            let base = scroll;

            let cb_wheel = std::rc::Rc::clone(&cb);
            let area = mouse_area(clipped).on_scroll(move |delta: mouse::ScrollDelta| {
                cb_wheel(base.wheel(delta))
            });

            let cb_show = std::rc::Rc::clone(&cb);
            let cb_resize = std::rc::Rc::clone(&cb);
            let list = sensor(area)
                .on_show(move |size: iced::Size| cb_show(base.with_viewport_h(size.height)))
                .on_resize(move |size: iced::Size| {
                    cb_resize(base.with_viewport_h(size.height))
                })
                .into();
            (list, Some(cb))
        } else {
            (clipped.into(), None)
        };

    // Chips only take space when there is overflow on that side — no
    // permanent gap under the section title at rest. Click jumps to end.
    let top_chip = overflow_slot(
        OverflowDir::Up,
        above,
        on_jump.as_ref().map(|cb| cb(scroll.jump_top())),
    );
    let bottom_chip = overflow_slot(
        OverflowDir::Down,
        below,
        on_jump.as_ref().map(|cb| cb(scroll.jump_bottom())),
    );

    column![top_chip, list, bottom_chip]
        .spacing(0.0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ──────────────────────────── ClipScroll widget ─────────────────────────────
//
// Viewport-sized host that lays out its child with **infinite max height**
// (so every row gets a layout node), then draws with a Y translation and a
// scissor clip. Offset is controlled by the parent — no internal scroll
// state, so app rebuilds cannot remount/reset it.

struct ClipScroll<'a, Message> {
    content: Element<'a, Message, Theme>,
    offset_y: f32,
}

impl<'a, Message> ClipScroll<'a, Message> {
    fn offset(&self) -> Vector {
        Vector::new(0.0, self.offset_y)
    }
}

impl<Message> Widget<Message, Theme, iced::Renderer> for ClipScroll<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(Length::Fill).height(Length::Fill);
        let size = limits.resolve(Length::Fill, Length::Fill, Size::ZERO);

        // Infinite max height — same trick iced's scrollable uses so the
        // full item column lays out, not just a viewport-sized slice.
        let child_limits =
            layout::Limits::new(Size::new(0.0, 0.0), Size::new(size.width, f32::INFINITY));

        let content =
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &child_limits);

        layout::Node::with_children(size, vec![content])
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let content_layout = layout.children().next().expect("clip-scroll content");
        let translation = self.offset();

        let cursor = match cursor.position_over(bounds) {
            Some(pos) => mouse::Cursor::Available(pos + translation),
            None => cursor,
        };

        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            content_layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &Rectangle {
                y: bounds.y + translation.y,
                x: bounds.x + translation.x,
                ..bounds
            },
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        let content_layout = layout.children().next().expect("clip-scroll content");
        let translation = self.offset();

        let cursor = match cursor.position_over(bounds) {
            Some(pos) => mouse::Cursor::Available(pos + translation),
            None => cursor,
        };

        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            content_layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(visible) = bounds.intersection(viewport) else {
            return;
        };
        let content_layout = layout.children().next().expect("clip-scroll content");
        let translation = self.offset();

        let cursor = match cursor.position_over(bounds) {
            Some(pos) => mouse::Cursor::Available(pos + translation),
            None => mouse::Cursor::Unavailable,
        };

        renderer.with_layer(visible, |renderer| {
            renderer.with_translation(Vector::new(0.0, -translation.y), |renderer| {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    content_layout,
                    cursor,
                    &Rectangle {
                        y: visible.y + translation.y,
                        x: visible.x + translation.x,
                        ..visible
                    },
                );
            });
        });
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let content_layout = layout.children().next().expect("clip-scroll content");
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            content_layout,
            renderer,
            operation,
        );
    }
}

impl<'a, Message: 'a> From<ClipScroll<'a, Message>> for Element<'a, Message, Theme> {
    fn from(value: ClipScroll<'a, Message>) -> Self {
        Element::new(value)
    }
}

enum OverflowDir {
    Up,
    Down,
}

/// Overflow chip. When `n == 0` collapses to zero height (no gap under the
/// section title at rest). When present, click emits `on_jump` (scroll to
/// top/bottom via the same [`SectionScroll`] callback as the wheel).
///
/// Note: do **not** use `center_y(Length::Fill)` — that sets height to Fill
/// and the three-row column (chip / list / chip) splits ⅓ each.
fn overflow_slot<'a, Message: Clone + 'a>(
    dir: OverflowDir,
    n: usize,
    on_jump: Option<Message>,
) -> Element<'a, Message, Theme> {
    if n == 0 {
        return Space::new()
            .width(Length::Fill)
            .height(Length::Fixed(0.0))
            .into();
    }
    let glyph = match dir {
        OverflowDir::Up => "↑",
        OverflowDir::Down => "↓",
    };
    let label = text(format!("{glyph}  {n} …"))
        .font(fonts::ui())
        .size(11)
        .style(|theme: &Theme| {
            let c = theme.extended_palette().background.base.text;
            iced::widget::text::Style {
                color: Some(Color { a: 0.55, ..c }),
            }
        });
    let chip = container(label)
        .width(Length::Fill)
        .height(Length::Fixed(OVERFLOW_CHIP_H))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center);

    match on_jump {
        Some(msg) => mouse_area(chip)
            .interaction(mouse::Interaction::Pointer)
            .on_press(msg)
            .into(),
        None => chip.into(),
    }
}

/// Collapsed (icon-only) row: shows just the shortcut number (or
/// `index + 1`), pressable via the same reorder/select mouse_area when
/// reorder is enabled, else a plain button.
fn collapsed_row<'a, Message>(
    item: &SidebarItem<'_, Message>,
    index: usize,
    reorder: Option<&ReorderCfg<'a, Message>>,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let number = item.shortcut.unwrap_or((index + 1) as u8);
    let active = item.active;
    let chrome = item.chrome;
    match reorder {
        Some(cfg) => mouse_area(
            container(collapsed_content::<Message>(number))
                .width(Length::Fill)
                .padding(Padding::from([6, 4]))
                .style(move |theme: &Theme| {
                    row_container_style(theme, active, chrome, false)
                }),
        )
        .on_press((cfg.on_press)(index))
        .into(),
        None => button(collapsed_content::<Message>(number))
            .style(move |t, status| item_style_chrome(t, status, active, chrome))
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
/// (the reorder / hover-action path).
///
/// - **Row:** selected → quiet [`crate::theme::selection`]; idle flat / hover lift.
/// - **Card:** OD session-tab graphite (not selection teal). Idle raised
///   wash + hairline; active gradient + stronger border. Same box either way.
/// Mid-drag lift (scale + shadow) is applied by [`with_reorder_motion`].
fn row_container_style(
    theme: &Theme,
    active: bool,
    chrome: SidebarItemChrome,
    hovered: bool,
) -> container::Style {
    let p = theme.extended_palette();
    match chrome {
        SidebarItemChrome::Row => {
            let bg = if active {
                Some(Background::Color(crate::theme::selection()))
            } else if hovered {
                Some(Background::Color(alpha(p.background.strong.color, 0.70)))
            } else {
                None
            };
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
        SidebarItemChrome::Card => card_surface_style(theme, active, hovered),
    }
}

/// OD `sola-agent-ds` session tab surface — idle/active share dimensions;
/// selection is graphite surface only (never the kit selection atom).
fn card_surface_style(theme: &Theme, active: bool, hovered: bool) -> container::Style {
    let p = theme.extended_palette();
    let raised = p.background.weaker.color;
    let base = p.background.base.color;
    let hover = p.background.strong.color; // ~bg-hover
    let border_atom = p.background.stronger.color;

    if active {
        // --tab-active-bg: gradient hover@92%+raised → raised@88%+#0a0c10
        let top = mix(hover, raised, 0.92);
        let bottom = mix(raised, Color::from_rgb(0.039, 0.047, 0.063), 0.88);
        container::Style {
            background: Some(linear_bg(180.0, &[(0.0, top), (1.0, bottom)])),
            text_color: Some(p.background.base.text),
            border: Border {
                // --tab-active-border: white@12% into border
                color: mix(mix_white(border_atom, 0.12), border_atom, 0.55),
                width: 1.0,
                radius: RADIUS_LG.into(),
            },
            // inset top hairline approximated as a light top edge via shadow
            shadow: Shadow {
                color: Color::from_rgba(1.0, 1.0, 1.0, 0.04),
                offset: Vector::new(0.0, 1.0),
                blur_radius: 0.0,
            },
            ..container::Style::default()
        }
    } else {
        // --tab-idle-bg: raised@42% over canvas; hover lifts toward bg-hover.
        let idle = mix(raised, base, 0.42);
        let fill = if hovered {
            mix(hover, idle, 0.55)
        } else {
            idle
        };
        container::Style {
            background: Some(Background::Color(fill)),
            text_color: Some(p.background.base.text),
            border: Border {
                // --tab-idle-border: white@5%
                color: mix_white(fill, 0.05),
                width: 1.0,
                radius: RADIUS_LG.into(),
            },
            ..container::Style::default()
        }
    }
}

/// Style fn for an individual sidebar row. Exposed so consumers
/// building custom row widgets (e.g. with leading icons) can match the
/// kit's visual language.
///
/// Active = quiet selection wash + rounded corners only. No left accent
/// bar (that shifted title/subtitle relative to idle rows).
pub fn item_style(theme: &Theme, status: button::Status, active: bool) -> button::Style {
    item_style_chrome(theme, status, active, SidebarItemChrome::Row)
}

/// Like [`item_style`], with explicit [`SidebarItemChrome`].
pub fn item_style_chrome(
    theme: &Theme,
    status: button::Status,
    active: bool,
    chrome: SidebarItemChrome,
) -> button::Style {
    let p = theme.extended_palette();
    match chrome {
        SidebarItemChrome::Row => {
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
        SidebarItemChrome::Card => {
            let hovered = matches!(
                status,
                button::Status::Hovered | button::Status::Pressed
            );
            let s = card_surface_style(theme, active, hovered);
            button::Style {
                background: s.background,
                text_color: s.text_color.unwrap_or(p.background.base.text),
                border: s.border,
                shadow: s.shadow,
                snap: false,
            }
        }
    }
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

    // --- section_overflow_counts ---

    #[test]
    fn overflow_counts_none_when_fits() {
        let s = SectionScroll {
            offset_y: 0.0,
            viewport_h: 200.0,
            content_h: 150.0,
        };
        assert_eq!(section_overflow_counts(s, 5), (0, 0));
    }

    #[test]
    fn overflow_counts_at_top() {
        // 10 items, 40px each → content 400; viewport 120 → 3 visible-ish
        let s = SectionScroll {
            offset_y: 0.0,
            viewport_h: 120.0,
            content_h: 400.0,
        };
        let (above, below) = section_overflow_counts(s, 10);
        assert_eq!(above, 0);
        assert!(below >= 6, "below={below}");
    }

    #[test]
    fn overflow_counts_scrolled_mid() {
        let s = SectionScroll {
            offset_y: 160.0, // 4 full rows of 40
            viewport_h: 120.0,
            content_h: 400.0,
        };
        let (above, below) = section_overflow_counts(s, 10);
        assert_eq!(above, 4);
        assert!(below >= 2, "below={below}");
    }

    #[test]
    fn overflow_counts_empty_list() {
        let s = SectionScroll {
            offset_y: 0.0,
            viewport_h: 100.0,
            content_h: 200.0,
        };
        assert_eq!(section_overflow_counts(s, 0), (0, 0));
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
