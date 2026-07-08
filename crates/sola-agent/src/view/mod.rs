//! Agent UI composition. Borders/fills only — this iced stack does not blur
//! shadows.
pub(crate) mod approval;
pub(crate) mod bubble;
pub(crate) mod firstrun;
pub(crate) mod footer;
pub(crate) mod sidebar;
pub(crate) mod tool;

use iced::widget::{button, container, row, scrollable, text, text_input, Column};
use iced::Element;
use iced::{Length, Padding};

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
    let transcript = scrollable(
        Column::with_children(bubbles)
            .spacing(12)
            .padding(Padding::new(20.0))
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
            .padding(12)
            .size(15)
            .width(Length::Fill)
    } else {
        text_input("Ask Sola Agent…", &app.draft)
            .on_input(Msg::DraftChanged)
            .on_submit(Msg::Send)
            .padding(12)
            .size(15)
            .width(Length::Fill)
    };

    let action: Element<'_, Msg> = if app.streaming.is_some() {
        button(text("Stop"))
            .style(sola_kit::components::button::danger)
            .on_press(Msg::Abort)
            .into()
    } else {
        let send = button(text("Send")).style(sola_kit::components::button::primary);
        if gated { send.into() } else { send.on_press(Msg::Send).into() }
    };

    container(row![field, action].spacing(8))
        .padding(Padding::new(16.0))
        .width(Length::Fill)
        .into()
}
