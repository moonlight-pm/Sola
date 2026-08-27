//! Notification overlay (live cards) and the missed-pile menu panel.

use iced::widget::{button, column, container, mouse_area, row, text};
use iced::{Alignment, Element, Length, Padding};

use sola_bus::topics::AppNotification;
use sola_kit::components::button as kit_btn;
use sola_kit::components::popover;
use sola_kit::components::style::{RADIUS_LG, SPACE_MD, SPACE_SM, SPACE_XS, hairline};
use sola_kit::components::{icon, icon_colored};
use sola_kit::fonts;

use crate::app::{Msg, Shell};

const TITLE_SIZE: f32 = 13.0;
const SOURCE_SIZE: f32 = 11.0;
const BODY_SIZE: f32 = 12.0;
const ICON: u16 = 16;
const PILE_WIDTH: f32 = 320.0;

pub fn view(shell: &Shell) -> Element<'_, Msg> {
    if !shell.notify.visible() {
        return container(text(""))
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }
    let cards: Vec<Element<'_, Msg>> = shell
        .notify
        .live
        .iter()
        .map(|b| banner_card(shell, &b.n))
        .collect();
    column(cards)
        .spacing(crate::notify::CARD_GAP as f32)
        .width(Length::Fill)
        .into()
}

fn banner_card<'a>(shell: &'a Shell, n: &'a AppNotification) -> Element<'a, Msg> {
    let id = n.id.clone();
    let icon_name = icon_for(shell, n);
    let muted = shell.theme.extended_palette().secondary.base.text;

    let source: Element<'_, Msg> = text(n.source.clone())
        .font(fonts::chrome())
        .size(SOURCE_SIZE)
        .style(move |_: &iced::Theme| iced::widget::text::Style { color: Some(muted) })
        .into();
    let title: Element<'_, Msg> = text(n.title.clone())
        .font(fonts::ui_medium())
        .size(TITLE_SIZE)
        .into();
    let mut copy = column![source, title].spacing(SPACE_XS);
    if !n.body.is_empty() {
        let body: Element<'_, Msg> = text(n.body.clone())
            .font(fonts::ui())
            .size(BODY_SIZE)
            .style(move |_: &iced::Theme| iced::widget::text::Style { color: Some(muted) })
            .into();
        copy = copy.push(body);
    }

    let dismiss_id = id.clone();
    let x: Element<'_, Msg> = button(icon_colored("lucide/x", 12, muted))
        .padding([4, 6])
        .style(kit_btn::ghost)
        .on_press(Msg::NotifyDismiss(dismiss_id))
        .into();

    let body_click: Element<'_, Msg> = mouse_area(
        row![icon(icon_name, ICON), copy.spacing(SPACE_XS)]
            .spacing(SPACE_SM)
            .align_y(Alignment::Start)
            .width(Length::Fill),
    )
    .on_press(Msg::NotifyActivate(id))
    .into();

    popover(
        row![body_click, x]
            .spacing(SPACE_XS)
            .align_y(Alignment::Start)
            .width(Length::Fill),
    )
    .padding(SPACE_MD)
    .width(Length::Fill)
    .into()
}

fn icon_for<'a>(shell: &'a Shell, n: &'a AppNotification) -> &'a str {
    if n.app_id == "sola-browser" {
        return "lucide/globe";
    }
    shell
        .applications
        .get_for_window(&n.app_id)
        .map(|a| a.icon.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("lucide/bell")
}

/// Missed-pile list hosted in the menu overlay (calendar-style).
pub fn pile_panel(shell: &Shell) -> Element<'_, Msg> {
    let output_w = shell.output_size.map(|(w, _)| w as f32).unwrap_or(1920.0);
    let left = (output_w - PILE_WIDTH - 8.0).max(0.0);

    let title: Element<'_, Msg> = text("Notifications")
        .font(fonts::ui_medium())
        .size(13.0)
        .into();

    let mut rows: Vec<Element<'_, Msg>> = vec![title];
    if shell.notify.pile.is_empty() {
        let muted = shell.theme.extended_palette().secondary.base.text;
        rows.push(
            text("Nothing missed.")
                .font(fonts::ui())
                .size(12.0)
                .style(move |_: &iced::Theme| iced::widget::text::Style { color: Some(muted) })
                .into(),
        );
    } else {
        for n in &shell.notify.pile {
            rows.push(pile_row(shell, n));
        }
        rows.push(
            kit_btn::labeled_sm("Clear", kit_btn::ghost)
                .on_press(Msg::NotifyClearPile)
                .into(),
        );
    }

    let card: Element<'_, Msg> = popover(column(rows).spacing(SPACE_SM).width(Length::Fill))
        .padding(SPACE_MD)
        .width(Length::Fixed(PILE_WIDTH))
        .into();

    let positioned: Element<'_, Msg> = container(card)
        .padding(Padding {
            top: 0.0,
            left,
            right: 0.0,
            bottom: 0.0,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::alignment::Vertical::Top)
        .into();

    let backdrop: Element<'_, Msg> =
        mouse_area(container(text("")).width(Length::Fill).height(Length::Fill))
            .on_press(Msg::CloseMenu)
            .into();

    iced::widget::stack![backdrop, positioned]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn pile_row<'a>(shell: &'a Shell, n: &'a AppNotification) -> Element<'a, Msg> {
    let id = n.id.clone();
    let muted = shell.theme.extended_palette().secondary.base.text;
    let source: Element<'_, Msg> = text(n.source.clone())
        .font(fonts::chrome())
        .size(SOURCE_SIZE)
        .style(move |_: &iced::Theme| iced::widget::text::Style { color: Some(muted) })
        .into();
    let title: Element<'_, Msg> = text(n.title.clone())
        .font(fonts::ui())
        .size(12.0)
        .into();
    let dismiss_id = id.clone();
    let x: Element<'_, Msg> = button(icon_colored("lucide/x", 12, muted))
        .padding([2, 4])
        .style(kit_btn::ghost)
        .on_press(Msg::NotifyDismiss(dismiss_id))
        .into();
    let click: Element<'_, Msg> = mouse_area(
        column![source, title]
            .spacing(SPACE_XS)
            .width(Length::Fill),
    )
    .on_press(Msg::NotifyActivate(id))
    .into();

    container(
        row![click, x]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    )
    .padding([4, 2])
    .style(|theme: &iced::Theme| {
        let p = theme.extended_palette();
        iced::widget::container::Style {
            border: hairline(p, RADIUS_LG),
            background: None,
            ..Default::default()
        }
    })
    .into()
}
