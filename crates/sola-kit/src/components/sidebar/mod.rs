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
//! [`SidebarPanel`] builder plus a [`State`] blob. Gesture, hover, and
//! animation live in the kit; the consumer maps [`Msg`] through
//! [`State::update`] and handles [`Event`]. List chrome
//! ([`SidebarItemChrome::Row`]) is the browser etched title stack
//! (muted idle, reserved 1px lip + inset active well, hover-only `×`).
//! Collapsible sections render as an inset pocket with nested members.
//! [`SidebarItemChrome::Card`] is a separate product surface and is not
//! restyled by list etch.
//!
//! ```ignore
//! SidebarPanel::new(sections)
//!     .controller(&self.sidebar, Msg::Sidebar)
//!     .reorderable()
//!     .resizable_with(self.width, colors)
//!     .build()
//! ```
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
use std::rc::Rc;
use std::sync::OnceLock;

mod gesture;
mod strip;
pub use gesture::{Dest, Drop, Event, Msg, State};

use iced::advanced::Renderer as _;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::widget::scrollable::{Direction, Scrollbar, Viewport};
use iced::widget::text::Wrapping;
use iced::widget::{
    Container, Space, button, column, container, mouse_area, row, scrollable, sensor, stack, text,
};
use iced::{
    Background, Border, Color, Element, Length, Padding, Rectangle, Shadow, Size, Theme, Vector,
    mouse,
};

use crate::components::icon::{icon_handle, icon_svg, icon_svg_colored};
use crate::components::style::{
    CHROME_SURFACE, HAIRLINE_A, RADIUS_LG, RADIUS_SM, SPACE_MD, SPACE_SM, SPACE_XS, alpha,
    hairline_on, inset_surface, linear_bg, mix, mix_white,
};
use crate::fonts;

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
/// [`Self::Row`] is the default **list etch** (quiet title stack: muted
/// idle type, 1px lip + inset well when active, no selection-teal wash).
/// [`Self::Card`] is a softer, roomier product surface (session switcher)
/// — raised idle material, larger radius, more internal pad. Pair cards
/// with non-zero [`SidebarPanel::item_spacing`] (e.g. [`SPACE_MD`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarItemChrome {
    #[default]
    Row,
    Card,
}

pub use crate::components::status_mark::{STATUS_MARK_SLOT, SidebarIndicator, status_mark};

/// List density for [`SidebarPanel`] (and the [`sidebar`] helper).
///
/// [`Self::Normal`] is settings / mail / preview / storybook nav.
/// [`Self::Large`] is the browser / terminal tab-strip density.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarDensity {
    #[default]
    Normal,
    Large,
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
/// to `None` / [`SidebarItemChrome::Row`]. List rows use etch materials;
/// [`SidebarItemChrome::Card`] keeps the raised product surface.
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
    /// When set, [`SidebarPanel`] renders a hover-only stacked `×` that
    /// emits this message. Visibility is live pointer-over-row when
    /// [`SidebarPanel::item_hover`] is wired (so a close that slides the
    /// next row under a stationary cursor still shows the chip). `id` is
    /// auto-assigned from the row index when missing.
    pub on_close: Option<Message>,
    /// Hover-only stacked pencil (same overlay as [`Self::on_close`]).
    /// Used for inline rename on a collapsible header. Hidden while
    /// [`SidebarSection::header_content`] is set.
    pub on_edit: Option<Message>,
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
    /// List etch vs card materials / padding. Default is list etch.
    pub chrome: SidebarItemChrome,
    /// Custom body — replaces the default title/subtitle/secondary layout.
    pub content: Option<Element<'a, Message, Theme>>,
    /// Scroll-chip / overflow math when body height is not obvious from
    /// label+subtitle (required accuracy for tall custom cards).
    pub height_hint: Option<f32>,
    /// Right-click on the row. Does not start a reorder gesture.
    pub on_context: Option<Message>,
    /// Extra leading steps (12px each) for lineage / nesting. Zero by
    /// default so existing rows stay aligned.
    pub indent: u8,
    /// Collapsible section header: `Some(collapsed)` draws a lucide
    /// chevron and folder-caption type instead of a tab title.
    pub section_header: Option<bool>,
}

impl<'a, Message> SidebarItem<'a, Message> {
    pub fn new(label: impl Into<String>, message: Message) -> Self {
        Self {
            label: label.into(),
            active: false,
            message,
            shortcut: None,
            on_close: None,
            on_edit: None,
            secondary: None,
            subtitle: None,
            on_double_click: None,
            indicator: None,
            id: None,
            hover_action: None,
            chrome: SidebarItemChrome::Row,
            content: None,
            height_hint: None,
            on_context: None,
            indent: 0,
            section_header: None,
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

    /// Attach a hover-only stacked `×` emitting `msg`. With
    /// [`SidebarPanel::item_hover`] the chip follows the pointer after
    /// a row slides away (no mouse-out needed).
    pub fn on_close(mut self, msg: Message) -> Self {
        self.on_close = Some(msg);
        self
    }

    /// Hover-only pencil overlay; same hit rules as [`Self::on_close`].
    pub fn on_edit(mut self, msg: Message) -> Self {
        self.on_edit = Some(msg);
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
    /// Right-click emits `msg` and does not start reorder.
    pub fn on_context(mut self, msg: Message) -> Self {
        self.on_context = Some(msg);
        self
    }

    /// Indent the row by `steps` × 12px (lineage, nested lists).
    pub fn indent(mut self, steps: u8) -> Self {
        self.indent = steps;
        self
    }

    /// Mark this row as a collapsible-section header (`collapsed` picks
    /// the chevron). Members nest under it; the header stays flush.
    pub fn section_header(mut self, collapsed: bool) -> Self {
        self.section_header = Some(collapsed);
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
    /// Stable id for collapsible sections (drop / toggle). Optional for
    /// unlabeled or static groups.
    pub id: Option<String>,
    pub label: Option<String>,
    pub items: Vec<SidebarItem<'a, Message>>,
    /// When true, this section's item list fills remaining height and
    /// scrolls (hidden bar). At most one fill section is useful; if
    /// several are marked, the first wins the `Fill` slot and others
    /// still get a bounded scroll body.
    pub fill: bool,
    /// Opt-in collapsible header (browser tab groups). Renders as an
    /// inset pocket with nested members. Static section labels are
    /// unchanged when this is `None`.
    pub collapse: Option<SectionCollapse<'a, Message>>,
    /// When set, the section label is a quiet press target (collapse).
    pub on_label: Option<Message>,
    /// Trailing `+` on the section header (add a row in this group).
    pub on_add: Option<Message>,
}

/// Header chrome for a [`SidebarSection::collapsible`] section.
pub struct SectionCollapse<'a, Message> {
    pub collapsed: bool,
    pub on_toggle: Message,
    pub header_active: bool,
    pub on_context: Option<Message>,
    /// Hover pencil on the header (inline rename).
    pub on_edit: Option<Message>,
    /// Trailing count (shown when collapsed).
    pub count: Option<String>,
    /// Replace the default chevron+name body (e.g. an inline rename field).
    pub header_content: Option<Element<'a, Message, Theme>>,
}

impl<'a, Message> SidebarSection<'a, Message> {
    pub fn new(label: impl Into<String>, items: Vec<SidebarItem<'a, Message>>) -> Self {
        Self {
            id: None,
            label: Some(label.into()),
            items,
            fill: false,
            collapse: None,
            on_label: None,
            on_add: None,
        }
    }

    pub fn unlabeled(items: Vec<SidebarItem<'a, Message>>) -> Self {
        Self {
            id: None,
            label: None,
            items,
            fill: false,
            collapse: None,
            on_label: None,
            on_add: None,
        }
    }

    /// Identity used by reorder / toggle events.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Make the section label emit `msg` (e.g. collapse the group).
    pub fn on_label(mut self, msg: Message) -> Self {
        self.on_label = Some(msg);
        self
    }

    /// Trailing `+` on the group header.
    pub fn on_add(mut self, msg: Message) -> Self {
        self.on_add = Some(msg);
        self
    }

    /// This section's item body fills remaining panel height and scrolls
    /// without a visible scrollbar. Pair with
    /// [`SidebarPanel::section_scroll`] for overflow chips.
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }

    /// Clickable Large-density header. Items nest one step inside an
    /// inset pocket and are omitted while collapsed. The header is a
    /// reorder row when the panel is reorderable.
    pub fn collapsible(mut self, collapsed: bool, on_toggle: Message) -> Self {
        self.collapse = Some(SectionCollapse {
            collapsed,
            on_toggle,
            header_active: false,
            on_context: None,
            on_edit: None,
            count: None,
            header_content: None,
        });
        self
    }

    pub fn header_active(mut self, active: bool) -> Self {
        if let Some(c) = &mut self.collapse {
            c.header_active = active;
        }
        self
    }

    pub fn header_context(mut self, msg: Message) -> Self {
        if let Some(c) = &mut self.collapse {
            c.on_context = Some(msg);
        }
        self
    }

    pub fn header_count(mut self, n: usize) -> Self {
        if let Some(c) = &mut self.collapse {
            c.count = Some(n.to_string());
        }
        self
    }

    pub fn header_content(mut self, el: impl Into<Element<'a, Message, Theme>>) -> Self {
        if let Some(c) = &mut self.collapse {
            c.header_content = Some(el.into());
        }
        self
    }

    /// Hover pencil on the group header (rename). Hidden while a
    /// [`Self::header_content`] field is showing.
    pub fn header_edit(mut self, msg: Message) -> Self {
        if let Some(c) = &mut self.collapse {
            c.on_edit = Some(msg);
        }
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
fn item_row_height<Message>(item: &SidebarItem<'_, Message>, density: SidebarDensity) -> f32 {
    if let Some(h) = item.height_hint {
        return h;
    }
    // Header rename fields use `content` but must stay a list row —
    // card height is only for session/card chrome.
    if item.section_header.is_none()
        && (item.content.is_some() || item.chrome == SidebarItemChrome::Card)
    {
        return CARD_HEIGHT_HINT;
    }
    let m = density.metrics();
    let title_h = m.font as f32;
    let text_h = if item.subtitle.is_some() {
        title_h + TITLE_SUB_GAP + 11.0
    } else {
        title_h
    };
    // Multi-line secondary (e.g. context KB + age) needs room in scroll math.
    let trail_h = item
        .secondary
        .as_ref()
        .map(|s| s.lines().filter(|l| !l.is_empty()).count().max(1) as f32 * 12.0)
        .unwrap_or(0.0);
    let pad_v = m.row_pad_v as f32;
    // List rows always reserve the 1px etch lip so selecting a row
    // never shifts the title (the lip paints only when active).
    let lip = if item.chrome == SidebarItemChrome::Row {
        2.0
    } else {
        0.0
    };
    pad_v * 2.0 + text_h.max(trail_h) + lip
}

/// Layout height of a Large/Normal list-etch row, matching iced's default
/// [`LineHeight::Relative(1.3)`] (size 12 → 15.6). [`PANEL_ROW_H`] is 32;
/// using it as a `Space` hole is ~0.4px taller and pixel-snaps as a 1px
/// shift of whatever rows sit on a rounding boundary.
pub fn panel_etch_row_height(density: SidebarDensity) -> f32 {
    let m = density.metrics();
    let text_h = m.font as f32 * 1.3;
    let lip = 2.0;
    px(m.row_pad_v as f32 * 2.0 + text_h + lip)
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
    section_content_height_with(items, item_spacing, SidebarDensity::Normal)
}

/// Like [`section_content_height_with_spacing`], with explicit list density.
pub fn section_content_height_with<Message>(
    items: &[SidebarItem<'_, Message>],
    item_spacing: f32,
    density: SidebarDensity,
) -> f32 {
    let pad_v = 8.0; // matches body column padding [4, 8]
    if items.is_empty() {
        return pad_v;
    }
    let rows: f32 = items
        .iter()
        .map(|item| item_row_height(item, density))
        .sum();
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
        let first_below = ((scroll.offset_y + scroll.viewport_h) / avg)
            .ceil()
            .max(0.0) as usize;
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
    let first_below = ((scroll.offset_y + scroll.viewport_h) / avg)
        .ceil()
        .max(0.0) as usize;
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
            col = col.push(section_header(label, section.on_label, section.on_add));
        }
        for (i, item) in section.items.into_iter().enumerate() {
            // `sidebar()` never enables reorder, so `render_item` takes
            // the plain `button(..).on_press(item.message)` path. `index`
            // is only read on the reorder path. No `item_hover` → close
            // (if any) stays visible as a stacked fallback.
            col = col.push(render_item(
                item,
                None,
                i,
                false,
                SidebarDensity::Normal,
                false,
                false,
            ));
        }
    }
    container(col)
        .style(style)
        .width(Length::Fixed(SIDEBAR_WIDTH))
        .height(Length::Fill)
}

/// Resolved per-density metrics. Values are deliberate, not derived.
struct DensityMetrics {
    row_pad_v: u16,
    row_pad_h: u16,
    font: u32,
    close: u32,
    gap: f32,
}

impl SidebarDensity {
    fn metrics(self) -> DensityMetrics {
        match self {
            SidebarDensity::Normal => DensityMetrics {
                row_pad_v: 6,
                row_pad_h: 10,
                font: 13,
                close: 14,
                gap: SPACE_XS,
            },
            // Browser chrome: a stack of titles, not fat list-pills.
            SidebarDensity::Large => DensityMetrics {
                row_pad_v: 7,
                row_pad_h: 10,
                font: 12,
                close: 14,
                gap: 3.0,
            },
        }
    }
}

/// 1px rim of the etch — a hair lighter than the column so the cut
/// reads as a lip, not a painted card.
fn tab_etch_lip() -> container::Style {
    container::Style {
        background: Some(Background::Color(mix_white(CHROME_SURFACE, 0.06))),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_SM.into(),
        },
        ..container::Style::default()
    }
}

/// Air between collapsible group pockets (and before the loose run).
/// Pockets already carry their own pad — this is only enough that two
/// wells do not fuse. A large value jumps when reorder flatten drops
/// the wells.
pub(crate) const GROUP_WELL_GAP: f32 = SPACE_XS;
/// Horizontal inset of the pocket around header + members.
pub(crate) const GROUP_WELL_PAD_H: f32 = SPACE_SM;
/// Vertical inset of the pocket. Keep tight so stacked groups do not
/// open a band of empty chrome.
pub(crate) const GROUP_WELL_PAD_V: f32 = SPACE_XS;
/// Body pad inside a group well (`Padding::from([2, 4])`).
pub(crate) const COLLAPSE_BODY_PAD_V: f32 = 2.0;

/// Quiet inset pocket for a collapsible section — membership reads as
/// containment. A 1px etch rim (same hairline as fields) marks the
/// well so a drop at the floor stays in this group, not the next.
pub(crate) fn group_well_style() -> container::Style {
    let fill = inset_surface(CHROME_SURFACE, 0.12);
    container::Style {
        background: Some(Background::Color(fill)),
        border: hairline_on(fill, HAIRLINE_A, RADIUS_SM),
        ..container::Style::default()
    }
}

pub(crate) fn wrap_group_well<'a, Message: 'a>(
    body: impl Into<Element<'a, Message>>,
    clip: bool,
) -> Element<'a, Message> {
    container(body.into())
        .width(Length::Fill)
        .padding(Padding {
            top: GROUP_WELL_PAD_V,
            bottom: GROUP_WELL_PAD_V,
            left: GROUP_WELL_PAD_H,
            right: GROUP_WELL_PAD_H,
        })
        .style(|_theme: &Theme| group_well_style())
        .clip(clip)
        .into()
}

fn section_chevron<'a, Message: 'a>(collapsed: bool) -> Element<'a, Message> {
    let name = if collapsed {
        "lucide/chevron-right"
    } else {
        "lucide/chevron-down"
    };
    let color = Color {
        r: 0.55,
        g: 0.58,
        b: 0.64,
        a: 0.95,
    };
    icon_svg_colored(icon_handle(name), 12, color)
}

fn tab_close_icon() -> iced::widget::svg::Handle {
    static H: OnceLock<iced::widget::svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/x")).clone()
}

fn tab_edit_icon() -> iced::widget::svg::Handle {
    static H: OnceLock<iced::widget::svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/pencil")).clone()
}

/// Quiet title stack: idle is muted type on nothing; active is a
/// darker well (etched into the column), not a gradient card.
fn tab_item_style(theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let p = theme.extended_palette();
    let muted = p.secondary.base.text;
    let fg = p.background.base.text;
    if active {
        return button::Style {
            background: Some(Background::Color(inset_surface(CHROME_SURFACE, 0.22))),
            text_color: fg,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 4.0.into(),
            },
            shadow: Default::default(),
            snap: false,
        };
    }
    let (bg, text_color) = match status {
        button::Status::Hovered | button::Status::Pressed => {
            (alpha(p.background.strong.color, 0.45), fg)
        }
        _ => (Color::TRANSPARENT, muted),
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

fn tab_close_style(
    theme: &Theme,
    status: button::Status,
    active: bool,
    chrome: SidebarItemChrome,
) -> button::Style {
    let p = theme.extended_palette();
    // Opaque chip so the stacked × covers the title instead of sitting
    // on it. Idle hover wash is 45% alpha; bake that onto the column
    // so the pad matches the row without the glyphs showing through.
    let rest = match chrome {
        SidebarItemChrome::Row => {
            if active {
                inset_surface(CHROME_SURFACE, 0.22)
            } else {
                mix(p.background.strong.color, CHROME_SURFACE, 0.45)
            }
        }
        SidebarItemChrome::Card => {
            let raised = p.background.weaker.color;
            if active {
                mix(p.background.strong.color, raised, 0.92)
            } else {
                let idle = mix(raised, p.background.base.color, 0.42);
                mix(p.background.strong.color, idle, 0.55)
            }
        }
    };
    let bg = match status {
        button::Status::Hovered | button::Status::Pressed => {
            mix(p.background.strong.color, rest, 0.40)
        }
        _ => rest,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: p.secondary.base.text,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_SM.into(),
        },
        shadow: Default::default(),
        snap: false,
    }
}

/// Synthesize a reorderable header row for a collapsible section.
fn collapse_header_item<'a, Message: Clone + 'a>(
    label: Option<String>,
    collapse: SectionCollapse<'a, Message>,
) -> SidebarItem<'a, Message> {
    let name = label.unwrap_or_default();
    let mut item = SidebarItem::new(name.clone(), collapse.on_toggle)
        .active(collapse.header_active)
        .id(format!("__section:{name}"))
        .section_header(collapse.collapsed);
    if collapse.collapsed {
        if let Some(n) = collapse.count {
            item = item.secondary(n);
        }
    }
    if let Some(ctx) = collapse.on_context {
        item = item.on_context(ctx);
    }
    if collapse.header_content.is_none() {
        if let Some(edit) = collapse.on_edit {
            item = item.on_edit(edit);
        }
    }
    if let Some(content) = collapse.header_content {
        // Keep the folder chevron; only the name is the field.
        let body = row![section_chevron(collapse.collapsed), content]
            .spacing(SPACE_SM)
            .align_y(iced::Alignment::Center)
            .width(Length::Fill);
        item = item.content(body);
    }
    item
}

fn section_header<'a, Message: Clone + 'a>(
    label: String,
    on_press: Option<Message>,
    on_add: Option<Message>,
) -> Element<'a, Message> {
    // Uppercase tracked section labels — graphite tool UI (sola-kit-ds).
    let label_el = text(label.to_uppercase())
        .font(fonts::ui_medium())
        .size(10)
        .style(|theme: &Theme| {
            let p = theme.extended_palette();
            iced::widget::text::Style {
                color: Some(p.secondary.base.text),
            }
        });
    let pad = Padding {
        top: SPACE_SM + 2.0, // 6
        bottom: SPACE_SM + 1.0,
        left: SPACE_MD + 2.0, // 10
        right: SPACE_MD + 2.0,
    };
    let name: Element<'a, Message> = match on_press {
        Some(msg) => button(label_el)
            .padding(pad)
            .style(|theme: &Theme, status| {
                let p = theme.extended_palette();
                let hover = matches!(status, button::Status::Hovered | button::Status::Pressed);
                button::Style {
                    background: hover.then_some(Background::Color(p.background.weak.color)),
                    text_color: p.secondary.base.text,
                    border: Border::default(),
                    shadow: Shadow::default(),
                    snap: true,
                }
            })
            .on_press(msg)
            .into(),
        None => container(label_el).padding(pad).into(),
    };
    let Some(add) = on_add else {
        return name;
    };
    let plus = {
        let handle = icon_handle("lucide/plus");
        button(icon_svg_colored(
            handle,
            12,
            Color {
                r: 0.55,
                g: 0.58,
                b: 0.64,
                a: 0.95,
            },
        ))
        .padding(Padding::from([2, 4]))
        .style(|theme: &Theme, status| {
            let p = theme.extended_palette();
            let hover = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: hover.then_some(Background::Color(p.background.weak.color)),
                text_color: p.secondary.base.text,
                border: Border {
                    radius: RADIUS_SM.into(),
                    ..Default::default()
                },
                shadow: Shadow::default(),
                snap: true,
            }
        })
        .on_press(add)
    };
    row![
        name,
        Space::new().width(Length::Fill),
        container(plus).padding(Padding {
            top: SPACE_SM,
            bottom: SPACE_SM,
            left: 0.0,
            right: SPACE_MD,
        }),
    ]
    .align_y(iced::Alignment::Center)
    .width(Length::Fill)
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
/// Snap layout geometry to whole pixels. Fractional `Length::Fixed`
/// (31.6 vs 32) is a 1px jump of every row below.
#[inline]
pub(crate) fn px(v: f32) -> f32 {
    v.round()
}

/// Height of each panel row (px). Used by [`panel_drop_index`].
pub const PANEL_ROW_H: f32 = 32.0;
/// Height of the toggle-button header row (px). Used as the list-top
/// offset for [`panel_drop_index`].
pub const PANEL_HEADER_H: f32 = 32.0;
/// Movement threshold (px) below which a press-then-release is a click,
/// not a completed reorder drag. Spec: 2px — 5px made pickup jump.
pub const PANEL_REORDER_THRESHOLD: f32 = 2.0;
/// Sibling glide duration while a row is mid-reorder.
pub const PANEL_REORDER_ANIM_MS: u64 = 180;

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
    pub cursor_y: f32,
}

// ──────────────────────────── Shared item render ────────────────────────────

fn build_reorder_strip<'a, Message: Clone + 'a>(
    sections: Vec<SidebarSection<'a, Message>>,
    density: SidebarDensity,
    item_spacing: f32,
    hovered_id: Option<String>,
    hover_wired: bool,
    collapsed: bool,
    on_action: Rc<dyn Fn(Msg) -> Message + 'a>,
) -> Element<'a, Message> {
    let mut leaves = Vec::new();
    let mut meta = Vec::new();
    let mut spans = Vec::new();
    let mut row_index = 0usize;
    for (si, section) in sections.into_iter().enumerate() {
        let grouped = section.collapse.is_some();
        let hide = section.collapse.as_ref().is_some_and(|c| c.collapsed);
        let start = leaves.len();
        let gid = section
            .id
            .clone()
            .or_else(|| grouped.then(|| format!("s{si}")));
        if let Some(collapse) = section.collapse {
            let header = assign_close_id(collapse_header_item(section.label, collapse), row_index);
            let hid = gid.clone().unwrap_or_else(|| format!("s{si}"));
            let hover_id = header.id.clone();
            let show = hover_id
                .as_ref()
                .is_some_and(|id| hovered_id.as_ref() == Some(id));
            let mut el = if collapsed {
                collapsed_row(&header, row_index, None)
            } else {
                render_item(header, None, row_index, show, density, hover_wired, true)
            };
            if let Some(id) = hover_id {
                let act = Rc::clone(&on_action);
                el = mouse_area(el).on_enter(act(Msg::Hover(Some(id)))).into();
            }
            leaves.push(el);
            meta.push(strip::LeafMeta {
                id: hid.clone(),
                kind: strip::LeafKind::Header,
                group: Some(hid),
            });
            row_index += 1;
        }
        if !hide {
            for item in section.items {
                let item = assign_close_id(item, row_index);
                let id = item.id.clone().unwrap_or_else(|| format!("i{row_index}"));
                let show = item
                    .id
                    .as_ref()
                    .is_some_and(|hid| hovered_id.as_ref() == Some(hid));
                let hover_id = item.id.clone();
                let mut el = if collapsed {
                    collapsed_row(&item, row_index, None)
                } else {
                    render_item(item, None, row_index, show, density, hover_wired, true)
                };
                if let Some(hid) = hover_id {
                    let act = Rc::clone(&on_action);
                    el = mouse_area(el).on_enter(act(Msg::Hover(Some(hid)))).into();
                }
                leaves.push(el);
                meta.push(strip::LeafMeta {
                    id: id.clone(),
                    kind: strip::LeafKind::Item,
                    group: if grouped { gid.clone() } else { None },
                });
                row_index += 1;
            }
        }
        spans.push(strip::SectionSpan {
            grouped,
            start,
            len: leaves.len() - start,
        });
    }
    let strip: Element<'a, Message> =
        strip::ReorderStrip::new(leaves, meta, spans, item_spacing, Rc::clone(&on_action)).into();
    mouse_area(hidden_scroll(strip, None, None))
        .on_exit(on_action(Msg::Hover(None)))
        .into()
}

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
/// OUTSIDE that `mouse_area`, stacked over the row so it never steals
/// title width.
///
/// `hover_tracked` is true when the parent wired [`SidebarPanel::item_hover`].
/// List `on_close` is then hover-only via live cursor-over-row (not enter
/// tracking, so a close that slides the next row under the pointer still
/// shows ×). Without tracking the × stays visible so callers that have
/// not migrated hover still get a close target.
fn render_item<'a, Message>(
    item: SidebarItem<'a, Message>,
    reorder: Option<&ReorderCfg<'a, Message>>,
    index: usize,
    show_hover_action: bool,
    density: SidebarDensity,
    hover_tracked: bool,
    strip_owned: bool,
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
        on_edit,
        secondary,
        subtitle,
        on_double_click,
        indicator,
        id: _,
        hover_action,
        chrome,
        content: custom,
        height_hint: _,
        on_context,
        indent,
        section_header,
    } = item;

    let m = density.metrics();
    // Custom card bodies own their own padding (session tabs inset a
    // bottom context bar); structured card rows keep kit pad.
    // List etch uses density metrics (browser title-stack, not fat pills).
    let (pad_v, pad_h) = match (chrome, custom.is_some()) {
        (SidebarItemChrome::Row, _) => (m.row_pad_v as f32, m.row_pad_h as f32),
        (SidebarItemChrome::Card, true) => (0.0, 0.0),
        (SidebarItemChrome::Card, false) => (CARD_PAD_V, CARD_PAD_H),
    };
    let pad = Padding {
        top: pad_v,
        bottom: pad_v,
        left: pad_h + f32::from(indent) * 12.0,
        right: pad_h,
    };
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
            active,
            chrome,
            density,
            section_header,
        )
    };

    // Strip-owned press: non-pressable face so ReorderStrip sees mousedown.
    // A child `button` / `mouse_area.on_press` would capture the event and
    // the 2px threshold would never run.
    if strip_owned {
        let row_el: Element<'a, Message> = container(body)
            .width(Length::Fill)
            .padding(pad)
            .style(move |theme: &Theme| row_container_style(theme, active, chrome, hovered))
            .into();
        return finish_list_row(
            row_el,
            chrome,
            active,
            on_close,
            on_edit,
            hover_tracked,
            density,
        );
    }

    // ── Plain path (no reorder). ──
    let Some(reorder) = reorder else {
        // Hover-action rows: full padded row is the select target (pad is
        // inside the mouse_area so inter-row space is clickable). Trash
        // overlays bottom-right (under the age label) via `stack` — same
        // pattern as the list-etch close overlay — so showing it never steals width
        // from the age label or shifts layout (which also broke hover
        // enter/exit when moving across rows).
        let row_el: Element<'a, Message> =
            if hover_action.is_some() {
                let mut select =
                    mouse_area(container(body).width(Length::Fill).padding(pad).style(
                        move |theme: &Theme| row_container_style(theme, active, chrome, hovered),
                    ))
                    .interaction(mouse::Interaction::Pointer)
                    .on_press(message);
                if let Some(ctx) = on_context.clone() {
                    select = select.on_right_press(ctx);
                }
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
            } else if on_double_click.is_some() || on_context.is_some() {
                let mut area =
                    mouse_area(container(body).width(Length::Fill).padding(pad).style(
                        move |theme: &Theme| row_container_style(theme, active, chrome, false),
                    ))
                    .interaction(mouse::Interaction::Pointer)
                    .on_press(message);
                if let Some(ctx) = on_context {
                    area = area.on_right_press(ctx);
                }
                if let Some(dbl) = on_double_click {
                    area = area.on_double_click(dbl);
                }
                area.into()
            } else {
                button(body)
                    .style(move |t, status| item_style_chrome(t, status, active, chrome))
                    .padding(pad)
                    .width(Length::Fill)
                    .on_press(message)
                    .into()
            };
        return finish_list_row(
            row_el,
            chrome,
            active,
            on_close,
            on_edit,
            hover_tracked,
            density,
        );
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
            .style(move |theme: &Theme| row_container_style(theme, active, chrome, hovered)),
    )
    // Pointer at rest; grabbing while this row is the one in flight.
    .interaction(if is_dragged {
        mouse::Interaction::Grabbing
    } else {
        mouse::Interaction::Pointer
    })
    .on_press((reorder.on_press)(index));
    if let Some(ctx) = on_context {
        pressable = pressable.on_right_press(ctx);
    }
    if let Some(dbl) = on_double_click {
        pressable = pressable.on_double_click(dbl);
    }

    // Motion is applied by the caller via [`with_reorder_motion`] so the
    // drag path can pass pointer-relative dy for the lifted row.
    finish_list_row(
        pressable.into(),
        chrome,
        active,
        on_close,
        on_edit,
        hover_tracked,
        density,
    )
}

/// Etch lip (list rows) + hover-only stacked close/edit. The 1px pad is
/// reserved on every list row so selecting does not shift the title;
/// the lip colour paints only when active. Card chrome skips the lip
/// so agent session rows stay pixel-stable. The chip sits *on top* of the
/// row via `stack` (never a trailing sibling) so the title width is stable
/// and the control does not steal the reorder press target.
fn finish_list_row<'a, Message: Clone + 'a>(
    row_el: Element<'a, Message>,
    chrome: SidebarItemChrome,
    active: bool,
    on_close: Option<Message>,
    on_edit: Option<Message>,
    hover_tracked: bool,
    density: SidebarDensity,
) -> Element<'a, Message> {
    let etched: Element<'a, Message> = if chrome == SidebarItemChrome::Row {
        container(row_el)
            .padding(1)
            .width(Length::Fill)
            .style(move |_theme: &Theme| {
                if active {
                    tab_etch_lip()
                } else {
                    container::Style::default()
                }
            })
            .into()
    } else {
        row_el
    };

    let chip = if let Some(msg) = on_close {
        Some(hover_chip(HoverChip::Close, msg, active, chrome, density))
    } else {
        on_edit.map(|msg| hover_chip(HoverChip::Edit, msg, active, chrome, density))
    };
    let Some(chip) = chip else {
        return etched;
    };
    if hover_tracked {
        // Always mount: paint/hit from live cursor-over-row, not enter
        // tracking. After a close the next row slides under a stationary
        // pointer; `on_enter` never fires, but the next draw still sees
        // the cursor and the chip stays available.
        return stack![etched, HoverClose { chip }].into();
    }
    // No hover tracking: chip stays visible (`sidebar()` helper, or a
    // panel that never called `item_hover`).
    stack![etched, chip].into()
}

#[derive(Clone, Copy)]
enum HoverChip {
    Close,
    Edit,
}

/// Right-aligned stacked chip. Shared by the always-visible fallback and
/// the cursor-gated [`HoverClose`] overlay.
fn hover_chip<'a, Message: Clone + 'a>(
    kind: HoverChip,
    msg: Message,
    active: bool,
    chrome: SidebarItemChrome,
    density: SidebarDensity,
) -> Element<'a, Message> {
    let m = density.metrics();
    let handle = match kind {
        HoverChip::Close => tab_close_icon(),
        HoverChip::Edit => tab_edit_icon(),
    };
    let chip = button(icon_svg(handle, m.close as u16))
        .style(move |theme, status| tab_close_style(theme, status, active, chrome))
        .padding(Padding::from([3, 6]))
        .on_press(msg);
    container(chip)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Center)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding::from([0, 4]))
        .into()
}

/// Full-row overlay that paints the close chip only while the pointer is
/// over the row. Mounted even when app-owned hover is `None`, so a row
/// that slides under a stationary cursor (close, collapse, dissolve)
/// still shows × without a mouse-out.
struct HoverClose<'a, Message> {
    chip: Element<'a, Message>,
}

impl<Message> Widget<Message, Theme, iced::Renderer> for HoverClose<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.chip)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.chip));
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
        let child = self
            .chip
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &limits);
        layout::Node::with_children(size, vec![child])
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if !cursor.is_over(layout.bounds()) {
            return;
        }
        let child = layout.children().next().expect("hover-close chip");
        self.chip.as_widget_mut().update(
            &mut tree.children[0],
            event,
            child,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
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
        if !cursor.is_over(layout.bounds()) {
            return mouse::Interaction::None;
        }
        let child = layout.children().next().expect("hover-close chip");
        self.chip.as_widget().mouse_interaction(
            &tree.children[0],
            child,
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
        if !cursor.is_over(layout.bounds()) {
            return;
        }
        let child = layout.children().next().expect("hover-close chip");
        self.chip.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            child,
            cursor,
            viewport,
        );
    }
}

impl<'a, Message: 'a> From<HoverClose<'a, Message>> for Element<'a, Message> {
    fn from(value: HoverClose<'a, Message>) -> Self {
        Element::new(value)
    }
}

/// Stable id for close-on-hover when the caller omitted [`SidebarItem::id`].
fn assign_close_id<'a, Message>(
    mut item: SidebarItem<'a, Message>,
    index: usize,
) -> SidebarItem<'a, Message> {
    if item.on_close.is_some() && item.id.is_none() {
        item.id = Some(format!("__row:{index}"));
    }
    item
}

/// Title + optional subtitle (+ leading indicator). Takes `Fill` width.
///
/// List etch: idle muted + regular; active full fg + [`fonts::ui_medium`].
/// Section headers stay medium with a lucide chevron (folder caption).
/// Card structured rows keep the historical 14px regular face.
fn item_text_block<'a, Message: 'a>(
    label: &str,
    subtitle: Option<&str>,
    indicator: Option<SidebarIndicator>,
    active: bool,
    chrome: SidebarItemChrome,
    density: SidebarDensity,
    section_header: Option<bool>,
) -> Element<'a, Message> {
    let is_header = section_header.is_some();
    let (title_font, title_size) = match chrome {
        SidebarItemChrome::Row => {
            let font = if active || is_header {
                fonts::ui_medium()
            } else {
                fonts::ui()
            };
            (font, density.metrics().font)
        }
        SidebarItemChrome::Card => (fonts::ui(), 14),
    };
    let title = text(label.to_string())
        .font(title_font)
        .size(title_size)
        .wrapping(Wrapping::None)
        .width(Length::Fill);

    let mut title_row = row![].spacing(SPACE_SM).align_y(iced::Alignment::Center);
    if let Some(collapsed) = section_header {
        title_row = title_row.push(section_chevron(collapsed));
    }
    if let Some(ind) = indicator {
        title_row = title_row.push(status_dot(ind));
    }
    title_row = title_row.push(title);

    let mut text_col = column![title_row]
        .spacing(TITLE_SUB_GAP)
        .width(Length::Fill);
    if let Some(sub) = subtitle {
        // Indent subtitle under the title text when a leading dot is present.
        let sub_pad = if indicator.is_some() {
            STATUS_MARK_SLOT + SPACE_SM
        } else {
            0.0
        };
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
    active: bool,
    chrome: SidebarItemChrome,
    density: SidebarDensity,
    section_header: Option<bool>,
) -> Element<'a, Message> {
    let text_box = item_text_block(
        label,
        subtitle,
        indicator,
        active,
        chrome,
        density,
        section_header,
    );
    let has_trail = secondary.is_some() || shortcut.is_some() || hover_action.is_some();
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
    status_mark(indicator)
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

// ─────────────────────────────── SidebarPanel ───────────────────────────────

/// Opt-in richer sidebar: collapse/expand, drag-to-resize, drag-reorder,
/// per-item shortcut hints / close buttons / secondary labels, section-
/// scoped scroll with overflow chips, plus an optional footer.
///
/// Gesture / hover / animation live in [`State`]. Wire
/// [`Self::controller`] then opt into [`Self::reorderable`] and
/// [`Self::resizable`]. Returns an `Element` (a `row!`/`stack!`), not a
/// `Container`, so it composes directly.
pub struct SidebarPanel<'a, Message> {
    sections: Vec<SidebarSection<'a, Message>>,
    collapse: Option<(bool, Message)>,
    /// `(width, colors)` — `colors` is `None` for theme-default chrome.
    resize: Option<(f32, Option<crate::components::DividerColors>)>,
    reorder: bool,
    controller: Option<(&'a State, Rc<dyn Fn(Msg) -> Message + 'a>)>,
    /// Optional leading content (search field, brand, rename bar).
    /// Stacked above the section list; never scrolls with items.
    header: Option<Element<'a, Message, Theme>>,
    footer: Option<Element<'a, Message, Theme>>,
    /// Viewport snapshot + callback for the fill section's scroll body.
    /// When set, fill sections show `↑ N …` / `↓ N …` overflow chips.
    section_scroll: Option<(SectionScroll, Box<dyn Fn(SectionScroll) -> Message + 'a>)>,
    /// Vertical gap between item rows in a section body. `None` means
    /// "use density default" (list etch gap). Explicit `0` keeps a packed
    /// clickable band. Card stacks pass e.g. [`SPACE_MD`].
    item_spacing: Option<f32>,
    /// List pad / type / default gap. Card chrome ignores pad/type.
    density: SidebarDensity,
    /// When true and the panel is not resizable, the column is `Fill`
    /// so a parent can size it.
    fill_width: bool,
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
            reorder: false,
            controller: None,
            header: None,
            footer: None,
            section_scroll: None,
            item_spacing: None,
            density: SidebarDensity::Normal,
            fill_width: false,
        }
    }

    /// Kit-owned gesture blob. Forward every [`Msg`] into [`State::update`].
    pub fn controller(mut self, state: &'a State, f: impl Fn(Msg) -> Message + 'a) -> Self {
        self.controller = Some((state, Rc::new(f)));
        self
    }

    /// Space between consecutive item rows (`0` = packed / fully clickable).
    /// Overrides the density default gap.
    pub fn item_spacing(mut self, spacing: f32) -> Self {
        self.item_spacing = Some(spacing.max(0.0));
        self
    }

    /// List density (pad, primary type size, default inter-row gap).
    /// Card chrome is unchanged. Default [`SidebarDensity::Normal`].
    pub fn density(mut self, density: SidebarDensity) -> Self {
        self.density = density;
        self
    }

    /// Column width follows the parent instead of [`SIDEBAR_WIDTH`].
    /// Ignored when [`Self::resizable`] / [`Self::resizable_with`] is set.
    pub fn fill_width(mut self) -> Self {
        self.fill_width = true;
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

    /// Render a drag divider on the right edge. Requires [`Self::controller`].
    /// Divider colours use the theme default; prefer [`Self::resizable_with`]
    /// when the adjacent surfaces differ.
    pub fn resizable(mut self, width: f32) -> Self {
        self.resize = Some((width, None));
        self
    }

    /// Like [`Self::resizable`], but with explicit **a | line | b**
    /// divider colours so the hit strip matches the panel and its
    /// neighbour (e.g. raised sidebar | terminal canvas).
    pub fn resizable_with(mut self, width: f32, colors: crate::components::DividerColors) -> Self {
        self.resize = Some((width, Some(colors)));
        self
    }

    /// Enable drag-to-reorder. Requires [`Self::controller`]. Click vs
    /// drag is decided on release; drop arrives as [`Event::Drop`].
    pub fn reorderable(mut self) -> Self {
        self.reorder = true;
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

    pub fn build(self) -> Element<'a, Message, Theme> {
        let SidebarPanel {
            sections,
            collapse,
            resize,
            reorder: reorder_enabled,
            controller,
            header,
            footer,
            section_scroll,
            item_spacing,
            density,
            fill_width,
        } = self;
        let item_spacing = item_spacing.unwrap_or(density.metrics().gap);

        let on_action = controller.as_ref().map(|(_, f)| Rc::clone(f));
        let hovered_id: Option<String> = controller
            .as_ref()
            .and_then(|(s, _)| s.hover().map(str::to_string));
        let hover_wired = on_action.is_some();
        let reorder_ref: Option<&ReorderCfg<'a, Message>> = None;

        let collapsed = collapse.as_ref().map(|(c, _)| *c).unwrap_or(false);
        let (scroll_snap, mut on_section_scroll) = match section_scroll {
            Some((snap, cb)) => (snap, Some(cb)),
            None => (SectionScroll::default(), None),
        };

        // Fixed chrome (collapse + header + footer). Section *labels* also
        // stay outside the scroll body; only item lists scroll.
        let mut chrome = column![]
            .spacing(0.0)
            .width(Length::Fill)
            .height(Length::Fill);

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
                chrome = chrome.push(container(header).padding(Padding {
                    top: 10.0,
                    right: 10.0,
                    bottom: 8.0,
                    left: 10.0,
                }));
            }
        }

        // Visible reorder rows: collapsible headers + non-hidden items.
        let _total_items: usize = sections
            .iter()
            .map(|s| {
                let header = usize::from(s.collapse.is_some());
                let n = match &s.collapse {
                    Some(c) if c.collapsed => 0,
                    _ => s.items.len(),
                };
                header + n
            })
            .sum();
        let n_sections = sections.len();
        let any_explicit_fill = sections.iter().any(|s| s.fill);
        // Auto-fill a lone section so a single long list scrolls without
        // the caller remembering `.fill()`. Multiple sections require an
        // explicit mark so short groups don't steal the Fill slot.
        let auto_fill_single = !any_explicit_fill && n_sections == 1;
        let sections_el: Element<'a, Message, Theme> = if reorder_enabled {
            match on_action.as_ref() {
                Some(act) => build_reorder_strip(
                    sections,
                    density,
                    item_spacing,
                    hovered_id,
                    hover_wired,
                    collapsed,
                    Rc::clone(act),
                ),
                None => column![].into(),
            }
        } else {
            // At rest: section labels sticky; item bodies scroll per-fill.
            let mut sections_col = column![]
                .spacing(0.0)
                .width(Length::Fill)
                .height(Length::Fill);
            let mut row_index = 0usize;
            let mut assigned_fill = false;

            let mut prev_collapsible = false;
            for (si, section) in sections.into_iter().enumerate() {
                let is_collapsible = section.collapse.is_some();
                if si > 0 && !collapsed {
                    let gap = if is_collapsible || prev_collapsible {
                        GROUP_WELL_GAP
                    } else {
                        12.0
                    };
                    sections_col = sections_col.push(Space::new().height(Length::Fixed(gap)));
                }
                prev_collapsible = is_collapsible;

                let hide_items = section.collapse.as_ref().is_some_and(|c| c.collapsed);
                let visible_items: Vec<SidebarItem<'a, Message>> = if hide_items {
                    Vec::new()
                } else {
                    section.items
                };
                let n_in_section = visible_items.len() + usize::from(section.collapse.is_some());
                let mut content_h =
                    section_content_height_with(&visible_items, item_spacing, density);
                if is_collapsible {
                    // Tighter body pad + pocket pad; header row.
                    content_h =
                        content_h - 8.0 + COLLAPSE_BODY_PAD_V * 2.0 + GROUP_WELL_PAD_V * 2.0;
                    content_h += PANEL_ROW_H + item_spacing;
                }
                let wants_fill = !collapsed && (section.fill || auto_fill_single) && !assigned_fill;
                if wants_fill {
                    assigned_fill = true;
                }

                if section.collapse.is_none() {
                    if let Some(label) = section.label.clone() {
                        if !collapsed {
                            sections_col = sections_col.push(section_header(
                                label,
                                section.on_label,
                                section.on_add,
                            ));
                        }
                    }
                }

                let body_pad = if is_collapsible {
                    Padding::from([COLLAPSE_BODY_PAD_V, 4.0])
                } else {
                    Padding::from([4.0, 8.0])
                };
                let mut body_items = column![].spacing(item_spacing).padding(body_pad);
                if let Some(collapse) = section.collapse {
                    if !collapsed {
                        let header = collapse_header_item(section.label, collapse);
                        let header = assign_close_id(header, row_index);
                        let hid = header.id.clone();
                        let show_action = hid
                            .as_ref()
                            .is_some_and(|id| hovered_id.as_ref() == Some(id));
                        let mut row_el = render_item(
                            header,
                            reorder_ref,
                            row_index,
                            show_action,
                            density,
                            true,
                            false,
                        );
                        if let (Some(id), Some(act)) = (hid, on_action.as_ref()) {
                            let act = Rc::clone(act);
                            row_el = mouse_area(row_el)
                                .on_enter(act(Msg::Hover(Some(id))))
                                .into();
                        }
                        body_items = body_items.push(row_el);
                        row_index += 1;
                    }
                }
                for item in visible_items {
                    if collapsed {
                        body_items = body_items.push(collapsed_row(&item, row_index, reorder_ref));
                    } else {
                        let item = assign_close_id(item, row_index);
                        let item_id = item.id.clone();
                        let show_action = item_id
                            .as_ref()
                            .is_some_and(|id| hovered_id.as_ref() == Some(id));
                        let mut row_el = render_item(
                            item,
                            reorder_ref,
                            row_index,
                            show_action,
                            density,
                            true,
                            false,
                        );
                        // Enter only — list-level exit clears hover so A→B
                        // cannot race (exit A after enter B → stuck None).
                        if let (Some(id), Some(act)) = (item_id, on_action.as_ref()) {
                            let act = Rc::clone(act);
                            row_el = mouse_area(row_el)
                                .on_enter(act(Msg::Hover(Some(id))))
                                .into();
                        }
                        body_items = body_items.push(row_el);
                    }
                    row_index += 1;
                }

                if wants_fill {
                    // First fill section owns app-driven scroll + chips
                    // *when* `section_scroll` is wired. Without a callback
                    // (browser / terminal tab strips), use a hidden iced
                    // scrollbar — no `↓ N` chip against a fake viewport.
                    let scroll_cb = on_section_scroll.take();
                    let fill_col = if is_collapsible {
                        column![wrap_group_well(body_items, true)].width(Length::Fill)
                    } else {
                        body_items
                    };
                    let mut body = fill_section_body(
                        fill_col,
                        n_in_section,
                        content_h,
                        scroll_snap,
                        scroll_cb,
                    );
                    if let Some(act) = on_action.as_ref() {
                        let act = Rc::clone(act);
                        body = mouse_area(body).on_exit(act(Msg::Hover(None))).into();
                    }
                    sections_col = sections_col.push(body);
                } else {
                    let body_el: Element<'a, Message> = if is_collapsible {
                        wrap_group_well(body_items, true)
                    } else {
                        body_items.into()
                    };
                    let body: Element<'a, Message> = if let Some(act) = on_action.as_ref() {
                        let act = Rc::clone(act);
                        mouse_area(body_el).on_exit(act(Msg::Hover(None))).into()
                    } else {
                        body_el
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
                chrome = chrome.push(container(footer).padding(Padding::from([8.0, 10.0])));
            }
        }

        let width = match &resize {
            Some((w, _)) if !collapsed => Length::Fixed(*w),
            _ if collapsed => Length::Fixed(36.0),
            _ if fill_width => Length::Fill,
            _ => Length::Fixed(SIDEBAR_WIDTH),
        };

        let panel = container(chrome)
            .style(style)
            .width(width)
            .height(Length::Fill);

        let capturing = controller.as_ref().is_some_and(|(s, _)| s.capturing());
        let resize_dragging = controller.as_ref().is_some_and(|(s, _)| s.resizing());
        let reorder_dragging = controller.as_ref().is_some_and(|(s, _)| s.reordering());

        // Compose optional resize divider — same three-band hit strip as
        // kit `split` / terminal pane dividers.
        let body: Element<'a, Message, Theme> = match resize {
            Some((w, colors)) => {
                let on_press = on_action
                    .as_ref()
                    .map(|act| act(Msg::PressDivider { width: w }));
                let divider = match (on_press, colors) {
                    (Some(msg), Some(c)) => crate::components::vertical_divider_with(msg, c),
                    (Some(msg), None) => crate::components::vertical_divider(msg),
                    (None, _) => Space::new()
                        .width(Length::Fixed(crate::components::DIVIDER_HIT_PX))
                        .into(),
                };
                row![panel, divider].height(Length::Fill).into()
            }
            None => panel.into(),
        };

        // Cursor chrome only. Move/release come from [`State::subscription`]
        // — the dragged row is an iced `float` overlay and would steal
        // widget-local `on_move` / `on_release`.
        if capturing {
            let interaction = if resize_dragging {
                iced::mouse::Interaction::ResizingColumn
            } else if reorder_dragging {
                iced::mouse::Interaction::Grabbing
            } else {
                iced::mouse::Interaction::Pointer
            };
            stack![
                body,
                mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                    .interaction(interaction),
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
    // No app-owned scroll → hidden iced scrollbar, no overflow chips.
    // A lone section auto-fills so long lists can scroll; inventing a
    // 480px viewport made `↓ N` appear whenever content exceeded that,
    // even when the real pane was taller and every row was visible.
    let Some(on_scroll) = on_scroll else {
        return hidden_scroll(items, None, None).into();
    };

    // Prefer measured content_h; keep any larger viewport hint from sensor.
    let mut scroll = scroll.with_content_h(content_h);
    scroll = scroll.clamped();

    // Unmeasured viewport: hide chips until the sensor reports a height.
    // Do not invent a default pane size — that flashes false `↓ N` chips.
    let (above, below) = if scroll.viewport_h <= 1.0 {
        (0, 0)
    } else {
        section_overflow_counts(scroll, n_items)
    };
    let offset = scroll.offset_y;

    // Unbounded content layout + clip + translate (see [`ClipScroll`]).
    let clipped = ClipScroll {
        content: items.into(),
        offset_y: offset,
    };

    let cb: std::rc::Rc<dyn Fn(SectionScroll) -> Message + 'a> = std::rc::Rc::from(on_scroll);
    let base = scroll;

    let cb_wheel = std::rc::Rc::clone(&cb);
    let area =
        mouse_area(clipped).on_scroll(move |delta: mouse::ScrollDelta| cb_wheel(base.wheel(delta)));

    let cb_show = std::rc::Rc::clone(&cb);
    let cb_resize = std::rc::Rc::clone(&cb);
    let list: Element<'a, Message, Theme> = sensor(area)
        .on_show(move |size: iced::Size| cb_show(base.with_viewport_h(size.height)))
        .on_resize(move |size: iced::Size| cb_resize(base.with_viewport_h(size.height)))
        .into();

    // Chips only take space when there is overflow on that side — no
    // permanent gap under the section title at rest. Click jumps to end.
    let top_chip = overflow_slot(OverflowDir::Up, above, Some(cb(scroll.jump_top())));
    let bottom_chip = overflow_slot(OverflowDir::Down, below, Some(cb(scroll.jump_bottom())));

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
        event: &iced::Event,
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
                .style(move |theme: &Theme| row_container_style(theme, active, chrome, false)),
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
    // Full outline is intentionally off — the storybook / shell draws a
    // single right hairline separator against the content column.
    container::Style {
        background: Some(Background::Color(CHROME_SURFACE)),
        border: Border::default(),
        ..container::Style::default()
    }
}

/// Background style for a row rendered as a non-pressable `container`
/// (the reorder / hover-action path).
///
/// - **Row:** browser etch — idle flat / muted; active inset well on
///   [`CHROME_SURFACE`]; hover wash. No selection-teal.
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
            let muted = p.secondary.base.text;
            let fg = p.background.base.text;
            if active {
                return container::Style {
                    background: Some(Background::Color(inset_surface(CHROME_SURFACE, 0.22))),
                    text_color: Some(fg),
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: 4.0.into(),
                    },
                    ..container::Style::default()
                };
            }
            let (bg, text_color) = if hovered {
                (alpha(p.background.strong.color, 0.45), fg)
            } else {
                (Color::TRANSPARENT, muted)
            };
            container::Style {
                background: Some(Background::Color(bg)),
                text_color: Some(text_color),
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: RADIUS_SM.into(),
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
/// List etch: idle muted + flat; active inset well (no selection wash).
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
        SidebarItemChrome::Row => tab_item_style(theme, status, active),
        SidebarItemChrome::Card => {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
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
    fn density_metrics_are_stable() {
        let n = SidebarDensity::Normal.metrics();
        assert_eq!((n.row_pad_v, n.row_pad_h, n.font, n.close), (6, 10, 13, 14));
        assert_eq!(n.gap, SPACE_XS);

        let l = SidebarDensity::Large.metrics();
        assert_eq!((l.row_pad_v, l.row_pad_h, l.font, l.close), (7, 10, 12, 14));
        assert_eq!(l.gap, 3.0);

        assert_eq!(SidebarDensity::default(), SidebarDensity::Normal);
    }

    #[test]
    fn row_chrome_is_etch_not_selection() {
        let theme = crate::default_theme();
        let row = item_style_chrome(&theme, button::Status::Active, true, SidebarItemChrome::Row);
        match row.background {
            Some(Background::Color(c)) => {
                assert_eq!(c, inset_surface(CHROME_SURFACE, 0.22));
            }
            other => panic!("row active should be solid inset, got {other:?}"),
        }
        assert_eq!(row.border.width, 0.0);
        assert_ne!(
            match row.background {
                Some(Background::Color(c)) => c,
                _ => Color::TRANSPARENT,
            },
            crate::theme::selection()
        );
    }

    #[test]
    fn card_chrome_does_not_collapse_into_etch() {
        let theme = crate::default_theme();
        let card = item_style_chrome(
            &theme,
            button::Status::Active,
            true,
            SidebarItemChrome::Card,
        );
        let row = item_style_chrome(&theme, button::Status::Active, true, SidebarItemChrome::Row);
        // Card keeps a 1px hairline + graphite gradient; etch is inset fill.
        assert_eq!(card.border.width, 1.0);
        assert_eq!(row.border.width, 0.0);
        match card.background {
            Some(Background::Gradient(_)) => {}
            Some(Background::Color(c)) => {
                assert_ne!(c, inset_surface(CHROME_SURFACE, 0.22));
            }
            other => panic!("card active should stay graphite surface, got {other:?}"),
        }
    }

    #[test]
    fn assign_close_id_only_when_needed() {
        let with_close = assign_close_id(SidebarItem::new("x", ()).on_close(()), 3);
        assert_eq!(with_close.id.as_deref(), Some("__row:3"));
        let with_id = assign_close_id(SidebarItem::new("x", ()).on_close(()).id("keep"), 3);
        assert_eq!(with_id.id.as_deref(), Some("keep"));
        let no_close = assign_close_id(SidebarItem::new("x", ()), 3);
        assert_eq!(no_close.id, None);
    }

    #[test]
    fn etch_row_height_is_whole_pixels() {
        let h = panel_etch_row_height(SidebarDensity::Large);
        assert_eq!(h, h.round());
        assert_eq!(h, PANEL_ROW_H);
    }

    #[test]
    fn list_row_height_reserves_etch_lip_when_idle() {
        let idle = SidebarItem::new("tab", ());
        let active = SidebarItem::new("tab", ()).active(true);
        let h_idle = item_row_height(&idle, SidebarDensity::Large);
        let h_active = item_row_height(&active, SidebarDensity::Large);
        assert_eq!(h_idle, h_active);
    }

    #[test]
    fn collapse_header_is_folder_caption() {
        let item = collapse_header_item(
            Some("Work".into()),
            SectionCollapse {
                collapsed: true,
                on_toggle: (),
                header_active: false,
                on_context: None,
                on_edit: None,
                count: Some("3".into()),
                header_content: None,
            },
        );
        assert_eq!(item.label, "Work");
        assert_eq!(item.section_header, Some(true));
        assert_eq!(item.secondary.as_deref(), Some("3"));
    }

    #[test]
    fn header_edit_is_pencil_until_rename_field() {
        let idle = collapse_header_item(
            Some("Work".into()),
            SectionCollapse {
                collapsed: false,
                on_toggle: (),
                header_active: false,
                on_context: None,
                on_edit: Some(()),
                count: None,
                header_content: None,
            },
        );
        assert!(idle.on_edit.is_some());
        let renaming = collapse_header_item(
            Some("Work".into()),
            SectionCollapse {
                collapsed: false,
                on_toggle: (),
                header_active: false,
                on_context: None,
                on_edit: Some(()),
                count: None,
                header_content: Some(iced::widget::text("Work").into()),
            },
        );
        assert!(renaming.on_edit.is_none());
    }

    #[test]
    fn header_rename_stays_list_row_height() {
        let idle = collapse_header_item(
            Some("Work".into()),
            SectionCollapse {
                collapsed: false,
                on_toggle: (),
                header_active: false,
                on_context: None,
                on_edit: None,
                count: None,
                header_content: None,
            },
        );
        let renaming = collapse_header_item(
            Some("Work".into()),
            SectionCollapse {
                collapsed: false,
                on_toggle: (),
                header_active: false,
                on_context: None,
                on_edit: None,
                count: None,
                header_content: Some(iced::widget::text("Work").into()),
            },
        );
        assert_eq!(
            item_row_height(&idle, SidebarDensity::Large),
            item_row_height(&renaming, SidebarDensity::Large)
        );
        assert!(item_row_height(&renaming, SidebarDensity::Large) < CARD_HEIGHT_HINT);
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

    #[test]
    fn overflow_counts_none_when_viewport_unmeasured() {
        // Default SectionScroll (viewport 0) must not invent overflow —
        // a fake 480px pane used to flash `↓ N` on auto-filled tab strips.
        let s = SectionScroll {
            offset_y: 0.0,
            viewport_h: 0.0,
            content_h: 800.0,
        };
        assert_eq!(section_overflow_counts(s, 20), (0, 0));
        assert!(!s.overflows());
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
