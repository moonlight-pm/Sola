//! Project → workspace rail.
//!
//! Operate: one scan column. Project name is a quiet section header.
//! Workspace rows carry a reserved status mark so later working/waiting/done
//! never shift the title.

use iced::{Element, Theme};
use sola_kit::components::{
    DividerColors, SidebarIndicator, SidebarItem, SidebarPanel, SidebarSection,
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
    workspace: &'a Workspace,
    theme: &Theme,
    term_bg: iced::Color,
) -> Element<'a, Msg> {
    let item = SidebarItem::new(workspace.name.clone(), Msg::Noop)
        .active(true)
        .indicator(SidebarIndicator::Idle)
        .subtitle(
            workspace
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string(),
        )
        .id(workspace.id.clone());

    let sections = vec![SidebarSection::new(project.name.clone(), vec![item]).fill()];

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
