//! Project → workspace rail.
//!
//! Operate: one scan column. Project name is a quiet section header.
//! Every workspace row carries a reserved status mark. Demo rows are
//! labeled as such — they exist so the column can be scanned before
//! hooks exist.

use iced::{Element, Theme};
use sola_kit::components::{
    DividerColors, SidebarItem, SidebarPanel, SidebarSection,
};

use crate::workspace::{Project, Workspace};
use crate::Msg;

pub const SIDEBAR_W_DEFAULT: f32 = 240.0;

pub struct SidebarState {
    pub width: f32,
    pub dragging_divider: bool,
    pub drag_anchor: Option<(f32, f32)>,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            width: SIDEBAR_W_DEFAULT,
            dragging_divider: false,
            drag_anchor: None,
        }
    }
}

pub fn view<'a>(
    state: &'a SidebarState,
    project: &'a Project,
    workspaces: &'a [Workspace],
    selected: &str,
    theme: &Theme,
    term_bg: iced::Color,
) -> Element<'a, Msg> {
    let items: Vec<SidebarItem<Msg>> = workspaces
        .iter()
        .map(|ws| {
            let subtitle = if ws.demo {
                "demo".to_string()
            } else {
                ws.path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string()
            };
            let mut item = SidebarItem::new(
                ws.name.clone(),
                if ws.demo {
                    Msg::Noop
                } else {
                    Msg::SelectWorkspace(ws.id.clone())
                },
            )
            .active(ws.id == selected)
            .indicator(ws.status.indicator())
            .subtitle(subtitle)
            .id(ws.id.clone());
            if let Some(agent) = &ws.agent {
                item = item.secondary(agent.clone());
            }
            item
        })
        .collect();

    let sections = vec![SidebarSection::new(project.name.clone(), items).fill()];

    let p = theme.extended_palette();
    let divider = DividerColors {
        a: p.background.weaker.color,
        line: p.background.stronger.color,
        b: term_bg,
    };

    SidebarPanel::new(sections)
        .resizable_with(
            state.width,
            state.dragging_divider,
            Msg::SidebarDragStart,
            divider,
        )
        .build()
}
