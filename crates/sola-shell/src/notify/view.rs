//! Notification overlay: the live cards.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, mouse_area, row, text};
use iced::{Alignment, Element, Length};

use sola_bus::topics::AppNotification;
use sola_kit::components::button as kit_btn;
use sola_kit::components::popover;
use sola_kit::components::style::{SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::{icon, icon_colored};
use sola_kit::fonts;

use crate::app::{Msg, Shell};

const TITLE_SIZE: f32 = 13.0;
const SOURCE_SIZE: f32 = 11.0;
const BODY_SIZE: f32 = 12.0;
const ICON: u16 = 18;

pub fn view(shell: &Shell) -> Element<'_, Msg> {
    if !shell.notify.visible() {
        return container(text(""))
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }
    let cards: Vec<Element<'_, Msg>> = shell
        .notify
        .prompt
        .iter()
        .map(|p| prompt_card(shell, p))
        .chain(shell.notify.live.iter().map(|b| banner_card(shell, &b.n)))
        .collect();
    column(cards)
        .spacing(crate::notify::CARD_GAP as f32)
        .width(Length::Fill)
        .into()
}

/// An app asking to be the handler for a scheme or a type: **Keep** (the
/// default) or **Use**. It stays until you answer.
fn prompt_card<'a>(shell: &'a Shell, p: &'a crate::handlers::Prompt) -> Element<'a, Msg> {
    let muted = shell.theme.extended_palette().secondary.base.text;
    let source: Element<'_, Msg> = text("Default app")
        .font(fonts::chrome())
        .size(SOURCE_SIZE)
        .style(move |_: &iced::Theme| iced::widget::text::Style { color: Some(muted) })
        .into();
    let title: Element<'_, Msg> = text(p.question())
        .font(fonts::ui_medium())
        .size(TITLE_SIZE)
        .wrapping(Wrapping::Word)
        .into();
    let buttons = row![
        kit_btn::labeled_sm(p.keep_label(), kit_btn::ghost).on_press(Msg::HandlerAnswer(false)),
        kit_btn::labeled_sm(p.use_label(), kit_btn::primary).on_press(Msg::HandlerAnswer(true)),
    ]
    .spacing(SPACE_XS);
    let copy = column![source, title, buttons].spacing(SPACE_SM);
    popover(
        row![icon("lucide/link", ICON), copy]
            .spacing(SPACE_SM)
            .align_y(Alignment::Start)
            .width(Length::Fill),
    )
    .padding(SPACE_MD)
    .width(Length::Fill)
    .into()
}

fn banner_card<'a>(shell: &'a Shell, n: &'a AppNotification) -> Element<'a, Msg> {
    let id = n.id.clone();
    let icon_name = shell.app_icon(&n.app_id);
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
