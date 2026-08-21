//! Sidebar showcase — dogfoods [`SidebarPanel`] with collapse, resize,
//! reorder, shortcuts, close, and a collapsible group pocket.
//!
//! Gesture state lives in kit [`SidebarState`]. This page only applies
//! [`SidebarEvent`].

use iced::widget::{button, column, container, row, text};
use iced::{Element, Length, Theme};

use sola_kit::components::card::style as card_style;
use sola_kit::components::sidebar::{self, Dest, Event as SidebarEvent};
use sola_kit::components::text::{body, heading, muted};
use sola_kit::components::{
    DividerColors, SectionScroll, SidebarDensity, SidebarIndicator, SidebarItem, SidebarPanel,
    SidebarSection, SidebarState,
};

const ITEMS: [&str; 5] = ["Inbox", "Drafts", "Sent", "Archive", "Spam"];

#[derive(Clone, Debug)]
pub enum Msg {
    Toggle,
    Panel(sidebar::Msg),
    SectionScroll(SectionScroll),
    ItemPress(usize),
    ToggleGroup,
    Noop,
    MarkTick,
}

pub struct State {
    pub collapsed: bool,
    pub width: f32,
    pub panel: SidebarState,
    pub order: Vec<usize>,
    pub selected: usize,
    pub section_scroll: SectionScroll,
    pub group_collapsed: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            collapsed: false,
            width: 200.0,
            panel: SidebarState::new(),
            order: (0..ITEMS.len()).collect(),
            selected: 0,
            section_scroll: SectionScroll::default(),
            group_collapsed: false,
        }
    }
}

impl State {
    pub fn subscription(&self) -> iced::Subscription<Msg> {
        self.panel.subscription().map(Msg::Panel)
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Toggle => self.collapsed = !self.collapsed,
            Msg::Panel(m) => {
                if let Some(ev) = self.panel.update(m) {
                    self.on_event(ev);
                }
            }
            Msg::ItemPress(index) => {
                if let Some(&item) = self.order.get(index) {
                    self.selected = item;
                }
            }
            Msg::SectionScroll(s) => self.section_scroll = s,
            Msg::ToggleGroup => self.group_collapsed = !self.group_collapsed,
            Msg::Noop | Msg::MarkTick => {}
        }
    }

    fn on_event(&mut self, ev: SidebarEvent) {
        match ev {
            SidebarEvent::Activate { id } => {
                if let Ok(item) = id.parse::<usize>() {
                    self.selected = item;
                }
            }
            SidebarEvent::ToggleSection { .. } => {
                self.group_collapsed = !self.group_collapsed;
            }
            SidebarEvent::Resize { width } => self.width = width,
            SidebarEvent::Drop(drop) => self.apply_drop(drop),
        }
    }

    fn apply_drop(&mut self, drop: sidebar::Drop) {
        let Ok(dragged) = drop.id.parse::<usize>() else {
            return;
        };
        self.order.retain(|&i| i != dragged);
        let insert = match drop.dest {
            Dest::Join {
                before: Some(next), ..
            }
            | Dest::Loose { before: Some(next) } => next
                .parse::<usize>()
                .ok()
                .and_then(|p| self.order.iter().position(|&i| i == p))
                .unwrap_or(self.order.len()),
            Dest::Join { before: None, .. } | Dest::Loose { before: None } => self.order.len(),
            Dest::Sections(_) => {
                self.order.insert(self.order.len(), dragged);
                return;
            }
        };
        self.order.insert(insert.min(self.order.len()), dragged);
    }
}

pub fn view<'a>(state: &'a State, theme: &Theme) -> Element<'a, Msg> {
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
            .id("work")
            .collapsible(state.group_collapsed, Msg::ToggleGroup)
            .header_count(n_work)
            .header_context(Msg::Noop),
        SidebarSection::unlabeled(loose).fill(),
    ];

    let divider = DividerColors::raised(theme);

    let panel = SidebarPanel::new(sections)
        .density(SidebarDensity::Normal)
        .controller(&state.panel, Msg::Panel)
        .collapsible(state.collapsed, Msg::Toggle)
        .resizable_with(state.width, divider)
        .reorderable()
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
             measured. Gesture and animation live in the kit."
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
        density_demo(),
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

fn density_demo<'a>() -> Element<'a, Msg> {
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
