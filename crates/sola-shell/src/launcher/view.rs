//! Launcher window view.
//!
//! Spotlight-like restraint: dim backdrop, single search field, compact
//! result list. Outside-click on the backdrop fires `Msg::CloseLauncher`.
//!
//! Chord wiring (Meta+Space toggle, Escape close, arrows nav, Enter launch)
//! is handled in `on_chord`. Messages are defined on `Msg`.
//!
//! Picking Projects turns the card into the Projects chooser
//! ([`crate::launcher::projects`]): the field scoped to "Projects", the
//! folders of `~/Projects` as rows (open ones say where), then New.
//! Escape steps back to the apps. Picking Browser turns it into the
//! Browser field ([`crate::launcher::web`]): the field scoped to
//! "Browser", what Return opens, then matching visits.

use iced::widget::{Id as WidgetId, column, container, mouse_area, row, stack, text};
use iced::{Alignment, Element, Length, Padding};

use sola_kit::components::{
    button as kit_btn,
    divider::horizontal_divider,
    icon, modal,
    text_input::{style as input_style, text_input},
};
use sola_kit::fonts;

use crate::app::{Msg, Shell};
use crate::launcher::projects::{Chooser, Row};
use crate::launcher::web::{self, WebChooser};
use sola_kit::components::scroll;

/// Stable widget ID for the launcher query input — used to focus it on open.
pub const QUERY_INPUT_ID: &str = "launcher-query";

// ── Density (Spotlight-ish) ───────────────────────────────────────────
const QUERY_SIZE: f32 = 16.0;
const QUERY_PAD: f32 = 12.0;
const ROW_ICON: u16 = 20;
const ROW_LABEL: f32 = 14.0;
const ROW_GAP: f32 = 10.0;
const LIST_SPACING: f32 = 2.0;
const LIST_MAX_H: f32 = 360.0;
const LIST_INSET: f32 = 6.0;
/// A Browser row's address: its type, and the most of the row it takes.
const NOTE_SIZE: f32 = 12.0;
const NOTE_SHARE: f32 = 0.45;

/// Render the launcher overlay for `shell`.
///
/// When inactive the iced window is parked at 2×2; this placeholder is
/// the parked view (and a race if a frame lands after dismiss).
pub fn view(shell: &Shell) -> Element<'_, Msg> {
    if !shell.launcher.active {
        return mouse_area(container(text("")).width(Length::Fill).height(Length::Fill))
            .on_press(Msg::CloseLauncher)
            .into();
    }

    let launcher = &shell.launcher;

    let placeholder = match (&launcher.web, &launcher.chooser) {
        (Some(_), _) => "Search or enter an address",
        (None, Some(c)) if c.naming => "Name the new project",
        (None, Some(_)) => "Open a project, or type a new name",
        (None, None) => "Search…",
    };
    // Search field — part of the card chrome, not a loud form control.
    let query_input: Element<'_, Msg> = text_input(placeholder, &launcher.query)
        .id(WidgetId::new(QUERY_INPUT_ID))
        .on_input(Msg::LauncherQuery)
        .style(input_style)
        .padding(Padding::new(QUERY_PAD))
        .size(QUERY_SIZE)
        .width(Length::Fill)
        .into();
    // In the chooser, the field is scoped: "Projects" before it.
    let scope = match (&launcher.web, &launcher.chooser) {
        (Some(_), _) => Some(("lucide/globe", "Browser")),
        (None, Some(_)) => Some(("lucide/folders", "Projects")),
        (None, None) => None,
    };
    let query_input: Element<'_, Msg> = match scope {
        Some((icon_name, label)) => row![scope_chip(icon_name, label), query_input]
            .padding(Padding::ZERO.left(QUERY_PAD))
            .align_y(Alignment::Center)
            .into(),
        None => query_input,
    };

    let rows: Vec<Element<'_, Msg>> = if let Some(web) = &launcher.web {
        web_rows(shell, web)
    } else if let Some(chooser) = &launcher.chooser {
        chooser_rows(shell, chooser)
    } else if launcher.filtered_ids.is_empty() {
        vec![
            container(
                text("No matching applications.")
                    .font(fonts::chrome())
                    .size(13)
                    .style(|theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(iced::Color {
                            a: 0.55,
                            ..theme.palette().text
                        }),
                    }),
            )
            .padding(Padding::new(12.0))
            .width(Length::Fill)
            .into(),
        ]
    } else {
        launcher
            .filtered_ids
            .iter()
            .enumerate()
            .map(|(i, app_id)| {
                let app = shell.applications.get(app_id);
                let label_str = app.map(|a| a.label.as_str()).unwrap_or(app_id.as_str());
                let icon_name = app.map(|a| a.icon.as_str()).unwrap_or("lucide/box");
                let is_selected = i == launcher.selected;

                let icon_el: Element<'_, Msg> = icon(icon_name, ROW_ICON);
                let label_el: Element<'_, Msg> =
                    text(label_str).font(fonts::ui()).size(ROW_LABEL).into();

                let row_content: Element<'_, Msg> = row![icon_el, label_el]
                    .spacing(ROW_GAP)
                    .align_y(Alignment::Center)
                    .into();

                // Quiet selection via list_item (selection atom, not accent).
                // Pass the row's app_id — Msg::Launch alone always used the
                // keyboard `selected` index (usually 0), so clicks launched
                // the top row instead of the one under the pointer.
                let lp = shell.style.launcher_pad;
                let id = app_id.clone();
                iced::widget::button(row_content)
                    .on_press(Msg::LaunchApp(id))
                    .padding(Padding {
                        top: lp,
                        bottom: lp,
                        left: lp + 4.0,
                        right: lp + 4.0,
                    })
                    .width(Length::Fill)
                    .style(kit_btn::list_item(is_selected))
                    .into()
            })
            .collect()
    };

    let list: Element<'_, Msg> = scroll::scrollable(column(rows).width(Length::Fill).spacing(LIST_SPACING))
        .width(Length::Fill)
        .height(Length::Fixed(LIST_MAX_H))
        .into();

    // Kit modal: raised panel, calm shadow (Spotlight restraint).
    let card_body: Element<'_, Msg> = column![
        query_input,
        horizontal_divider(),
        container(list).padding(Padding::new(LIST_INSET)),
    ]
    .spacing(0)
    .width(Length::Fill)
    .into();

    let card: Element<'_, Msg> = modal(card_body)
        .width(Length::Fixed(shell.style.launcher_width))
        .into();

    let positioned: Element<'_, Msg> = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();

    // Backdrop dim from shell-backdrop-dim.
    let dim = shell.style.backdrop_dim;
    let backdrop: Element<'_, Msg> = mouse_area(
        container(text(""))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(dim)),
                ..Default::default()
            }),
    )
    .on_press(Msg::CloseLauncher)
    .into();

    stack![backdrop, positioned]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// The stage's name before the field, on a quiet pill.
fn scope_chip<'a>(icon_name: &str, label: &'a str) -> Element<'a, Msg> {
    container(
        row![icon(icon_name, 14), text(label).font(fonts::ui_medium()).size(13)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding(Padding::from([4.0, 8.0]))
    .style(|theme: &iced::Theme| iced::widget::container::Style {
        background: Some(
            iced::Color {
                a: 0.08,
                ..theme.palette().text
            }
            .into(),
        ),
        border: iced::Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

fn dim(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(iced::Color {
            a: 0.55,
            ..theme.palette().text
        }),
    }
}

/// The Browser field's rows: what Return opens (an address, or the first
/// result for a search), Kagi's results page, then visits.
fn web_rows<'a>(shell: &'a Shell, chooser: &'a WebChooser) -> Vec<Element<'a, Msg>> {
    let selected = shell.launcher.selected;
    let query = chooser.query();
    let rows = chooser.rows();
    if rows.is_empty() {
        return vec![
            container(
                text("Type an address, or what to search for")
                    .font(fonts::chrome())
                    .size(13)
                    .style(dim),
            )
            .padding(Padding::new(12.0))
            .width(Length::Fill)
            .into(),
        ];
    }
    rows.into_iter()
        .enumerate()
        .map(|(i, r)| {
            let (icon_name, label, note): (&str, String, Option<String>) = match &r {
                web::Row::Go { url, address: true } => (
                    "lucide/globe",
                    url.split_once("://").map_or(url.as_str(), |(_, rest)| rest).to_string(),
                    Some("Open".into()),
                ),
                web::Row::Go { address: false, .. } => (
                    "lucide/search",
                    format!("“{query}”"),
                    Some("Top result".into()),
                ),
                web::Row::Results(_) => (
                    "lucide/list",
                    format!("Search Kagi for “{query}”"),
                    None,
                ),
                web::Row::Visit(v) => {
                    let visit = &chooser.hits[*v];
                    ("lucide/clock", web::visit_label(visit), Some(web::visit_note(visit)))
                }
            };
            // Both lines end in "…" where they meet: the address takes at
            // most NOTE_SHARE of the row, the title the rest.
            let lp = shell.style.launcher_pad;
            let inner = shell.style.launcher_width
                - LIST_INSET * 2.0
                - (lp + 4.0)
                - (lp + 8.0)
                - ROW_ICON as f32
                - ROW_GAP;
            let note = note.map(|n| fonts::fit_line(&n, NOTE_SIZE, fonts::chrome(), inner * NOTE_SHARE));
            let note_w = note
                .as_deref()
                .map_or(0.0, |n| fonts::line_width(n, NOTE_SIZE, fonts::chrome()) + ROW_GAP);
            let label = fonts::fit_line(&label, ROW_LABEL, fonts::ui(), inner - note_w - 1.0);
            let mut content = row![
                icon(icon_name, ROW_ICON),
                text(label)
                    .font(fonts::ui())
                    .size(ROW_LABEL)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .width(Length::Fill),
            ]
            .spacing(ROW_GAP)
            .align_y(Alignment::Center);
            if let Some(note) = note {
                content = content.push(
                    text(note)
                        .font(fonts::chrome())
                        .size(NOTE_SIZE)
                        .wrapping(iced::widget::text::Wrapping::None)
                        .style(dim),
                );
            }
            iced::widget::button(content)
                .on_press(Msg::LauncherPick(i))
                .padding(Padding {
                    top: lp,
                    bottom: lp,
                    left: lp + 4.0,
                    right: lp + 8.0,
                })
                .width(Length::Fill)
                .style(kit_btn::list_item(i == selected))
                .into()
        })
        .collect()
}

/// The chooser's rows: each folder of `~/Projects` (open ones say where),
/// then New.
fn chooser_rows<'a>(shell: &'a Shell, chooser: &'a Chooser) -> Vec<Element<'a, Msg>> {
    let selected = shell.launcher.selected;
    chooser
        .rows()
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let (icon_name, label, note): (&str, String, Option<String>) = match r {
                Row::Folder(f) => {
                    let folder = &chooser.folders[f];
                    let open = chooser.known(folder).filter(|k| k.open);
                    let note = open.map(|k| match shell.project_workspace(&k.id) {
                        Some(ws) => format!("Open on {ws}"),
                        None => "Open".to_string(),
                    });
                    let icon_name = if open.is_some() {
                        "lucide/folder-open"
                    } else {
                        "lucide/folder"
                    };
                    (icon_name, folder.name.clone(), note)
                }
                Row::New => {
                    let name = shell.launcher.query.trim();
                    let label = match (chooser.naming, chooser.new_name()) {
                        (true, _) if name.is_empty() => "A name for its folder in ~/Projects".into(),
                        (true, _) => format!("Create ~/Projects/{name}"),
                        (false, Some(n)) => format!("New project “{n}”"),
                        (false, None) => "New project…".into(),
                    };
                    ("lucide/folder-plus", label, None)
                }
            };
            let mut content = row![
                icon(icon_name, ROW_ICON),
                text(label).font(fonts::ui()).size(ROW_LABEL),
            ]
            .spacing(ROW_GAP)
            .align_y(Alignment::Center);
            if let Some(note) = note {
                content = content
                    .push(iced::widget::Space::new().width(Length::Fill))
                    .push(text(note).font(fonts::chrome()).size(12).style(dim));
            }
            let lp = shell.style.launcher_pad;
            iced::widget::button(content)
                .on_press(Msg::LauncherPick(i))
                .padding(Padding {
                    top: lp,
                    bottom: lp,
                    left: lp + 4.0,
                    right: lp + 8.0,
                })
                .width(Length::Fill)
                .style(kit_btn::list_item(i == selected))
                .into()
        })
        .collect()
}
