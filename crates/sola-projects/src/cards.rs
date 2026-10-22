//! The cards over a project window (new tab, add project, startup
//! script) and the pane placeholders.

use iced::widget::{checkbox, column, container, mouse_area, row, text_editor};
use iced::{Alignment, Background, Border, Color, Element, Length, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::card;
use sola_kit::components::field::field;
use sola_kit::components::form::checkbox_style;
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;

use crate::Msg;
use crate::spawn;
use crate::catalog::{self, Project};

pub const SPAWN_INPUT_ID: &str = "ws-spawn-name";
pub const ADD_INPUT_ID: &str = "ws-add-path";

#[derive(Clone, Debug, Default)]
pub struct SpawnDraft {
    pub project_id: Option<String>,
    pub name: String,
    pub error: Option<String>,
    /// Open in the project root (no worktree).
    pub in_root: bool,
    /// Root checkout's branch when the card opened (`None` = detached /
    /// not git). Copy only.
    pub root_branch: Option<String>,
}

impl SpawnDraft {
    pub fn open(project_id: impl Into<String>, root_branch: Option<String>) -> Self {
        Self {
            project_id: Some(project_id.into()),
            root_branch,
            ..Self::default()
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

pub struct StartupDraft {
    pub project_id: Option<String>,
    pub content: text_editor::Content,
}

impl Default for StartupDraft {
    fn default() -> Self {
        Self {
            project_id: None,
            content: text_editor::Content::new(),
        }
    }
}

impl StartupDraft {
    pub fn open(project_id: impl Into<String>, script: &str) -> Self {
        Self {
            project_id: Some(project_id.into()),
            content: text_editor::Content::with_text(script),
        }
    }

    pub fn is_open(&self) -> bool {
        self.project_id.is_some()
    }
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

/// A tab with no pane to show (none attached yet).
pub fn empty_pane<'a>() -> Element<'a, Msg> {
    container(iced::widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The window shown while there is no project yet.
pub fn no_project<'a>() -> Element<'a, Msg> {
    container(
        column![
            sola_kit::components::text::caption("Add a project to open its tabs.")
                .style(sola_kit::components::text::muted),
            kit_btn::labeled_sm("Add project", kit_btn::primary).on_press(Msg::OpenAdd),
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

/// Dim overlay + name-only spawn card. Click the veil to dismiss.
pub fn overlay<'a>(
    spawn: &'a SpawnDraft,
    add: &'a AddDraft,
    startup: &'a StartupDraft,
    projects: &'a [Project],
) -> Option<Element<'a, Msg>> {
    if spawn.is_open() {
        let project_name = spawn
            .project_id
            .as_ref()
            .and_then(|id| catalog::find_project(projects, id))
            .map(|p| p.name.as_str())
            .unwrap_or("project");
        return Some(veil(spawn_card(spawn, project_name)));
    }
    if add.open {
        return Some(veil(add_card(add)));
    }
    if startup.is_open() {
        let project_name = startup
            .project_id
            .as_ref()
            .and_then(|id| catalog::find_project(projects, id))
            .map(|p| p.name.as_str())
            .unwrap_or("project");
        return Some(veil(startup_card(startup, project_name)));
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
    let (root_label, hint) = spawn_copy(draft, &slug);
    let in_root = checkbox(draft.in_root)
        .label(root_label)
        .font(fonts::ui())
        .text_size(13)
        .on_toggle(Msg::SpawnInRoot)
        .style(checkbox_style);
    card::modal(
        container(
            column![
                sola_kit::components::text::body(format!("New tab · {project_name}")),
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
                in_root,
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

/// Checkbox label + where-it-lands hint for the new-tab card.
fn spawn_copy(draft: &SpawnDraft, slug: &str) -> (String, String) {
    let label = match draft.root_branch.as_deref() {
        Some(branch) => format!("No worktree — work on {branch} in the root"),
        None => "No worktree — work in the project root".to_string(),
    };
    let hint = if draft.in_root {
        match draft.root_branch.as_deref() {
            Some(branch) => format!("Root checkout · {branch}"),
            None => "Root checkout".to_string(),
        }
    } else if slug.is_empty() {
        format!("{}/<name>", spawn::WORKTREE_DIR)
    } else {
        format!("{}/{}", spawn::WORKTREE_DIR, slug)
    };
    (label, hint)
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
                    "A git checkout. Tabs land in .worktrees/"
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

fn startup_card<'a>(draft: &'a StartupDraft, project_name: &str) -> Element<'a, Msg> {
    let editor = text_editor(&draft.content)
        .placeholder(r#"cp -a "$PROJECT/.grok" "$WORKTREE/""#)
        .height(Length::Fixed(168.0))
        .padding(10)
        .size(13.0)
        .font(fonts::mono())
        .style(startup_editor_style)
        .on_action(Msg::StartupAction);
    let mut vars = column![].spacing(4);
    for v in crate::startup::VARS {
        vars = vars.push(
            row![
                sola_kit::components::text::code(format!("${}", v.name)).width(Length::Fixed(88.0)),
                sola_kit::components::text::caption(v.help)
                    .style(sola_kit::components::text::muted),
            ]
            .spacing(10)
            .align_y(Alignment::Start),
        );
    }
    card::modal(
        container(
            column![
                sola_kit::components::text::body(format!("Startup · {project_name}")),
                sola_kit::components::text::caption(
                    "Project = folder on disk. Worktree = this tab (.worktrees/<name>)."
                )
                .style(sola_kit::components::text::muted),
                sola_kit::components::text::caption(
                    "Runs in the new worktree after spawn. Empty skips."
                )
                .style(sola_kit::components::text::muted),
                editor,
                vars,
                row![
                    iced::widget::Space::new().width(Length::Fill),
                    kit_btn::labeled_sm("Cancel", kit_btn::ghost).on_press(Msg::DismissDialog),
                    kit_btn::labeled_sm("Save", kit_btn::primary).on_press(Msg::SaveStartup),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(12),
        )
        .padding(18)
        .width(Length::Fixed(500.0)),
    )
    .width(Length::Shrink)
    .into()
}

fn startup_editor_style(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let p = theme.extended_palette();
    let border = match status {
        text_editor::Status::Focused { .. } => p.primary.strong.color,
        _ => p.background.strong.color,
    };
    text_editor::Style {
        background: Background::Color(p.background.weak.color),
        border: Border {
            color: border,
            width: 1.0,
            radius: 6.0.into(),
        },
        placeholder: Color {
            a: 0.55,
            ..p.background.base.text
        },
        value: p.background.base.text,
        selection: p.primary.weak.color,
    }
}
