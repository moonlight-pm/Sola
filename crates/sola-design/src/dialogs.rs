//! The two small dialogs (Add project, New canvas) and the body of the
//! window shown while there is no project. Nothing here edits a design.

use iced::widget::{column, container, mouse_area, row};
use iced::{Alignment, Color, Element, Length, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::card;
use sola_kit::components::field::field;
use sola_kit::components::text_input::text_input;

use crate::app::Msg;
use crate::projects::Project;

pub const ADD_INPUT_ID: &str = "design-add-path";
pub const CANVAS_INPUT_ID: &str = "design-canvas-name";

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

/// The no-project window: a word and Add project.
pub fn no_project<'a>() -> Element<'a, Msg> {
    container(
        column![
            sola_kit::components::text::caption("Add a project folder; its canvases live in design/.")
                .style(sola_kit::components::text::muted),
            kit_btn::labeled_sm("Add project", kit_btn::ghost).on_press(Msg::OpenAdd),
        ]
        .spacing(12)
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
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
