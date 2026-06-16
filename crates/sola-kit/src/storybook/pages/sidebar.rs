//! Sidebar showcase — meta-page that dogfoods the kit's [`SidebarPanel`]
//! with every opt-in turned on: collapse, drag-to-resize, drag-reorder,
//! plus per-item shortcut hints / a close button / a secondary label.
//!
//! Stateful so the gestures actually work in the storybook. The app owns
//! the cursor-move/release subscription (see the parent `storybook/mod.rs`
//! wiring) — this page just renders the panel and folds the gesture
//! messages into its own `State`.

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length};

use sola_kit::components::card::style as card_style;
use sola_kit::components::text::{body, code, heading, muted};
use sola_kit::components::{
    ReorderCfg, SidebarItem, SidebarPanel, SidebarSection, panel_dragged_width,
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
    /// A plain row click (collapsed buttons use this).
    ItemPress(usize),
    /// Demo placeholder (e.g. a close button) with no modelled effect.
    Noop,
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
    /// Current item order (indices into [`ITEMS`]).
    pub order: Vec<usize>,
    /// Selected row (by item index), for the active highlight.
    pub selected: usize,
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
            order: (0..ITEMS.len()).collect(),
            selected: 0,
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
            }
            Msg::ReorderMove(cursor_y) => {
                if let Some((_, ref mut start_y)) = self.reorder {
                    if *start_y == 0.0 {
                        *start_y = cursor_y;
                    }
                    self.reorder_cursor_y = cursor_y;
                    // Promote to a live drag once the cursor moves past the
                    // threshold — until then it stays a candidate click.
                    if (cursor_y - *start_y).abs()
                        >= sola_kit::components::PANEL_REORDER_THRESHOLD
                    {
                        self.reorder_dragging = true;
                    }
                }
            }
            Msg::ReorderEnd => {
                let gesture = self.reorder.take();
                let final_y = self.reorder_cursor_y;
                let was_dragging = self.reorder_dragging;
                self.reorder_cursor_y = 0.0;
                self.reorder_dragging = false;
                let Some((from, start_y)) = gesture else { return };

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
                let ids: Vec<String> =
                    self.order.iter().map(|i| i.to_string()).collect();
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
            Msg::Noop => {}
        }
    }
}

pub fn view(state: &State) -> Element<'_, Msg> {
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
                .active(item == state.selected)
                .shortcut((item + 1) as u8);
            if label == "Drafts" {
                si = si.secondary("3");
            }
            if label == "Spam" {
                si = si.on_close(Msg::Noop);
            }
            si
        })
        .collect();

    let sections = vec![SidebarSection::new("Mailboxes", items)];

    let cfg = ReorderCfg {
        on_press: Box::new(Msg::ReorderStart),
        // Expose the gesture as "active" only once it's a real drag, so the
        // panel shows no drag chrome on a plain (un-moved) press.
        active: if state.reorder_dragging { state.reorder } else { None },
        cursor_y: state.reorder_cursor_y,
    };

    let panel = SidebarPanel::new(sections)
        .collapsible(state.collapsed, Msg::Toggle)
        .resizable(state.width, state.dragging, Msg::DividerPress)
        .reorderable(cfg)
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
        body("The opt-in SidebarPanel: collapse (»/«), drag the right edge to resize, drag rows to reorder. Each row shows a dim shortcut hint; Drafts carries a secondary count, Spam a × close button.")
            .style(muted),
        demo,
        code("SidebarPanel::new(sections).collapsible(..).resizable(..).reorderable(..).build()")
            .style(muted),
    ]
    .spacing(16)
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
