//! Project rail: sections are projects, items are canvases. Plus the two
//! small dialogs (Add project, New canvas). Nothing here edits a design.

use std::collections::HashMap;

use iced::widget::{column, container, mouse_area, row};
use iced::{Alignment, Color, Element, Length, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::card;
use sola_kit::components::field::field;
use sola_kit::components::text_input::text_input;
use sola_kit::components::{DividerColors, SidebarItem, SidebarPanel, SidebarSection};

use crate::app::Msg;
use crate::canvas;
use crate::projects::{CanvasRef, Project};

pub const RAIL_W_DEFAULT: f32 = 220.0;
pub const ADD_INPUT_ID: &str = "design-add-path";
pub const CANVAS_INPUT_ID: &str = "design-canvas-name";

pub struct RailState {
    pub width: f32,
    pub gestures: sola_kit::components::SidebarState,
}

impl Default for RailState {
    fn default() -> Self {
        Self {
            width: RAIL_W_DEFAULT,
            gestures: sola_kit::components::SidebarState::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AddDraft {
    pub open: bool,
    pub path: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CanvasDraft {
    pub project: Option<String>,
    pub name: String,
    pub error: Option<String>,
}

pub fn view<'a>(
    state: &'a RailState,
    projects: &'a [Project],
    canvases: &'a HashMap<String, Vec<canvas::Entry>>,
    selected: Option<&CanvasRef>,
    // Canvases an agent is working in (a dim "agent" on the row).
    busy: &std::collections::HashSet<String>,
    theme: &Theme,
    plane_bg: Color,
) -> Element<'a, Msg> {
    let mut sections = Vec::with_capacity(projects.len());
    for project in projects {
        let mut items = Vec::new();
        if !project.collapsed {
            for entry in canvases.get(&project.id).map(Vec::as_slice).unwrap_or(&[]) {
                let r = CanvasRef {
                    project: project.id.clone(),
                    canvas: entry.name.clone(),
                };
                let key = r.key();
                let mut item = SidebarItem::new(entry.title.clone(), Msg::SelectCanvas(key.clone()))
                    .active(selected == Some(&r));
                if busy.contains(&key) {
                    item = item.secondary("agent");
                }
                items.push(item.id(key));
            }
        }
        let mark = if project.collapsed { "▸ " } else { "" };
        sections.push(
            SidebarSection::new(format!("{mark}{}", project.name), items)
                .id(project.id.clone())
                .on_add(Msg::OpenNewCanvas(project.id.clone())),
        );
    }

    let p = theme.extended_palette();
    let divider = DividerColors {
        a: p.background.weaker.color,
        line: p.background.stronger.color,
        b: plane_bg,
    };
    let mut panel = SidebarPanel::new(sections).controller(&state.gestures, Msg::Sidebar);
    if projects.is_empty() {
        panel = panel.footer(
            container(kit_btn::labeled_sm("Add project", kit_btn::ghost).on_press(Msg::OpenAdd))
                .padding(10)
                .width(Length::Fill)
                .into(),
        );
    }
    panel.resizable_with(state.width, divider).build()
}

/// Dim veil + dialog card, if one is open. Click the veil to dismiss.
pub fn dialog<'a>(
    add: &'a AddDraft,
    draft: &'a CanvasDraft,
    projects: &[Project],
) -> Option<Element<'a, Msg>> {
    if add.open {
        return Some(veil(add_card(add)));
    }
    let project = draft.project.as_ref()?;
    let name = projects
        .iter()
        .find(|p| &p.id == project)
        .map(|p| p.name.as_str())
        .unwrap_or("project");
    Some(veil(canvas_card(draft, name)))
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
                background: Some(iced::Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.45))),
                ..container::Style::default()
            }),
    )
    .on_press(Msg::DismissDialog)
    .into()
}

fn add_card(draft: &AddDraft) -> Element<'_, Msg> {
    dialog_card(
        "Add project",
        field(
            "Folder",
            text_input("~/path/to/project", &draft.path)
                .id(iced::widget::Id::new(ADD_INPUT_ID))
                .on_input(Msg::AddPath)
                .on_submit(Msg::AddProject),
            None,
            draft.error.as_deref(),
        )
        .padding(0)
        .into(),
        "Canvases live in design/ inside this folder.".into(),
        ("Add", Msg::AddProject),
    )
}

fn canvas_card<'a>(draft: &'a CanvasDraft, project: &str) -> Element<'a, Msg> {
    let slug = crate::projects::slug(&draft.name);
    let hint = if slug.is_empty() {
        "design/<name>/canvas.toml".to_string()
    } else {
        format!("design/{slug}/canvas.toml")
    };
    dialog_card(
        &format!("New canvas · {project}"),
        field(
            "Name",
            text_input("Settings redesign", &draft.name)
                .id(iced::widget::Id::new(CANVAS_INPUT_ID))
                .on_input(Msg::CanvasName)
                .on_submit(Msg::CreateCanvas),
            None,
            draft.error.as_deref(),
        )
        .padding(0)
        .into(),
        hint,
        ("Create", Msg::CreateCanvas),
    )
}

fn dialog_card<'a>(
    title: &str,
    input: Element<'a, Msg>,
    hint: String,
    (confirm, on_confirm): (&'a str, Msg),
) -> Element<'a, Msg> {
    card::modal(
        container(
            column![
                sola_kit::components::text::body(title.to_string()),
                input,
                sola_kit::components::text::caption(hint).style(sola_kit::components::text::muted),
                row![
                    iced::widget::Space::new().width(Length::Fill),
                    kit_btn::labeled_sm("Cancel", kit_btn::ghost).on_press(Msg::DismissDialog),
                    kit_btn::labeled_sm(confirm, kit_btn::primary).on_press(on_confirm),
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
