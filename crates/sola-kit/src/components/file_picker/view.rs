//! The panel: title, the crumb trail (and the hidden-files eye), places |
//! files, then the name field and the buttons — with the New Folder row
//! and Save's "replace?" question when they are up.

use std::path::PathBuf;
use std::sync::OnceLock;

use iced::widget::{Space, column, container, mouse_area, row, text, tooltip};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};

use super::fs::{Entry, closest_place, human_size, is_image_name};
use super::{FilePicker, Message, Mode};
use crate::components::button as kit_btn;
use crate::components::card;
use crate::components::icon::{icon_handle, icon_svg};
use crate::components::popover;
use crate::components::scroll;
use crate::components::style::{
    HAIRLINE_A, PAD_CONTROL_SM, RADIUS_MD, RADIUS_SM, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL,
    hairline_on, inset_surface,
};
use crate::components::text as kit_text;
use crate::components::text_input::text_input;
use crate::components::toolbar::toolbar_icon_tip;
use crate::components::{SidebarItem, SidebarPanel, SidebarSection};
use crate::fonts;

const SPACE_XS_LOCAL: f32 = 6.0;
/// Dialog width — not Fill. Overlay centers this; the modal face is Fill inside it.
const PANEL_W: f32 = 720.0;
/// 12px type + [`PAD_CONTROL_SM`] vertical. Fixed so crumb count can't
/// resize the modal (Home vs Home › Downloads).
const CRUMB_H: f32 = 28.0;
/// Places | files.
const BODY_H: f32 = 340.0;
/// A file row, and the gap under it: fixed, so the keyboard's row can be
/// kept in view without measuring.
const ROW_H: f32 = 28.0;
const ROW_GAP: f32 = 1.0;
/// The list's inner padding.
const LIST_PAD: f32 = SPACE_SM;

pub(super) fn list_id() -> iced::widget::Id {
    iced::widget::Id::new("kit-file-picker-list")
}

/// The scroll offset that brings row `row` into the list's view from
/// `scroll_y`, if it is not in view.
pub(super) fn follow_offset(row: usize, scroll_y: f32) -> Option<f32> {
    let view_h = BODY_H - LIST_PAD * 2.0;
    let top = row as f32 * (ROW_H + ROW_GAP);
    let bottom = top + ROW_H;
    if top < scroll_y {
        Some(top)
    } else if bottom > scroll_y + view_h {
        Some(bottom - view_h)
    } else {
        None
    }
}

impl FilePicker {
    /// Dimmed full-window overlay. Stack this over the app: it takes every
    /// click (what is under it gets none).
    pub fn overlay(&self) -> Element<'_, Message, Theme> {
        let dimmed = container(self.view())
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.48,
                    ..Color::BLACK
                })),
                ..container::Style::default()
            });
        iced::widget::opaque(dimmed)
    }

    /// The modal panel (no dim); apps usually want [`Self::overlay`].
    pub fn view(&self) -> Element<'_, Message, Theme> {
        let body = row![self.places_rail(), self.file_list()]
            .spacing(SPACE_MD)
            .height(Length::Fixed(BODY_H));

        let panel = column![
            kit_text::subheading(self.title.clone()),
            self.trail_row(),
            body,
            self.footer(),
        ]
        .spacing(SPACE_LG)
        .padding(SPACE_XL)
        .width(Length::Fill);

        // Width lives on the modal frame. `card::modal` faces are Fill, so
        // a Fixed child is ignored and the overlay would stretch full-bleed.
        card::modal(panel).width(Length::Fixed(PANEL_W)).into()
    }

    fn confirm_label(&self) -> String {
        self.action.clone().unwrap_or_else(|| {
            match self.mode {
                Mode::Open => "Open",
                Mode::Save => "Save",
                Mode::Folder => "Choose",
            }
            .into()
        })
    }

    fn footer(&self) -> Element<'_, Message, Theme> {
        // Save: "replace?" takes the footer until it is answered.
        if let Some(path) = &self.replacing {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            return column![
                kit_text::body(format!("“{name}” already exists. Do you want to replace it?")),
                row![
                    kit_text::caption("Replacing it overwrites what is in it now.")
                        .style(kit_text::muted)
                        .width(Length::Fill),
                    kit_btn::labeled("Cancel", kit_btn::secondary).on_press(Message::KeepFile),
                    kit_btn::labeled("Replace", kit_btn::danger).on_press(Message::Replace),
                ]
                .spacing(SPACE_MD)
                .align_y(Alignment::Center),
            ]
            .spacing(SPACE_SM)
            .into();
        }

        let mut confirm = kit_btn::labeled(self.confirm_label(), kit_btn::primary);
        if self.can_confirm() {
            confirm = confirm.on_press(Message::Confirm);
        }
        let cancel = kit_btn::labeled("Cancel", kit_btn::secondary).on_press(Message::Cancel);

        let mut line = row![].spacing(SPACE_MD).align_y(Alignment::Center);
        if self.mode != Mode::Open {
            line = line.push(
                kit_btn::labeled("New Folder", kit_btn::ghost).on_press(Message::NewFolder),
            );
        }
        match self.mode {
            Mode::Folder => line = line.push(Space::new().width(Length::Fill)),
            Mode::Save | Mode::Open => {
                let placeholder = if self.mode == Mode::Save {
                    "Save As"
                } else {
                    "Name or path"
                };
                line = line.push(
                    container(
                        text_input(placeholder, &self.name)
                            .on_input(Message::NameChanged)
                            .on_submit(Message::Confirm),
                    )
                    .width(Length::Fill),
                );
            }
        }
        line = line.push(cancel).push(confirm);

        let mut footer = column![line].spacing(SPACE_SM);
        if let Some(err) = self.error.as_deref() {
            footer = footer.push(kit_text::caption(err).style(kit_text::danger));
        } else if self.multiple && self.selected.len() > 1 {
            footer = footer.push(
                kit_text::caption(format!("{} chosen", self.selected.len())).style(kit_text::muted),
            );
        } else if let Some(filter) = self.filter.as_ref() {
            footer = footer.push(
                kit_text::caption(format!("Showing {}", filter.label)).style(kit_text::muted),
            );
        }
        footer.into()
    }

    /// The crumbs, and at the right the hidden-files eye.
    fn trail_row(&self) -> Element<'_, Message, Theme> {
        let crumbs = super::breadcrumbs(&self.cwd, self.home.as_deref());
        let last = crumbs.len().saturating_sub(1);
        let mut trail = row![].spacing(SPACE_XS_LOCAL).align_y(Alignment::Center);
        for (i, (label, path)) in crumbs.into_iter().enumerate() {
            if i > 0 {
                trail = trail.push(icon_svg(handle(&CHEVRON, "lucide/chevron-right"), 12));
            }
            // Same chip metrics for current and ancestors so Home (one
            // label) and Downloads (Home › Downloads) share a row height.
            trail = trail.push(crumb_chip(label, (i != last).then_some(path)));
        }
        let (eye, tip) = if self.hidden {
            (handle(&EYE, "lucide/eye"), "Hide hidden files (⌘⇧.)")
        } else {
            (handle(&EYE_OFF, "lucide/eye-off"), "Show hidden files (⌘⇧.)")
        };
        row![
            container(trail)
                .width(Length::Fill)
                .height(Length::Fixed(CRUMB_H))
                .center_y(Length::Fixed(CRUMB_H))
                .clip(true),
            toolbar_icon_tip(eye, tip, Some(Message::ToggleHidden)),
        ]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center)
        .into()
    }

    fn places_rail(&self) -> Element<'_, Message, Theme> {
        let items: Vec<SidebarItem<'_, Message>> = self
            .places
            .iter()
            .enumerate()
            .map(|(i, place)| {
                SidebarItem::new(place.label.clone(), Message::Place(i))
                    .active(closest_place(&self.places, &self.cwd) == Some(i))
            })
            .collect();

        container(
            SidebarPanel::new(vec![SidebarSection::new("Places", items)])
                .fill_width()
                .build(),
        )
        .width(Length::Fixed(168.0))
        .height(Length::Fill)
        .into()
    }

    fn file_list(&self) -> Element<'_, Message, Theme> {
        let mut rows: Vec<Element<'_, Message, Theme>> = Vec::new();
        if let Some(name) = &self.naming_folder {
            rows.push(new_folder_row(name));
        }
        rows.extend(self.entries.iter().enumerate().map(|(i, entry)| {
            let chosen = self.selected.contains(&entry.path);
            file_row(entry, chosen, self.cursor == Some(i) && chosen)
        }));
        let body: Element<'_, Message, Theme> = if rows.is_empty() && self.error.is_none() {
            let empty = if self.mode == Mode::Folder {
                "No folders here"
            } else {
                "This folder is empty"
            };
            container(kit_text::caption(empty).style(kit_text::muted))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .padding(SPACE_XL)
                .into()
        } else {
            scroll::scrollable(column(rows).spacing(ROW_GAP).width(Length::Fill))
                .id(list_id())
                .on_scroll(|v| Message::Scrolled(v.absolute_offset().y))
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(LIST_PAD)
            .style(list_well)
            .into()
    }
}

fn crumb_chip(label: String, target: Option<PathBuf>) -> Element<'static, Message, Theme> {
    let current = target.is_none();
    let label = text(label)
        .font(if current {
            fonts::ui_medium()
        } else {
            fonts::ui()
        })
        .size(12);
    let mut chip = iced::widget::button(label)
        .padding(PAD_CONTROL_SM)
        .style(kit_btn::ghost);
    if let Some(path) = target {
        chip = chip.on_press(Message::Crumb(path));
    }
    chip.into()
}

/// New Folder: a row at the top of the list naming it (Return makes it,
/// Escape drops it).
fn new_folder_row(name: &str) -> Element<'_, Message, Theme> {
    let field = text_input("Folder name", name)
        .on_input(Message::FolderNameChanged)
        .on_submit(Message::CreateFolder)
        .size(13);
    let content = row![
        icon_svg(handle(&FOLDER_PLUS, "lucide/folder-plus"), 14),
        container(field).width(Length::Fill),
        kit_btn::labeled_sm("Create", kit_btn::secondary).on_press(Message::CreateFolder),
        tooltip(
            kit_btn::labeled_sm("Cancel", kit_btn::ghost).on_press(Message::CancelFolder),
            container(kit_text::caption("Escape")).padding(4).style(popover::style),
            tooltip::Position::Bottom,
        ),
    ]
    .spacing(SPACE_MD)
    .align_y(Alignment::Center)
    .padding(Padding::from([SPACE_SM, SPACE_MD]));
    container(content)
        .width(Length::Fill)
        .style(|theme| row_style(theme, true))
        .into()
}

fn file_row(entry: &Entry, chosen: bool, cursor: bool) -> Element<'static, Message, Theme> {
    let icon = if entry.is_dir {
        handle(&FOLDER, "lucide/folder")
    } else if is_image_name(&entry.name) {
        handle(&IMAGE, "lucide/image")
    } else {
        handle(&FILE, "lucide/file")
    };
    let size = entry.size.map(human_size).unwrap_or_default();
    let size_el: Element<'static, Message, Theme> = if size.is_empty() {
        Space::new().width(0).into()
    } else {
        text(size)
            .font(fonts::mono())
            .size(11)
            .style(kit_text::muted)
            .into()
    };
    let content = row![
        icon_svg(icon, 14),
        text(entry.name.clone())
            .font(if cursor { fonts::ui_medium() } else { fonts::ui() })
            .size(13)
            .wrapping(iced::widget::text::Wrapping::None)
            .width(Length::Fill),
        size_el,
    ]
    .spacing(SPACE_MD)
    .align_y(Alignment::Center)
    .padding(Padding::from([0.0, SPACE_MD]));

    let path = entry.path.clone();
    let activate = entry.path.clone();
    mouse_area(
        container(content)
            .width(Length::Fill)
            .height(Length::Fixed(ROW_H))
            .center_y(Length::Fixed(ROW_H))
            .clip(true)
            .style(move |theme| row_style(theme, chosen)),
    )
    .on_press(Message::Select(path))
    .on_double_click(Message::Activate(activate))
    .into()
}

fn row_style(theme: &Theme, selected: bool) -> container::Style {
    let p = theme.extended_palette();
    if selected {
        let fill = inset_surface(p.background.weaker.color, 0.10);
        container::Style {
            background: Some(Background::Color(fill)),
            border: hairline_on(fill, HAIRLINE_A, RADIUS_SM),
            ..container::Style::default()
        }
    } else {
        container::Style {
            background: None,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: RADIUS_SM.into(),
            },
            ..container::Style::default()
        }
    }
}

fn list_well(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    let fill = p.background.base.color;
    container::Style {
        background: Some(Background::Color(fill)),
        border: hairline_on(fill, HAIRLINE_A, RADIUS_MD),
        ..container::Style::default()
    }
}

type Handle = iced::widget::svg::Handle;

static CHEVRON: OnceLock<Handle> = OnceLock::new();
static FOLDER: OnceLock<Handle> = OnceLock::new();
static FOLDER_PLUS: OnceLock<Handle> = OnceLock::new();
static FILE: OnceLock<Handle> = OnceLock::new();
static IMAGE: OnceLock<Handle> = OnceLock::new();
static EYE: OnceLock<Handle> = OnceLock::new();
static EYE_OFF: OnceLock<Handle> = OnceLock::new();

fn handle(cell: &'static OnceLock<Handle>, name: &str) -> Handle {
    cell.get_or_init(|| icon_handle(name)).clone()
}

#[cfg(test)]
mod tests {
    use super::follow_offset;

    #[test]
    fn the_keyboard_row_stays_in_view() {
        // In view: nothing to do.
        assert_eq!(follow_offset(0, 0.0), None);
        // Below the bottom: scroll so it is the last row.
        let y = follow_offset(30, 0.0).unwrap();
        assert!(y > 0.0);
        // Above the top: scroll to it.
        assert_eq!(follow_offset(2, 500.0), Some(2.0 * 29.0));
    }
}
