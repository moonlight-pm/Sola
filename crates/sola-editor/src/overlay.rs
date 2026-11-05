//! What comes up over the text: the line at the foot for find and go-to
//! line, the command palette, and the question about unsaved work.

use iced::widget::{Space, button, column, container, mouse_area, row, stack, text};
use iced::{Alignment, Color, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::card;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM};
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;

use crate::Msg;
use crate::buffer::Pos;
use crate::command::Command;
use crate::keymap::Keymap;
use crate::win::{mini_id, palette_id};

const PALETTE_ROWS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiniKind {
    Find,
    GotoLine,
}

/// The line at the foot.
#[derive(Debug, Clone)]
pub struct Mini {
    pub kind: MiniKind,
    pub text: String,
    /// Where the search started, which is where typing searches from.
    pub origin: Pos,
    /// Nothing found for what's typed.
    pub failed: bool,
}

impl Mini {
    pub fn new(kind: MiniKind, origin: Pos) -> Self {
        Self { kind, text: String::new(), origin, failed: false }
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let label = match self.kind {
            MiniKind::Find => "Find",
            MiniKind::GotoLine => "Go to line",
        };
        let hint = match (self.kind, self.failed) {
            (MiniKind::Find, true) => "Not found",
            (MiniKind::Find, false) => "C-s next · C-r previous · C-g close",
            (MiniKind::GotoLine, _) => "line or line:column · C-g close",
        };
        container(
            row![
                text(label).font(fonts::ui_medium()).size(12),
                text_input("", &self.text)
                    .id(mini_id())
                    .on_input(Msg::MiniInput)
                    .on_submit(Msg::MiniSubmit)
                    .font(fonts::mono())
                    .size(13)
                    .padding(Padding::from([3.0, 8.0]))
                    .width(Length::Fixed(360.0)),
                text(hint).font(fonts::ui()).size(12).style(muted),
            ]
            .spacing(SPACE_MD)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([4.0, 12.0]))
        .width(Length::Fill)
        .style(foot)
        .into()
    }
}

/// The commands to pick from, and what's typed to narrow them.
#[derive(Debug, Clone, Default)]
pub struct Palette {
    pub query: String,
    pub selected: usize,
}

impl Palette {
    /// The commands whose names hold every word typed.
    pub fn entries(&self) -> Vec<(Command, &'static str)> {
        let query = self.query.to_lowercase();
        Command::NAMED
            .iter()
            .filter(|(_, name)| {
                let name = name.to_lowercase();
                query.split_whitespace().all(|word| name.contains(word))
            })
            .copied()
            .collect()
    }

    pub fn step(&mut self, delta: isize) {
        let count = self.entries().len() as isize;
        if count > 0 {
            self.selected = (self.selected as isize + delta).rem_euclid(count) as usize;
        }
    }

    pub fn current(&self) -> Option<Command> {
        self.entries().get(self.selected).map(|(command, _)| *command)
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let mut rows = column![
            text_input("Type a command", &self.query)
                .id(palette_id())
                .on_input(Msg::PaletteInput)
                .on_submit(Msg::PaletteSubmit)
                .size(14)
                .padding(Padding::from([7.0, 10.0])),
        ]
        .spacing(SPACE_SM);
        let start = self.selected.saturating_sub(PALETTE_ROWS - 1);
        let entries = self.entries();
        for (i, (command, name)) in entries.iter().enumerate().skip(start).take(PALETTE_ROWS) {
            let key = Keymap::key_of(*command).unwrap_or("");
            rows = rows.push(
                button(
                    row![
                        text(*name).font(fonts::ui()).size(13),
                        Space::new().width(Length::Fill),
                        text(key).font(fonts::mono()).size(12).style(muted),
                    ]
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding(Padding::from([5.0, 10.0]))
                .style(kit_btn::list_item(i == self.selected))
                .on_press(Msg::PaletteRun(*command)),
            );
        }
        let panel = container(card::modal(container(rows).padding(SPACE_MD)))
            .max_width(520.0);
        // A click off the panel closes it.
        let backdrop = mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
            .on_press(Msg::PaletteClose);
        stack![
            backdrop,
            container(panel)
                .width(Length::Fill)
                .align_x(Alignment::Center)
                .padding(Padding { top: 60.0, ..Padding::ZERO }),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

/// A question that has to be answered before going on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    /// Closing a file with unsaved changes.
    CloseUnsaved,
    /// Saving over a file that changed on disk since it was loaded.
    SaveOverChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Save,
    Discard,
    Overwrite,
    Reload,
    Cancel,
}

impl Prompt {
    /// What Enter answers: the highlighted button.
    pub fn primary(self) -> Answer {
        match self {
            Prompt::CloseUnsaved => Answer::Save,
            Prompt::SaveOverChange => Answer::Overwrite,
        }
    }

    pub fn view<'a>(self, name: &str) -> Element<'a, Msg> {
        let (title, body, buttons): (String, &str, Vec<(&str, Answer, bool)>) = match self {
            Prompt::CloseUnsaved => (
                format!("Save changes to {name}?"),
                "Your changes are lost if you don't save them.",
                vec![
                    ("Don't Save", Answer::Discard, false),
                    ("Cancel", Answer::Cancel, false),
                    ("Save", Answer::Save, true),
                ],
            ),
            Prompt::SaveOverChange => (
                format!("{name} changed on disk"),
                "Something else wrote to it since you opened it.",
                vec![
                    ("Reload from Disk", Answer::Reload, false),
                    ("Cancel", Answer::Cancel, false),
                    ("Overwrite", Answer::Overwrite, true),
                ],
            ),
        };
        let mut actions = row![Space::new().width(Length::Fill)].spacing(SPACE_SM);
        for (label, answer, primary) in buttons {
            let style = if primary { kit_btn::primary } else { kit_btn::secondary };
            actions = actions.push(kit_btn::labeled_sm(label, style).on_press(Msg::Answer(answer)));
        }
        let panel = card::modal(
            container(
                column![
                    text(title).font(fonts::ui_medium()).size(14),
                    text(body).font(fonts::ui()).size(13).style(muted),
                    actions,
                ]
                .spacing(SPACE_LG),
            )
            .padding(SPACE_LG + SPACE_SM),
        );
        container(container(panel).max_width(420.0))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.25).into()),
                ..container::Style::default()
            })
            .into()
    }
}

/// The strip's surface, a line of the window's chrome.
pub fn foot(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.background.weak.color.into()),
        ..container::Style::default()
    }
}

pub fn muted(theme: &Theme) -> text::Style {
    let palette = theme.palette();
    text::Style {
        color: Some(sola_kit::components::style::mix(palette.background, palette.text, 0.45)),
    }
}
