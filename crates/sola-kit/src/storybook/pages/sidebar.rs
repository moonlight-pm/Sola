//! Sidebar showcase — meta-page that dogfoods the kit's [`SidebarPanel`]
//! with every opt-in turned on: collapse, drag-to-resize, drag-reorder,
//! plus per-item shortcut hints / a close button / a secondary label.
//!
//! Stateful so the gestures actually work in the storybook. The app owns
//! the cursor-move/release subscription (see the parent `storybook/mod.rs`
//! wiring) — this page just renders the panel and folds the gesture
//! messages into its own `State`.

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Theme};

use sola_kit::components::card::style as card_style;
use sola_kit::components::text::{body, heading, muted};
use sola_kit::components::{
    DividerColors, ReorderAnim, ReorderCfg, SectionScroll, SidebarDensity, SidebarIndicator,
    SidebarItem, SidebarPanel, SidebarSection, panel_dragged_width,
};

/// The demo item labels, in their current (reorderable) order.
const ITEMS: [&str; 5] = ["Inbox", "Drafts", "Sent", "Archive", "Spam"];

#[derive(Clone, Debug)]
pub enum Msg {
    /// Toggle the collapse/expand state.
    Toggle,
    /// Resize divider pressed — begin a width drag.
    DividerPress,
    /// Cursor moved during a width drag (carries cursor x).
    DividerMove(f32),
    /// Width drag released.
    DividerRelease,
    /// Row `usize` pressed — begin a potential reorder gesture.
    ReorderStart(usize),
    /// Cursor moved during a reorder gesture (carries cursor y).
    ReorderMove(f32),
    /// Reorder gesture released — commit the drop (or treat as a click).
    ReorderEnd,
    /// Animation tick while a reorder drag is live (sibling glides).
    ReorderTick,
    /// Fill-section scroll viewport (overflow chips).
    SectionScroll(SectionScroll),
    /// A plain row click (collapsed buttons use this).
    ItemPress(usize),
    /// Hovered list item id (for hover-only ×).
    ItemHover(Option<String>),
    /// Collapse the demo group section.
    ToggleGroup,
    /// Demo placeholder (e.g. a close button) with no modelled effect.
    Noop,
    /// Working-ring animation tick (parent frames subscription).
    MarkTick,
}

pub struct State {
    pub collapsed: bool,
    pub width: f32,
    pub dragging: bool,
    /// `(cursor_x, width)` captured on the first `DividerMove` after a
    /// press, mirroring the monitor/terminal anchor-on-first-move pattern.
    pub drag_anchor: Option<(f32, f32)>,
    /// `Some((from_index, start_y))` while a reorder gesture is active.
    pub reorder: Option<(usize, f32)>,
    /// Last cursor-y seen during a reorder gesture.
    pub reorder_cursor_y: f32,
    /// True once a reorder gesture passes the movement threshold. Gates the
    /// drag chrome so a plain click never flashes a drop highlight.
    pub reorder_dragging: bool,
    /// Sibling glide offsets while a reorder drag is live.
    pub reorder_anim: ReorderAnim,
    /// Current item order (indices into [`ITEMS`]).
    pub order: Vec<usize>,
    /// Selected row (by item index), for the active highlight.
    pub selected: usize,
    /// Fill-section scroll snapshot for overflow chips.
    pub section_scroll: SectionScroll,
    /// Hovered item id (close-on-hover + hover_action).
    pub hovered: Option<String>,
    /// Demo collapsible section (tab-group header).
    pub group_collapsed: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            collapsed: false,
            width: 200.0,
            dragging: false,
            drag_anchor: None,
            reorder: None,
            reorder_cursor_y: 0.0,
            reorder_dragging: false,
            reorder_anim: ReorderAnim::new(),
            order: (0..ITEMS.len()).collect(),
            selected: 0,
            section_scroll: SectionScroll::default(),
            hovered: None,
            group_collapsed: false,
        }
    }
}

impl State {
    /// True while a gesture needs the global cursor subscription — used by
    /// the parent to gate its `event::listen_with` listener.
    pub fn needs_cursor_subscription(&self) -> bool {
        self.dragging || self.reorder.is_some()
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Toggle => self.collapsed = !self.collapsed,
            Msg::DividerPress => {
                self.dragging = true;
                self.drag_anchor = None; // captured on first move
            }
            Msg::DividerMove(cursor_x) => {
                if self.dragging {
                    if let Some((anchor_x, anchor_w)) = self.drag_anchor {
                        self.width = panel_dragged_width(anchor_x, anchor_w, cursor_x);
                    } else {
                        self.drag_anchor = Some((cursor_x, self.width));
                    }
                }
            }
            Msg::DividerRelease => {
                self.dragging = false;
                self.drag_anchor = None;
            }
            Msg::ReorderStart(index) => {
                // start_y = 0.0 sentinel; captured on first ReorderMove. The
                // drag isn't "live" until it passes the movement threshold.
                self.reorder = Some((index, 0.0));
                self.reorder_cursor_y = 0.0;
                self.reorder_dragging = false;
                self.reorder_anim.clear();
            }
            Msg::ReorderMove(cursor_y) => {
                if let Some((_, ref mut start_y)) = self.reorder {
                    if *start_y == 0.0 {
                        *start_y = cursor_y;
                    }
                    self.reorder_cursor_y = cursor_y;
                    // Promote to a live drag once the cursor moves past the
                    // threshold — until then it stays a candidate click.
                    if (cursor_y - *start_y).abs() >= sola_kit::components::PANEL_REORDER_THRESHOLD
                    {
                        self.reorder_dragging = true;
                    }
                    if self.reorder_dragging {
                        self.sync_reorder_anim();
                    }
                }
            }
            Msg::ReorderTick => {
                self.sync_reorder_anim();
            }
            Msg::ReorderEnd => {
                let gesture = self.reorder.take();
                let final_y = self.reorder_cursor_y;
                let was_dragging = self.reorder_dragging;
                self.reorder_cursor_y = 0.0;
                self.reorder_dragging = false;
                self.reorder_anim.clear();
                let Some((from, start_y)) = gesture else {
                    return;
                };

                // Never crossed the threshold → it was a click, not a drag:
                // select the row instead of reordering.
                if !was_dragging {
                    if let Some(&item) = self.order.get(from) {
                        self.selected = item;
                    }
                    return;
                }

                let n = self.order.len();
                // Anchor-relative: the grabbed row shifted by how many
                // row-heights the cursor travelled (no absolute geometry).
                let to = sola_kit::components::panel_drop_index_relative(
                    from,
                    start_y,
                    final_y,
                    sola_kit::components::PANEL_ROW_H,
                    n,
                );
                if from == to {
                    return;
                }
                // Reorder by stringified index (the helper works on ids).
                let ids: Vec<String> = self.order.iter().map(|i| i.to_string()).collect();
                let new_ids = sola_kit::components::panel_reordered(&ids, from, to);
                self.order = new_ids
                    .iter()
                    .filter_map(|s| s.parse::<usize>().ok())
                    .collect();
            }
            Msg::ItemPress(index) => {
                if let Some(&item) = self.order.get(index) {
                    self.selected = item;
                }
            }
            Msg::SectionScroll(s) => {
                self.section_scroll = s;
            }
            Msg::ItemHover(id) => self.hovered = id,
            Msg::ToggleGroup => self.group_collapsed = !self.group_collapsed,
            Msg::Noop | Msg::MarkTick => {}
        }
    }

    fn sync_reorder_anim(&mut self) {
        let Some((from, start_y)) = self.reorder else {
            return;
        };
        if !self.reorder_dragging {
            return;
        }
        let n = self.order.len();
        if n == 0 {
            return;
        }
        let to = sola_kit::components::panel_drop_index_relative(
            from,
            start_y,
            self.reorder_cursor_y,
            sola_kit::components::PANEL_ROW_H,
            n,
        );
        self.reorder_anim
            .sync(from, to, n, iced::time::Instant::now());
    }
}

pub fn view<'a>(state: &'a State, theme: &Theme) -> Element<'a, Msg> {
    // Build one section of items in the current order, decorating each
    // with a shortcut hint, and demoing a close button + a secondary
    // label on two of them.
    let items: Vec<SidebarItem<Msg>> = state
        .order
        .iter()
        .enumerate()
        .map(|(row_i, &item)| {
            let label = ITEMS[item];
            let mut si = SidebarItem::new(label, Msg::ItemPress(row_i))
                .id(item.to_string())
                .active(item == state.selected)
                .shortcut((item + 1) as u8);
            if label == "Drafts" {
                si = si.secondary("3").on_context(Msg::Noop);
            }
            if label == "Spam" {
                si = si.on_close(Msg::Noop);
            }
            si
        })
        .collect();

    // Settings-style headed section + a tab-group pocket + a loose run
    // so membership (inset well, nested rows) is obvious against the
    // unlabeled stack underneath.
    let mut mailboxes = Vec::new();
    let mut work = Vec::new();
    let mut loose = Vec::new();
    for it in items {
        match it.label.as_str() {
            "Inbox" | "Drafts" => mailboxes.push(it),
            "Sent" | "Archive" => work.push(it),
            _ => loose.push(it),
        }
    }
    let n_work = work.len();
    let sections = vec![
        SidebarSection::new("Mailboxes", mailboxes),
        SidebarSection::new("Work", work)
            .collapsible(state.group_collapsed, Msg::ToggleGroup)
            .header_count(n_work)
            .header_context(Msg::Noop),
        SidebarSection::unlabeled(loose).fill(),
    ];

    let cfg = ReorderCfg {
        on_press: Box::new(Msg::ReorderStart),
        // Expose the gesture as "active" only once it's a real drag, so the
        // panel shows no drag chrome on a plain (un-moved) press.
        active: if state.reorder_dragging {
            state.reorder
        } else {
            None
        },
        cursor_y: state.reorder_cursor_y,
        anim: state.reorder_dragging.then_some(&state.reorder_anim),
    };

    // Demo sits in a raised card; the sidebar panel is also raised. Match
    // both divider side-bands to raised so only the 1px hairline shows
    // (theme-default canvas bands read as a black/grey/black gutter).
    let divider = DividerColors::raised(theme);

    let panel = SidebarPanel::new(sections)
        .density(SidebarDensity::Normal)
        .item_hover(state.hovered.clone(), Msg::ItemHover)
        .collapsible(state.collapsed, Msg::Toggle)
        .resizable_with(state.width, state.dragging, Msg::DividerPress, divider)
        .reorderable(cfg)
        .section_scroll(state.section_scroll, Msg::SectionScroll)
        .footer(footer())
        .build();

    let demo = container(
        row![panel, filler()]
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .style(card_style)
    .height(Length::Fixed(360.0))
    .width(Length::Fill);

    column![
        heading("Sidebar"),
        body(
            "List etch: muted idle, reserved lip so selected text does not \
             shift, inset active, hover-only × (follows the pointer after \
             a row slides away — no mouse-out needed). Work is a \
             collapsible group pocket (flush members, quiet hairline \
             rim); drag the header to move the whole pocket. Crossing a \
             pocket animates a hole in the source and a row-high slot in \
             the dest so members stay inside the well. Spam sits \
             in the loose run underneath. Right-click Drafts. Overflow \
             chips only when section_scroll is wired and the viewport is \
             measured."
        )
        .style(muted),
        demo,
        heading("Status marks"),
        body(
            "Reserved 12px slot. Working is an accent ring that spins (~0.85s); \
             waiting a warning diamond; done a success check; idle a dim disc. \
             Who stays off the mark."
        )
        .style(muted),
        marks_demo(),
        body("Density — Normal vs Large").style(muted),
        density_demo(state),
    ]
    .spacing(16)
    .into()
}

fn marks_demo<'a>() -> Element<'a, Msg> {
    let rows = [
        ("kvm-perf", "grok", SidebarIndicator::Working, true),
        ("mail-kit", "grok", SidebarIndicator::Waiting, false),
        ("distribution", "grok", SidebarIndicator::Done, false),
        ("main", "", SidebarIndicator::Idle, false),
    ];
    let items: Vec<SidebarItem<Msg>> = rows
        .into_iter()
        .map(|(label, who, mark, active)| {
            let mut item = SidebarItem::new(label, Msg::Noop)
                .active(active)
                .indicator(mark);
            if !who.is_empty() {
                item = item.secondary(who);
            }
            item
        })
        .collect();
    let panel = SidebarPanel::new(vec![SidebarSection::new("Sola", items)]).build();
    container(panel)
        .style(card_style)
        .width(Length::Fixed(260.0))
        .height(Length::Fixed(220.0))
        .into()
}

/// Etch strips at both densities — this is the product language now.
fn density_demo(state: &State) -> Element<'_, Msg> {
    let mk = |density: SidebarDensity| {
        let items: Vec<SidebarItem<Msg>> = ["Inbox", "A long tab title that truncates", "Sent"]
            .into_iter()
            .enumerate()
            .map(|(i, l)| {
                SidebarItem::new(l, Msg::ItemPress(i))
                    .id(format!("dens-{i}"))
                    .active(i == 0)
                    .on_close(Msg::Noop)
            })
            .collect();
        SidebarPanel::new(vec![SidebarSection::unlabeled(items)])
            .density(density)
            .item_hover(state.hovered.clone(), Msg::ItemHover)
            .fill_width()
            .build()
    };
    row![
        column![
            body("Normal").style(muted),
            container(mk(SidebarDensity::Normal))
                .width(Length::Fixed(200.0))
                .height(Length::Fixed(160.0)),
        ]
        .spacing(8),
        column![
            body("Large").style(muted),
            container(mk(SidebarDensity::Large))
                .width(Length::Fixed(200.0))
                .height(Length::Fixed(160.0)),
        ]
        .spacing(8),
    ]
    .spacing(24)
    .into()
}

fn footer<'a>() -> Element<'a, Msg, iced::Theme> {
    button(text("+ New Mailbox"))
        .on_press(Msg::Noop)
        .style(|t, status| sola_kit::components::sidebar::item_style(t, status, false))
        .width(Length::Fill)
        .padding([6, 10])
        .into()
}

fn filler() -> Element<'static, Msg> {
    container(body("Content").style(muted))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
