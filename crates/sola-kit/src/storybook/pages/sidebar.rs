//! Sidebar showcase — drag dogfood for grouped + ungrouped rows.
//!
//! Gesture state lives in kit [`SidebarState`]. This page applies
//! [`SidebarEvent`] onto named groups (A/B/C) and a loose run.

use iced::widget::{column, container, row};
use iced::{Element, Length, Theme};

use sola_kit::components::card::style as card_style;
use sola_kit::components::sidebar::{self, Dest, Event as SidebarEvent};
use sola_kit::components::text::{body, heading, muted};
use sola_kit::components::{
    DividerColors, SidebarDensity, SidebarIndicator, SidebarItem, SidebarPanel, SidebarSection,
    SidebarState,
};

#[derive(Clone, Debug)]
pub enum Msg {
    Panel(sidebar::Msg),
    Select(String),
    ToggleGroup(String),
    Noop,
    MarkTick,
}

#[derive(Clone)]
struct DemoItem {
    id: String,
    label: String,
}

#[derive(Clone)]
struct DemoGroup {
    id: String,
    name: String,
    collapsed: bool,
    items: Vec<DemoItem>,
}

pub struct State {
    pub width: f32,
    pub panel: SidebarState,
    groups: Vec<DemoGroup>,
    loose: Vec<DemoItem>,
    selected: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            width: 220.0,
            panel: SidebarState::new(),
            groups: vec![
                DemoGroup {
                    id: "a".into(),
                    name: "Group A".into(),
                    collapsed: false,
                    items: items(&[("a1", "Item A1"), ("a2", "Item A2"), ("a3", "Item A3")]),
                },
                DemoGroup {
                    id: "b".into(),
                    name: "Group B".into(),
                    collapsed: false,
                    items: items(&[("b1", "Item B1"), ("b2", "Item B2")]),
                },
                DemoGroup {
                    id: "c".into(),
                    name: "Group C".into(),
                    collapsed: false,
                    items: items(&[("c1", "Item C1"), ("c2", "Item C2")]),
                },
            ],
            loose: items(&[("u1", "Item U1"), ("u2", "Item U2"), ("u3", "Item U3")]),
            selected: "a1".into(),
        }
    }
}

fn items(rows: &[(&str, &str)]) -> Vec<DemoItem> {
    rows.iter()
        .map(|(id, label)| DemoItem {
            id: (*id).into(),
            label: (*label).into(),
        })
        .collect()
}

impl State {
    pub fn subscription(&self) -> iced::Subscription<Msg> {
        self.panel.subscription().map(Msg::Panel)
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Panel(m) => {
                if let Some(ev) = self.panel.update(m) {
                    self.on_event(ev);
                }
            }
            Msg::Select(id) => self.selected = id,
            Msg::ToggleGroup(id) => self.toggle_group(&id),
            Msg::Noop | Msg::MarkTick => {}
        }
    }

    fn toggle_group(&mut self, id: &str) {
        if let Some(g) = self.groups.iter_mut().find(|g| g.id == id) {
            g.collapsed = !g.collapsed;
        }
    }

    fn on_event(&mut self, ev: SidebarEvent) {
        match ev {
            SidebarEvent::Activate { id } => self.selected = id,
            SidebarEvent::ToggleSection { id } => self.toggle_group(&id),
            SidebarEvent::Resize { width } => self.width = width,
            SidebarEvent::Drop(drop) => self.apply_drop(drop),
        }
    }

    fn take_item(&mut self, id: &str) -> Option<DemoItem> {
        for g in &mut self.groups {
            if let Some(i) = g.items.iter().position(|it| it.id == id) {
                return Some(g.items.remove(i));
            }
        }
        self.loose
            .iter()
            .position(|it| it.id == id)
            .map(|i| self.loose.remove(i))
    }

    fn apply_drop(&mut self, drop: sidebar::Drop) {
        if let Dest::Sections(order) = &drop.dest {
            let mut next = Vec::with_capacity(order.len());
            for id in order {
                if let Some(g) = self.groups.iter().find(|g| g.id == *id).cloned() {
                    next.push(g);
                }
            }
            for g in &self.groups {
                if !next.iter().any(|n| n.id == g.id) {
                    next.push(g.clone());
                }
            }
            self.groups = next;
            return;
        }
        let Some(item) = self.take_item(&drop.id) else {
            return;
        };
        match drop.dest {
            Dest::Join { section, before } => {
                if let Some(g) = self.groups.iter_mut().find(|g| g.id == section) {
                    let at = before
                        .as_deref()
                        .and_then(|b| g.items.iter().position(|it| it.id == b))
                        .unwrap_or(g.items.len());
                    g.items.insert(at.min(g.items.len()), item);
                } else {
                    self.loose.push(item);
                }
            }
            Dest::Loose { before } => {
                let at = before
                    .as_deref()
                    .and_then(|b| self.loose.iter().position(|it| it.id == b))
                    .unwrap_or(self.loose.len());
                self.loose.insert(at.min(self.loose.len()), item);
            }
            Dest::Sections(_) => {}
        }
    }
}

pub fn view<'a>(state: &'a State, theme: &Theme) -> Element<'a, Msg> {
    let mut sections = Vec::new();
    for g in &state.groups {
        let n = g.items.len();
        let rows: Vec<SidebarItem<Msg>> = g
            .items
            .iter()
            .map(|it| {
                SidebarItem::new(it.label.clone(), Msg::Select(it.id.clone()))
                    .id(it.id.clone())
                    .active(state.selected == it.id)
            })
            .collect();
        sections.push(
            SidebarSection::new(g.name.clone(), rows)
                .id(g.id.clone())
                .collapsible(g.collapsed, Msg::ToggleGroup(g.id.clone()))
                .header_count(n),
        );
    }
    let loose: Vec<SidebarItem<Msg>> = state
        .loose
        .iter()
        .map(|it| {
            SidebarItem::new(it.label.clone(), Msg::Select(it.id.clone()))
                .id(it.id.clone())
                .active(state.selected == it.id)
        })
        .collect();
    sections.push(SidebarSection::unlabeled(loose).fill());

    let divider = DividerColors::raised(theme);
    let panel = SidebarPanel::new(sections)
        .density(SidebarDensity::Large)
        .controller(&state.panel, Msg::Panel)
        .resizable_with(state.width, divider)
        .reorderable()
        .build();

    let demo = container(
        row![panel, filler()]
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .style(card_style)
    .height(Length::Fixed(520.0))
    .width(Length::Fill);

    column![
        heading("Sidebar"),
        body(
            "Drag dogfood. Groups A, B, C are collapsible pockets; \
             ungrouped items sit in the loose run underneath. Names on \
             the strip are the names to use when reporting a drag. \
             Gesture and animation live in the kit."
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
        let items: Vec<SidebarItem<Msg>> = ["Item A1", "A long tab title that truncates", "Item U1"]
            .into_iter()
            .enumerate()
            .map(|(i, l)| {
                SidebarItem::new(l, Msg::Noop)
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

fn filler() -> Element<'static, Msg> {
    container(body("Content").style(muted))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
