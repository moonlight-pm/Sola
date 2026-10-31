//! The desk menu's look: Graphite dropdown sheets (`menu_sheet`, as a
//! titlebar menu's) where [`super::place`] puts them, over a backdrop: a
//! press anywhere off the sheets closes the menu.

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, mouse_area, pin, row, stack, text};
use iced::{Border, Element, Length, Padding, Theme};
use sola_kit::components::menu_sheet::{
    self, KEY_SIZE, LABEL_SIZE, PAD, RADIUS, ROW_H, ROW_PAD, ROW_RADIUS, SEP_H,
};
use sola_kit::components::text_input::{style as input_style, text_input};
use sola_kit::fonts;

use super::place::{CHEVRON_W, FIELD_H, Rect};
use super::rows::{Does, Item, Row};
use super::{DeskMenu, DeskMsg, SEARCH_INPUT_ID};
use crate::app::{Msg, Shell};

pub fn view(shell: &Shell) -> Element<'_, Msg> {
    let (Some(desk), Some(placed)) = (shell.desk.as_ref(), shell.desk_placed()) else {
        return backdrop();
    };
    let mut layers: Vec<Element<'_, Msg>> = vec![backdrop()];
    for (level, (col, sheet)) in desk.columns().into_iter().zip(placed.sheets).enumerate() {
        layers.push(pin(sheet_view(desk, level, col, sheet)).x(sheet.x).y(sheet.y).into());
    }
    stack(layers)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn backdrop<'a>() -> Element<'a, Msg> {
    mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
        .on_press(Msg::Desk(DeskMsg::Dismiss))
        .into()
}

fn sheet_view<'a>(desk: &DeskMenu, level: usize, rows: Vec<Row>, sheet: Rect) -> Element<'a, Msg> {
    let mut items: Vec<Element<'a, Msg>> = Vec::with_capacity(rows.len() + 1);
    if desk.searching(level) {
        let query = desk.search.as_ref().map_or("", |s| s.query.as_str());
        items.push(search_field(query));
    }
    for (i, r) in rows.into_iter().enumerate() {
        items.push(match r {
            Row::Sep => separator(),
            Row::Item(item) => {
                let lit = desk.hot(level) == Some(i) || desk.open_row(level) == Some(i);
                row_view(item, level, i, lit)
            }
        });
    }
    let body = container(column(items).width(Length::Fill))
        .padding(PAD)
        .width(Length::Fixed(sheet.w))
        .height(Length::Fixed(sheet.h))
        .clip(true)
        .style(|theme: &Theme| {
            let c = menu_sheet::colors(theme);
            container::Style {
                background: Some(c.sheet.into()),
                border: Border {
                    color: c.edge,
                    width: 1.0,
                    radius: RADIUS.into(),
                },
                shadow: menu_sheet::shadow(),
                ..container::Style::default()
            }
        });
    // A press on the sheet between rows is not a press off the menu.
    mouse_area(body).on_press(Msg::Noop).into()
}

fn row_view<'a>(item: Item, level: usize, index: usize, lit: bool) -> Element<'a, Msg> {
    let enabled = item.enabled;
    let label = text(item.label)
        .font(fonts::chrome())
        .size(LABEL_SIZE)
        .wrapping(Wrapping::None)
        .style(move |theme: &Theme| {
            let c = menu_sheet::colors(theme);
            text::Style {
                color: Some(if enabled { c.ink } else { c.ink_faint }),
            }
        });
    let quiet = |s: String| {
        text(s)
            .font(fonts::chrome())
            .size(KEY_SIZE)
            .wrapping(Wrapping::None)
            .style(|theme: &Theme| text::Style {
                color: Some(menu_sheet::colors(theme).ink_dim),
            })
    };
    let mut line = row![label, Space::new().width(Length::Fill)]
        .spacing(8.0)
        .align_y(iced::alignment::Vertical::Center);
    if let Some(hint) = item.hint {
        line = line.push(quiet(hint));
    }
    if matches!(item.does, Does::Sub(_)) {
        line = line.push(
            container(quiet("›".into()).size(LABEL_SIZE))
                .width(Length::Fixed(CHEVRON_W))
                .align_x(iced::alignment::Horizontal::Right),
        );
    }
    let body = container(line)
        .padding(Padding::from([0.0, ROW_PAD]))
        .width(Length::Fill)
        .height(Length::Fixed(ROW_H))
        .align_y(iced::alignment::Vertical::Center)
        .style(move |theme: &Theme| container::Style {
            background: lit.then(|| menu_sheet::colors(theme).hot.into()),
            border: Border {
                radius: ROW_RADIUS.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    if !enabled {
        return body.into();
    }
    mouse_area(body)
        .on_enter(Msg::Desk(DeskMsg::Hover { level, row: index }))
        .on_release(Msg::Desk(DeskMsg::Press { level, row: index }))
        .into()
}

fn separator<'a>() -> Element<'a, Msg> {
    let rule = container(Space::new().width(Length::Fill).height(Length::Fixed(1.0))).style(
        |theme: &Theme| container::Style {
            background: Some(menu_sheet::colors(theme).sep.into()),
            ..container::Style::default()
        },
    );
    container(rule)
        .padding(Padding::from([(SEP_H - 1.0) / 2.0, ROW_PAD]))
        .width(Length::Fill)
        .height(Length::Fixed(SEP_H))
        .into()
}

fn search_field<'a>(query: &str) -> Element<'a, Msg> {
    let field = text_input("Search vault", query)
        .id(iced::widget::Id::new(SEARCH_INPUT_ID))
        .on_input(|q| Msg::Desk(DeskMsg::SearchInput(q)))
        .style(input_style)
        .padding(Padding::from([6, 8]))
        .size(LABEL_SIZE)
        .width(Length::Fill);
    container(field)
        .padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: 6.0,
            left: 0.0,
        })
        .height(Length::Fixed(FIELD_H))
        .into()
}
