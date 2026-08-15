//! Context menu showcase — pointer-anchored flat menu.

use iced::widget::{column, container, mouse_area, text};
use iced::{Element, Length, Point};

use sola_kit::components::context_menu::{MenuItem, menu_at};
use sola_kit::components::text::{body, heading, muted};

use crate::storybook::pages::chrome::{lede, panel};

#[derive(Clone, Debug)]
pub enum Msg {
    OpenAt(Point),
    Dismiss,
    Picked(&'static str),
}

#[derive(Default)]
pub struct State {
    pub open_at: Option<Point>,
    pub last: Option<&'static str>,
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::OpenAt(p) => self.open_at = Some(p),
            Msg::Dismiss => self.open_at = None,
            Msg::Picked(s) => {
                self.last = Some(s);
                self.open_at = None;
            }
        }
    }
}

pub fn view(state: &State) -> Element<'_, Msg> {
    let hint = match state.last {
        Some(s) => format!("Last action: {s}"),
        None => "Right-click the well.".into(),
    };
    let well = mouse_area(
        container(text(hint).size(13))
            .width(Length::Fill)
            .height(Length::Fixed(160.0))
            .padding(16)
            .style(|theme: &iced::Theme| {
                let p = theme.extended_palette();
                container::Style {
                    background: Some(iced::Background::Color(p.background.weak.color)),
                    border: iced::Border {
                        color: p.background.strong.color,
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..container::Style::default()
                }
            }),
    )
    .on_right_press(Msg::OpenAt(Point::new(48.0, 120.0)));

    let mut stack = column![
        lede(
            "Context menu",
            "Flat actions at the pointer. Escape or click outside dismisses. No submenu in v1.",
        ),
        panel(column![body("Right-click the well."), well].spacing(10)),
    ]
    .spacing(16);

    if let Some(at) = state.open_at {
        let items = vec![
            MenuItem::action("New group", Msg::Picked("New group")),
            MenuItem::action("Add to Work", Msg::Picked("Add to Work")),
            MenuItem::separator(),
            MenuItem::disabled("Ungroup"),
        ];
        stack = stack.push(menu_at(at, items, Msg::Dismiss));
    }

    column![
        heading("Context menu"),
        body("Kit primitive.").style(muted),
        stack,
    ]
    .spacing(8)
    .into()
}
