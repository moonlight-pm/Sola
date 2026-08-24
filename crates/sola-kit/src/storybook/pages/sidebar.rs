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

/// Kit strip = named groups and singleton items, mixed.
#[derive(Clone)]
enum Block {
    Group(DemoGroup),
    Item(DemoItem),
}

pub struct State {
    pub width: f32,
    pub panel: SidebarState,
    blocks: Vec<Block>,
    selected: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            width: 220.0,
            panel: SidebarState::new(),
            blocks: vec![
                Block::Group(DemoGroup {
                    id: "a".into(),
                    name: "Group A".into(),
                    collapsed: false,
                    items: items(&[
                        ("a1", "Item A1"),
                        ("a2", "Item A2"),
                        ("a3", "Item A3"),
                    ]),
                }),
                Block::Group(DemoGroup {
                    id: "b".into(),
                    name: "Group B".into(),
                    collapsed: false,
                    items: items(&[
                        ("b1", "Item B1"),
                        ("b2", "Item B2"),
                        ("b3", "Item B3"),
                        ("b4", "Item B4"),
                        ("b5", "Item B5"),
                    ]),
                }),
                Block::Group(DemoGroup {
                    id: "c".into(),
                    name: "Group C".into(),
                    collapsed: false,
                    items: items(&[
                        ("c1", "Item C1"),
                        ("c2", "Item C2"),
                        ("c3", "Item C3"),
                        ("c4", "Item C4"),
                    ]),
                }),
                Block::Item(DemoItem {
                    id: "u1".into(),
                    label: "Item U1".into(),
                }),
                Block::Item(DemoItem {
                    id: "u2".into(),
                    label: "Item U2".into(),
                }),
                Block::Item(DemoItem {
                    id: "u3".into(),
                    label: "Item U3".into(),
                }),
                Block::Item(DemoItem {
                    id: "u4".into(),
                    label: "Item U4".into(),
                }),
                Block::Item(DemoItem {
                    id: "u5".into(),
                    label: "Item U5".into(),
                }),
            ],
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
        for block in &mut self.blocks {
            if let Block::Group(g) = block {
                if g.id == id {
                    g.collapsed = !g.collapsed;
                    return;
                }
            }
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
        for i in 0..self.blocks.len() {
            match &mut self.blocks[i] {
                Block::Group(g) => {
                    if let Some(j) = g.items.iter().position(|it| it.id == id) {
                        return Some(g.items.remove(j));
                    }
                }
                Block::Item(it) if it.id == id => {
                    if let Block::Item(it) = self.blocks.remove(i) {
                        return Some(it);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn take_group(&mut self, id: &str) -> Option<DemoGroup> {
        let i = self.blocks.iter().position(|b| matches!(b, Block::Group(g) if g.id == id))?;
        match self.blocks.remove(i) {
            Block::Group(g) => Some(g),
            other => {
                self.blocks.insert(i, other);
                None
            }
        }
    }

    fn insert_singleton(&mut self, at: usize, item: DemoItem) {
        let at = at.min(self.blocks.len());
        self.blocks.insert(at, Block::Item(item));
    }

    fn apply_drop(&mut self, drop: sidebar::Drop) {
        if let Dest::BlockBefore { before } = &drop.dest {
            let Some(g) = self.take_group(&drop.id) else {
                return;
            };
            let at = before
                .as_deref()
                .and_then(|id| {
                    self.blocks.iter().position(|b| match b {
                        Block::Group(x) if x.id == id => true,
                        Block::Item(it) if it.id == id => true,
                        _ => false,
                    })
                })
                .unwrap_or(self.blocks.len());
            self.blocks.insert(at.min(self.blocks.len()), Block::Group(g));
            return;
        }
        if let Dest::Sections(order) = &drop.dest {
            let mut groups: Vec<DemoGroup> = Vec::new();
            let mut rest: Vec<Block> = Vec::new();
            for block in self.blocks.drain(..) {
                match block {
                    Block::Group(g) => groups.push(g),
                    other => rest.push(other),
                }
            }
            let mut next_groups = Vec::new();
            for id in order {
                if let Some(i) = groups.iter().position(|g| g.id == *id) {
                    next_groups.push(groups.remove(i));
                }
            }
            next_groups.extend(groups);
            let mut blocks: Vec<Block> = next_groups.into_iter().map(Block::Group).collect();
            blocks.extend(rest);
            self.blocks = blocks;
            return;
        }
        let Some(item) = self.take_item(&drop.id) else {
            return;
        };
        match drop.dest {
            Dest::Join { section, before } => {
                if let Some(Block::Group(g)) = self
                    .blocks
                    .iter_mut()
                    .find(|b| matches!(b, Block::Group(g) if g.id == section))
                {
                    let at = before
                        .as_deref()
                        .and_then(|b| g.items.iter().position(|it| it.id == b))
                        .unwrap_or(g.items.len());
                    g.items.insert(at.min(g.items.len()), item);
                } else {
                    self.blocks.push(Block::Item(item));
                }
            }
            Dest::Loose { before } => {
                let at = before
                    .as_deref()
                    .and_then(|bid| {
                        self.blocks.iter().position(|b| match b {
                            Block::Item(it) => it.id == bid,
                            _ => false,
                        })
                    })
                    .unwrap_or(self.blocks.len());
                self.insert_singleton(at, item);
            }
            Dest::BeforeGroup { id } => {
                let at = self
                    .blocks
                    .iter()
                    .position(|b| matches!(b, Block::Group(g) if g.id == id))
                    .unwrap_or(self.blocks.len());
                self.insert_singleton(at, item);
            }
            Dest::BlockBefore { .. } | Dest::Sections(_) => {}
        }
    }
}

pub fn view<'a>(state: &'a State, theme: &Theme) -> Element<'a, Msg> {
    let mut sections = Vec::new();
    for block in &state.blocks {
        match block {
            Block::Group(g) => {
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
            Block::Item(it) => {
                let row = SidebarItem::new(it.label.clone(), Msg::Select(it.id.clone()))
                    .id(it.id.clone())
                    .active(state.selected == it.id);
                sections.push(SidebarSection::unlabeled(vec![row]));
            }
        }
    }

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
    .height(Length::Fixed(900.0))
    .width(Length::Fill);

    column![
        heading("Sidebar"),
        body(
            "Morphing-hole reorder (Scratch morph2). Click a group title \
             to fold it; drag it to move the whole group. 2px before the \
             drag starts."
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
        let items: Vec<SidebarItem<Msg>> =
            ["Item A1", "A long tab title that truncates", "Item U1"]
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
