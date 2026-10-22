//! A project window's tabs, along its top (Next), in their saved order.
//! Each shows its agent mark and `×N`; hover shows ×, a double-click
//! renames, a drag onto another tab takes its place, `+` opens the new-tab
//! card. The wheel scrolls the strip when the tabs pass the width.

use std::collections::HashMap;
use std::sync::OnceLock;

use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{Space, button, container, mouse_area, row, scrollable, svg, text};
use iced::{Alignment, Background, Element, Length, Padding, Theme};
use sola_kit::components::icon::icon_handle;
use sola_kit::components::sidebar::item_style;
use sola_kit::components::style::{CHROME_SURFACE, SPACE_SM, SPACE_XS};
use sola_kit::components::text_input::text_input;
use sola_kit::components::{status_mark, toolbar_icon_tip};
use sola_kit::fonts;

use crate::Msg;
use crate::catalog::{self, Tab};
use crate::status::PaneStatus;

/// The bar's height, above the panes.
pub const HEIGHT: f32 = 36.0;
/// The hairline between the bar and the panes.
pub const RULE: f32 = sola_kit::components::LINE_PX;
pub const RENAME_INPUT_ID: &str = "ws-tab-rename";

/// What the bar shows of the app's state.
pub struct Bar<'a> {
    pub project_id: &'a str,
    pub tabs: &'a [Tab],
    pub selected: &'a str,
    pub hovered: Option<&'a str>,
    /// `(tab id, draft name)` while a tab is being renamed.
    pub renaming: Option<&'a (String, String)>,
    /// The tab being dragged, if any.
    pub dragging: Option<&'a str>,
    pub pane_status: &'a HashMap<String, PaneStatus>,
}

pub fn view<'a>(bar: Bar<'a>) -> Element<'a, Msg> {
    let mut chips = row![].spacing(SPACE_XS).align_y(Alignment::Center);
    for tab in catalog::ordered_for_project(bar.project_id, bar.tabs) {
        let renaming = bar
            .renaming
            .filter(|(id, _)| *id == tab.id)
            .map(|(_, draft)| draft.as_str());
        let hovered = bar.hovered == Some(tab.id.as_str());
        let drop_target = hovered && bar.dragging.is_some_and(|d| d != tab.id);
        chips = chips.push(chip(
            tab,
            tab.id == bar.selected,
            hovered,
            drop_target,
            renaming,
            bar.pane_status,
        ));
    }
    // A plain wheel scrolls sideways (a horizontal scrollable only takes
    // Shift+wheel).
    let project = bar.project_id.to_string();
    let wheel = mouse_area(chips).on_scroll(move |delta| Msg::BarScroll(project.clone(), delta));
    let strip = scrollable(wheel)
        .id(strip_id(bar.project_id))
        .direction(Direction::Horizontal(
            Scrollbar::new().width(0).scroller_width(0),
        ))
        .width(Length::Fill);
    let add = toolbar_icon_tip(
        plus_icon(),
        "New tab",
        Some(Msg::OpenSpawn(bar.project_id.to_string())),
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
        background: Some(Background::Color(CHROME_SURFACE)),
        ..container::Style::default()
    })
    .into()
}

/// The strip's scrollable, per project window.
pub fn strip_id(project_id: &str) -> iced::widget::Id {
    iced::widget::Id::from(format!("topbar-{project_id}"))
}

fn chip<'a>(
    tab: &'a Tab,
    selected: bool,
    hovered: bool,
    drop_target: bool,
    renaming: Option<&'a str>,
    pane_status: &'a HashMap<String, PaneStatus>,
) -> Element<'a, Msg> {
    let mut content = row![status_mark(tab.status.indicator())]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center);
    if let Some(draft) = renaming {
        content = content.push(
            text_input("label", draft)
                .id(iced::widget::Id::new(RENAME_INPUT_ID))
                .size(12)
                .font(fonts::ui())
                .on_input(Msg::RenameInput)
                .on_submit(Msg::RenameCommit)
                .style(sola_kit::components::text_input::style)
                .padding(Padding::from([1, 4]))
                .width(Length::Fixed(150.0)),
        );
    } else {
        content = content.push(text(crate::cli::rail_label(tab)).font(fonts::ui()).size(12));
        let leaves = tab.layout().leaves();
        let n = crate::status::rail_compaction(leaves.iter().filter_map(|id| pane_status.get(id)));
        if n > 0 {
            content = content.push(
                text(format!("×{n}"))
                    .font(fonts::ui())
                    .size(11)
                    .style(sola_kit::components::text::muted),
            );
        }
    }
    // Root stays (Drop Project unregisters the project); the others close
    // from the chip. The slot is kept so the chip does not jump on hover.
    if catalog::can_close(tab) && renaming.is_none() {
        let close: Element<'a, Msg> = if hovered || selected {
            button(
                svg(close_icon())
                    .width(Length::Fixed(12.0))
                    .height(Length::Fixed(12.0))
                    .style(|theme: &Theme, _status| svg::Style {
                        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.6)),
                    }),
            )
                .padding(1)
                .style(|_theme: &Theme, _status| button::Style::default())
                .on_press(Msg::CloseTab(tab.id.clone()))
                .into()
        } else {
            Space::new().width(14).height(14).into()
        };
        content = content.push(close);
    }
    let status = if hovered {
        button::Status::Hovered
    } else {
        button::Status::Active
    };
    let body = container(content)
        .padding(Padding::from([5, 10]))
        .style(move |theme: &Theme| {
            let s = item_style(theme, status, selected);
            // A dragged tab dropped here takes this place.
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
    let mut area = mouse_area(body)
        .on_press(Msg::ChipPress(tab.id.clone()))
        .on_enter(Msg::HoverIn(tab.id.clone()))
        .on_exit(Msg::HoverOut(tab.id.clone()));
    if catalog::can_close(tab) {
        area = area.on_double_click(Msg::BeginRename(tab.id.clone()));
    }
    area.into()
}

fn close_icon() -> svg::Handle {
    static H: OnceLock<svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/x")).clone()
}

fn plus_icon() -> svg::Handle {
    static H: OnceLock<svg::Handle> = OnceLock::new();
    H.get_or_init(|| icon_handle("lucide/plus")).clone()
}
