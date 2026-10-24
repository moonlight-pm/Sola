//! A project window's canvases, along its top (Next), in their saved
//! order. A chip's label is the canvas's folder name; a ring marks a canvas
//! an agent is working in. A click shows it, a drag onto another chip takes
//! that place, `+` opens New Canvas. The wheel scrolls the strip when the
//! chips pass the width.

use std::collections::HashSet;
use std::sync::OnceLock;

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{container, mouse_area, row, scrollable, svg, text};
use iced::{Alignment, Background, Element, Length, Padding, Theme};
use sola_kit::components::icon::icon_handle;
use sola_kit::components::sidebar::item_style;
use sola_kit::components::style::{chrome_surface, SPACE_SM, SPACE_XS};
use sola_kit::components::{SidebarIndicator, status_mark, toolbar_icon_tip};
use sola_kit::fonts;

use crate::app::Msg;
use crate::canvas;
use crate::projects::CanvasRef;

/// The bar's height, above the plane.
pub const HEIGHT: f32 = 36.0;

/// What the bar shows of the app's state.
pub struct Bar<'a> {
    pub project: &'a str,
    pub canvases: &'a [canvas::Entry],
    /// The canvas the window shows (a dir name).
    pub selected: Option<String>,
    /// Chip under the pointer (a canvas key).
    pub hovered: Option<&'a str>,
    /// The chip being dragged (a canvas key).
    pub dragging: Option<&'a str>,
    /// Canvases an agent is working in (keys).
    pub busy: HashSet<String>,
}

pub fn view<'a>(bar: Bar<'a>) -> Element<'a, Msg> {
    let mut chips = row![].spacing(SPACE_XS).align_y(Alignment::Center);
    for entry in bar.canvases {
        let key = CanvasRef {
            project: bar.project.to_string(),
            canvas: entry.name.clone(),
        }
        .key();
        let hovered = bar.hovered == Some(key.as_str());
        let drop_target = hovered && bar.dragging.is_some_and(|d| d != key);
        chips = chips.push(chip(
            &entry.name,
            key.clone(),
            bar.selected.as_deref() == Some(entry.name.as_str()),
            hovered,
            drop_target,
            bar.busy.contains(&key),
        ));
    }
    // A plain wheel scrolls sideways (a horizontal scrollable only takes
    // Shift+wheel).
    let project = bar.project.to_string();
    let wheel = mouse_area(chips).on_scroll(move |delta| Msg::BarScroll(project.clone(), delta));
    let strip = scrollable(wheel)
        .id(strip_id(bar.project))
        .direction(Direction::Horizontal(
            Scrollbar::new().width(0).scroller_width(0),
        ))
        .width(Length::Fill);
    let add = toolbar_icon_tip(
        plus_icon(),
        "New canvas",
        Some(Msg::OpenNewCanvas(bar.project.to_string())),
    );
    container(
        row![strip, add]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center),
    )
    .padding(Padding::from([0, 6]))
    .height(Length::Fixed(HEIGHT))
    .width(Length::Fill)
    .align_y(Alignment::Center)
    .style(|_theme: &Theme| container::Style {
        background: Some(Background::Color(chrome_surface())),
        ..container::Style::default()
    })
    .into()
}

/// The strip's scrollable, per project window.
pub fn strip_id(project: &str) -> iced::widget::Id {
    iced::widget::Id::from(format!("canvases-{project}"))
}

fn chip<'a>(
    name: &str,
    key: String,
    selected: bool,
    hovered: bool,
    drop_target: bool,
    busy: bool,
) -> Element<'a, Msg> {
    let mut content = row![].spacing(SPACE_SM).align_y(Alignment::Center);
    if busy {
        content = content.push(status_mark(SidebarIndicator::Working));
    }
    content = content.push(text(name.to_string()).font(fonts::ui()).size(12));
    let status = if hovered {
        iced::widget::button::Status::Hovered
    } else {
        iced::widget::button::Status::Active
    };
    let body = container(content)
        .padding(Padding::from([5, 10]))
        .style(move |theme: &Theme| {
            let s = item_style(theme, status, selected);
            // A dragged chip dropped here takes this place.
            let border = if drop_target {
                iced::Border {
                    color: theme.extended_palette().primary.base.color,
                    width: 1.0,
                    ..s.border
                }
            } else {
                s.border
            };
            container::Style {
                background: s.background,
                text_color: Some(s.text_color),
                border,
                ..container::Style::default()
            }
        });
    mouse_area(body)
        .on_press(Msg::ChipPress(key.clone()))
        .on_enter(Msg::HoverIn(key.clone()))
        .on_exit(Msg::HoverOut(key))
        .into()
}

fn plus_icon() -> svg::Handle {
    static H: OnceLock<svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/plus")).clone()
}
