//! The cards over a project window (new tab, add project, startup
//! script) and the pane placeholders.

use iced::widget::{column, container, mouse_area, row};
use iced::{Alignment, Element, Length, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::card;
use sola_kit::components::field::field;
use sola_kit::components::text_input::text_input;

use crate::Msg;
use crate::catalog::{self, Project};

pub const SPAWN_INPUT_ID: &str = "ws-spawn-name";
pub const ADD_INPUT_ID: &str = "ws-new-project";

#[derive(Clone, Debug, Default)]
pub struct SpawnDraft {
    pub project_id: Option<String>,
    pub name: String,
    pub error: Option<String>,
    /// The project is on another computer (an ssh host).
    pub host: Option<String>,
}

impl SpawnDraft {
    pub fn open(project_id: impl Into<String>, host: Option<String>) -> Self {
        Self {
            project_id: Some(project_id.into()),
            host,
            ..Self::default()
        }
    }

    pub fn is_open(&self) -> bool {
        self.project_id.is_some()
    }
}

/// The End Session card: asked before ending a session whose agent is
/// working or waiting on you.
#[derive(Clone, Debug, Default)]
pub struct EndDraft {
    pub tab: Option<String>,
    /// What the card calls it ("Oath › fix login").
    pub label: String,
    pub host: String,
    pub waiting: bool,
}

impl EndDraft {
    pub fn is_open(&self) -> bool {
        self.tab.is_some()
    }
}

#[derive(Clone, Debug, Default)]
/// The New Project card: a name for its folder in `~/Projects`.
pub struct AddDraft {
    pub open: bool,
    pub name: String,
    pub error: Option<String>,
}

/// Dead PTY (Ctrl-D / shell exit). One action: start a new shell.
pub fn exited_pane(pane_id: impl Into<String>) -> Element<'static, Msg> {
    let pane_id = pane_id.into();
    container(
        column![
            sola_kit::components::text::caption("Shell exited.")
                .style(sola_kit::components::text::muted),
            kit_btn::labeled("Start new shell", kit_btn::primary)
                .on_press(Msg::RestartShell(pane_id)),
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

/// A pane on `host` whose ssh ended with its session out of reach: why,
/// and Reconnect (which attaches the session if it is there, else makes
/// a new one).
pub fn unreachable_pane(
    pane_id: impl Into<String>,
    host: &str,
    why: &str,
) -> Element<'static, Msg> {
    let pane_id = pane_id.into();
    container(
        column![
            sola_kit::components::text::body(format!("Can't reach {host}")),
            sola_kit::components::text::caption(why.to_string())
                .style(sola_kit::components::text::muted),
            kit_btn::labeled("Reconnect", kit_btn::primary)
                .on_press(Msg::RestartShell(pane_id)),
        ]
        .spacing(12)
        .max_width(420)
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// A pane on `host` waiting for its link to come up.
pub fn connecting_pane<'a>(host: &str) -> Element<'a, Msg> {
    container(
        sola_kit::components::text::caption(format!("Connecting to {host}…"))
            .style(sola_kit::components::text::muted),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// A tab with no pane to show (none attached yet).
pub fn empty_pane<'a>() -> Element<'a, Msg> {
    container(iced::widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Dim overlay + the open card. Click the veil to dismiss.
pub fn overlay<'a>(
    spawn: &'a SpawnDraft,
    add: &'a AddDraft,
    end: &'a EndDraft,
    projects: &'a [Project],
) -> Option<Element<'a, Msg>> {
    if spawn.is_open() {
        let project_name = spawn
            .project_id
            .as_ref()
            .and_then(|id| catalog::find_project(projects, id))
            .map(catalog::island_name)
            .unwrap_or("project");
        return Some(veil(spawn_card(spawn, project_name)));
    }
    if add.open {
        return Some(veil(add_card(add)));
    }
    if end.is_open() {
        return Some(veil(end_card(end)));
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
    let on = draft
        .host
        .as_deref()
        .map(|h| format!(" on {h}"))
        .unwrap_or_default();
    card::modal(
        container(
            column![
                sola_kit::components::text::body(format!("New session · {project_name}")),
                field(
                    "Name",
                    text_input("kvm perf", &draft.name)
                        .id(iced::widget::Id::new(SPAWN_INPUT_ID))
                        .on_input(Msg::SpawnName)
                        .on_submit(Msg::Spawn),
                    None,
                    draft.error.as_deref(),
                )
                .padding(0),
                sola_kit::components::text::caption(format!(
                    "Starts in the project's folder{on}. It makes a worktree itself when the work needs one."
                ))
                .style(sola_kit::components::text::muted),
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

fn end_card<'a>(draft: &'a EndDraft) -> Element<'a, Msg> {
    let doing = if draft.waiting {
        "Its agent is waiting on you."
    } else {
        "Its agent is working."
    };
    card::modal(
        container(
            column![
                sola_kit::components::text::body(format!("End {}?", draft.label)),
                sola_kit::components::text::caption(format!(
                    "{doing} Ending it stops it on {}, for every computer watching.",
                    draft.host
                ))
                .style(sola_kit::components::text::muted),
                row![
                    iced::widget::Space::new().width(Length::Fill),
                    kit_btn::labeled_sm("Cancel", kit_btn::ghost).on_press(Msg::DismissDialog),
                    kit_btn::labeled_sm("End Session", kit_btn::danger).on_press(Msg::EndConfirmed),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(12),
        )
        .padding(18)
        .width(Length::Fixed(340.0)),
    )
    .width(Length::Shrink)
    .into()
}

fn add_card<'a>(draft: &'a AddDraft) -> Element<'a, Msg> {
    card::modal(
        container(
            column![
                sola_kit::components::text::body("New project"),
                field(
                    "Name",
                    text_input("lantern", &draft.name)
                        .id(iced::widget::Id::new(ADD_INPUT_ID))
                        .on_input(Msg::AddPath)
                        .on_submit(Msg::AddProject),
                    None,
                    draft.error.as_deref(),
                )
                .padding(0),
                sola_kit::components::text::caption(
                    "Its folder in ~/Projects: made, or opened if it is there."
                )
                .style(sola_kit::components::text::muted),
                row![
                    iced::widget::Space::new().width(Length::Fill),
                    kit_btn::labeled_sm("Cancel", kit_btn::ghost).on_press(Msg::DismissDialog),
                    kit_btn::labeled_sm("Create", kit_btn::primary).on_press(Msg::AddProject),
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
