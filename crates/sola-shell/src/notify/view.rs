//! Notification overlay (live cards) and the missed-pile menu panel.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, mouse_area, row, scrollable, text};
use iced::{Alignment, Element, Length, Padding};

use sola_bus::topics::AppNotification;
use sola_kit::components::button as kit_btn;
use sola_kit::components::popover;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::{count_mark, icon, icon_colored};
use sola_kit::fonts;

use crate::app::{Msg, Shell};
use crate::notify::{COLLAPSE_AFTER, EXPAND_SHOW, PileGroup};

const TITLE_SIZE: f32 = 13.0;
const SOURCE_SIZE: f32 = 11.0;
const BODY_SIZE: f32 = 12.0;
const ICON: u16 = 18;
pub const PILE_WIDTH: f32 = 320.0;

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
    let title: Element<'_, Msg> = text("Notifications")
        .font(fonts::ui_medium())
        .size(13.0)
        .into();

    let body: Element<'_, Msg> = if shell.notify.pile.is_empty() {
        let muted = shell.theme.extended_palette().secondary.base.text;
        text("Nothing missed.")
            .font(fonts::ui())
            .size(12.0)
            .style(move |_: &iced::Theme| iced::widget::text::Style { color: Some(muted) })
            .into()
    } else {
        let groups = shell.notify.groups();
        let blocks: Vec<Element<'_, Msg>> = groups.iter().map(|g| group_block(shell, g)).collect();
        scrollable(column(blocks).spacing(SPACE_LG).width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    };

    let card: Element<'_, Msg> = popover(
        column![title, body]
            .spacing(SPACE_SM)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .padding(SPACE_MD)
    .width(Length::Fill)
    .height(Length::Fill)
    .into();
    crate::menu::host_card(card)
}

fn group_block<'a>(shell: &'a Shell, g: &PileGroup<'a>) -> Element<'a, Msg> {
    if g.items.len() <= COLLAPSE_AFTER {
        let rows: Vec<Element<'a, Msg>> =
            g.items.iter().map(|n| item_row(shell, n, true)).collect();
        return column(rows).spacing(SPACE_SM).width(Length::Fill).into();
    }
    let expanded = shell.notify.group_expanded(g.app_id);
    let mut col = column![group_header(shell, g, expanded)].spacing(SPACE_XS);
    if expanded {
        let show = g.items.len().min(EXPAND_SHOW);
        for n in g.items.iter().take(show) {
            col = col.push(item_row(shell, n, false));
        }
        if g.items.len() > EXPAND_SHOW {
            let more = g.items.len() - EXPAND_SHOW;
            let muted = shell.theme.extended_palette().secondary.base.text;
            col = col.push(
                container(
                    text(format!("{more} more"))
                        .font(fonts::chrome())
                        .size(SOURCE_SIZE)
                        .style(move |_: &iced::Theme| iced::widget::text::Style {
                            color: Some(muted),
                        }),
                )
                .padding(Padding {
                    top: 2.0,
                    right: 8.0,
                    bottom: 4.0,
                    left: 34.0,
                }),
            );
        }
    }
    col.width(Length::Fill).into()
}

fn group_header<'a>(shell: &'a Shell, g: &PileGroup<'a>, expanded: bool) -> Element<'a, Msg> {
    let muted = shell.theme.extended_palette().secondary.base.text;
    let latest = g.items[0];
    let app_id = g.app_id.to_string();
    let label = app_label(shell, g.app_id, &latest.source);
    let icon_name = icon_for(shell, latest);
    let count = g.items.len() as u32;

    let name: Element<'_, Msg> = text(label.to_string())
        .font(fonts::ui_medium())
        .size(TITLE_SIZE)
        .wrapping(Wrapping::None)
        .width(Length::Fill)
        .into();
    let preview: Element<'_, Msg> = text(latest.title.clone())
        .font(fonts::ui())
        .size(BODY_SIZE)
        .wrapping(Wrapping::None)
        .width(Length::Fill)
        .style(move |_: &iced::Theme| iced::widget::text::Style { color: Some(muted) })
        .into();

    let copy: Element<'_, Msg> = column![name, preview]
        .spacing(SPACE_XS)
        .width(Length::Fill)
        .into();

    let dismiss_id = app_id.clone();
    let x: Element<'_, Msg> = button(icon_colored("lucide/x", 12, muted))
        .padding([4, 6])
        .style(kit_btn::ghost)
        .on_press(Msg::NotifyDismissApp(dismiss_id))
        .into();

    let body: Element<'_, Msg> = row![icon(icon_name, ICON), copy, count_mark(count)]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into();

    row![
        button(body)
            .padding(Padding::from([6, 8]))
            .width(Length::Fill)
            .style(kit_btn::list_item(expanded))
            .on_press(Msg::NotifyToggleGroup(app_id)),
        x
    ]
    .spacing(SPACE_XS)
    .align_y(Alignment::Start)
    .width(Length::Fill)
    .into()
}

fn item_row<'a>(shell: &'a Shell, n: &'a AppNotification, leading_icon: bool) -> Element<'a, Msg> {
    let id = n.id.clone();
    let muted = shell.theme.extended_palette().secondary.base.text;
    let title: Element<'_, Msg> = text(n.title.clone())
        .font(fonts::ui())
        .size(BODY_SIZE)
        .wrapping(Wrapping::None)
        .width(Length::Fill)
        .into();
    let copy: Element<'_, Msg> = if leading_icon {
        let source: Element<'_, Msg> = text(n.source.clone())
            .font(fonts::chrome())
            .size(SOURCE_SIZE)
            .wrapping(Wrapping::None)
            .width(Length::Fill)
            .style(move |_: &iced::Theme| iced::widget::text::Style { color: Some(muted) })
            .into();
        column![source, title]
            .spacing(SPACE_XS)
            .width(Length::Fill)
            .into()
    } else {
        title
    };

    let dismiss_id = id.clone();
    let x: Element<'_, Msg> = button(icon_colored("lucide/x", 12, muted))
        .padding([4, 6])
        .style(kit_btn::ghost)
        .on_press(Msg::NotifyDismiss(dismiss_id))
        .into();

    let inner: Element<'_, Msg> = if leading_icon {
        row![icon(icon_for(shell, n), ICON), copy]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into()
    } else {
        container(copy)
            .padding(Padding {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: 26.0,
            })
            .width(Length::Fill)
            .into()
    };

    row![
        button(inner)
            .padding(Padding::from([6, 8]))
            .width(Length::Fill)
            .style(kit_btn::list_item(false))
            .on_press(Msg::NotifyActivate(id)),
        x
    ]
    .spacing(SPACE_XS)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

fn app_label<'a>(shell: &'a Shell, app_id: &str, fallback: &'a str) -> &'a str {
    shell
        .applications
        .get_for_window(app_id)
        .map(|a| a.label.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
}
