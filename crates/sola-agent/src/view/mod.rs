//! Agent UI composition. Borders/fills only — this iced stack does not blur
//! shadows.
//!
//! Chrome density inherits kit helpers (`button::labeled`, type roles, SPACE_*).
pub(crate) mod approval;
pub(crate) mod bubble;
pub(crate) mod firstrun;
pub(crate) mod footer;
pub(crate) mod sidebar;
pub(crate) mod tool;

use iced::widget::{container, row, scrollable, Column};
use iced::Element;
use iced::{Length, Padding};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL};
use sola_kit::components::text_input;
use sola_kit::components::text_input::text_input;

use crate::{App, Msg};

pub(crate) fn screen(app: &App) -> Element<'_, Msg> {
    if app.first_run {
        return firstrun::view(app);
    }
    let bubbles: Vec<Element<'_, Msg>> = app
        .turns
        .iter()
        .map(|t| bubble::turn_view(t, &app.theme))
        .collect();
    // Transcript pad 20 ≈ SPACE_XL (16) + SPACE_SM (4).
    let transcript = scrollable(
        Column::with_children(bubbles)
            .spacing(SPACE_LG)
            .padding(Padding::new(SPACE_XL + SPACE_SM))
            .width(Length::Fill),
    )
    .height(Length::Fill);

    let mut center: Vec<Element<'_, Msg>> = vec![transcript.into()];
    if let Some(p) = &app.pending {
        center.push(approval::strip(p, &app.theme));
    }
    center.push(input_row(app));
    center.push(footer::view(app));

    row![
        sidebar::view(app),
        Column::with_children(center)
            .width(Length::Fill)
            .height(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Draft field + Send/Stop. Disabled while an approval is pending: the
/// worker's `wait_for_decision` discards any `Send` command while
/// `App.pending` is `Some`, so submitting here would silently drop the
/// message — gate the field and swap in a static prompt instead.
fn input_row(app: &App) -> Element<'_, Msg> {
    let gated = app.pending.is_some();

    let field = if gated {
        text_input("Resolve the pending approval to continue…", &app.draft)
            .size(13)
            .style(text_input::style)
            .width(Length::Fill)
    } else {
        text_input("Ask Sola Agent…", &app.draft)
            .on_input(Msg::DraftChanged)
            .on_submit(Msg::Send)
            .size(13)
            .style(text_input::style)
            .width(Length::Fill)
    };

    let action: Element<'_, Msg> = if app.streaming.is_some() {
        kit_btn::labeled("Stop", kit_btn::danger)
            .on_press(Msg::Abort)
            .into()
    } else {
        let send = kit_btn::labeled("Send", kit_btn::primary);
        if gated {
            send.into()
        } else {
            send.on_press(Msg::Send).into()
        }
    };

    container(row![field, action].spacing(SPACE_MD))
        .padding(SPACE_XL)
        .width(Length::Fill)
        .into()
}
