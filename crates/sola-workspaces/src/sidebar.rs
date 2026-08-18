//! Project groups + workspace rows. `+` on the group opens a name modal.

use iced::widget::{column, container, mouse_area, row};
use iced::{Alignment, Element, Length, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::card;
use sola_kit::components::field::field;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{
    DividerColors, SidebarItem, SidebarPanel, SidebarSection,
};

use crate::Msg;
use crate::spawn;
use crate::workspace::{self, Kind, Project, Workspace};

pub const SIDEBAR_W_DEFAULT: f32 = 240.0;
pub const SPAWN_INPUT_ID: &str = "ws-spawn-name";
pub const ADD_INPUT_ID: &str = "ws-add-path";

pub struct SidebarState {
    pub width: f32,
    pub dragging_divider: bool,
    pub drag_anchor: Option<(f32, f32)>,
    pub hovered: Option<String>,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            width: SIDEBAR_W_DEFAULT,
            dragging_divider: false,
            drag_anchor: None,
            hovered: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SpawnDraft {
    pub project_id: Option<String>,
    pub name: String,
    pub error: Option<String>,
}

impl SpawnDraft {
    pub fn open(project_id: impl Into<String>) -> Self {
        Self {
            project_id: Some(project_id.into()),
            name: String::new(),
            error: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.project_id.is_some()
    }
}

#[derive(Clone, Debug, Default)]
pub struct AddDraft {
    pub open: bool,
    pub path: String,
    pub error: Option<String>,
}

pub fn view<'a>(
    state: &'a SidebarState,
    projects: &'a [Project],
    workspaces: &'a [Workspace],
    selected: &str,
    theme: &Theme,
    term_bg: iced::Color,
) -> Element<'a, Msg> {
    let mut sections = Vec::new();
    for project in projects {
        let mut items = Vec::new();
        if !project.collapsed {
            for ws in workspace::ordered_for_project(&project.id, workspaces) {
                let title = if ws.kind == Kind::Main {
                    "root".to_string()
                } else {
                    ws.name.clone()
                };
                let mut item = SidebarItem::new(title, Msg::SelectWorkspace(ws.id.clone()))
                    .active(ws.id == selected)
                    .indicator(ws.status.indicator())
                    .id(ws.id.clone());
                // Kit list `on_close` — hover × (lucide/x), vertically
                // centered. Not `hover_action` (session-card trash).
                // Root stays; close the project when we have that verb.
                if workspace::can_close(ws) {
                    item = item.on_close(Msg::CloseWorkspace(ws.id.clone()));
                }
                items.push(item);
            }
        }
        let mark = if project.collapsed { "▸ " } else { "" };
        sections.push(
            SidebarSection::new(format!("{mark}{}", project.name), items)
                .on_label(Msg::ToggleProject(project.id.clone()))
                .on_add(Msg::OpenSpawn(project.id.clone())),
        );
    }

    let p = theme.extended_palette();
    let divider = DividerColors {
        a: p.background.weaker.color,
        line: p.background.stronger.color,
        b: term_bg,
    };

    let mut panel =
        SidebarPanel::new(sections).item_hover(state.hovered.clone(), Msg::HoverSidebar);
    if projects.is_empty() {
        panel = panel.footer(empty_footer());
    }
    panel
        .resizable_with(
            state.width,
            state.dragging_divider,
            Msg::SidebarDragStart,
            divider,
        )
        .build()
}

fn empty_footer<'a>() -> Element<'a, Msg> {
    container(kit_btn::labeled_sm("Add project", kit_btn::ghost).on_press(Msg::OpenAdd))
        .padding(10)
        .width(Length::Fill)
        .into()
}

/// Dim overlay + name-only spawn card. Click the veil to dismiss.
pub fn overlay<'a>(
    spawn: &'a SpawnDraft,
    add: &'a AddDraft,
    projects: &'a [Project],
) -> Option<Element<'a, Msg>> {
    if spawn.is_open() {
        let project_name = spawn
            .project_id
            .as_ref()
            .and_then(|id| workspace::find_project(projects, id))
            .map(|p| p.name.as_str())
            .unwrap_or("project");
        return Some(veil(spawn_card(spawn, project_name)));
    }
    if add.open {
        return Some(veil(add_card(add)));
    }
    None
}

fn veil<'a>(card: Element<'a, Msg>) -> Element<'a, Msg> {
    mouse_area(
        container(mouse_area(card).on_press(Msg::Ignore))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(24)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgba(
                    0.0, 0.0, 0.0, 0.45,
                ))),
                ..container::Style::default()
            }),
    )
    .on_press(Msg::DismissDialog)
    .into()
}

fn spawn_card<'a>(draft: &'a SpawnDraft, project_name: &str) -> Element<'a, Msg> {
    let slug = spawn::slug(&draft.name);
    let hint = if slug.is_empty() {
        format!("{}/<name>", spawn::WORKTREE_DIR)
    } else {
        format!("{}/{}", spawn::WORKTREE_DIR, slug)
    };
    card::modal(
        container(
            column![
                sola_kit::components::text::body(format!("New workspace · {project_name}")),
                field(
                    "Name",
                    text_input("kvm-perf", &draft.name)
                        .id(iced::widget::Id::new(SPAWN_INPUT_ID))
                        .on_input(Msg::SpawnName)
                        .on_submit(Msg::Spawn),
                    None,
                    draft.error.as_deref(),
                )
                .padding(0),
                sola_kit::components::text::caption(hint).style(sola_kit::components::text::muted),
                row![
                    iced::widget::Space::new().width(Length::Fill),
                    kit_btn::labeled_sm("Cancel", kit_btn::ghost).on_press(Msg::DismissDialog),
                    kit_btn::labeled_sm("Create", kit_btn::primary).on_press(Msg::Spawn),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(12),
        )
        .padding(18)
        .width(Length::Fixed(320.0)),
    )
    .width(Length::Shrink)
    .into()
}

fn add_card<'a>(draft: &'a AddDraft) -> Element<'a, Msg> {
    card::modal(
        container(
            column![
                sola_kit::components::text::body("Add project"),
                field(
                    "Folder",
                    text_input("~/path/to/checkout", &draft.path)
                        .id(iced::widget::Id::new(ADD_INPUT_ID))
                        .on_input(Msg::AddPath)
                        .on_submit(Msg::AddProject),
                    None,
                    draft.error.as_deref(),
                )
                .padding(0),
                sola_kit::components::text::caption(
                    "A git checkout. Workspaces land in .worktrees/"
                )
                .style(sola_kit::components::text::muted),
                row![
                    iced::widget::Space::new().width(Length::Fill),
                    kit_btn::labeled_sm("Cancel", kit_btn::ghost).on_press(Msg::DismissDialog),
                    kit_btn::labeled_sm("Add", kit_btn::primary).on_press(Msg::AddProject),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(12),
        )
        .padding(18)
        .width(Length::Fixed(360.0)),
    )
    .width(Length::Shrink)
    .into()
}
