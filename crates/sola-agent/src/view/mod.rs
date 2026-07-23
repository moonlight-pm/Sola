//! Agent UI composition — two-pane kit layout with chat column.

pub(crate) mod approval;
pub(crate) mod bubble;
pub(crate) mod firstrun;
pub(crate) mod footer;
pub(crate) mod sidebar;

use iced::widget::{column, container, row, scrollable, Space, Column};
use iced::{Alignment, Background, Border, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{
    hairline, RADIUS_LG, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input;
use sola_kit::components::text_input::text_input;

use crate::{App, Msg};

/// Comfortable chat column width on large displays.
const CHAT_MAX: f32 = 720.0;

pub(crate) fn screen(app: &App) -> Element<'_, Msg> {
    if app.need_setup.is_some() && app.session_id.is_none() && app.turns.is_empty() {
        return firstrun::view(app);
    }

    let main = column![
        transcript(app),
        if let Some(p) = &app.pending {
            approval::strip(p)
        } else {
            Space::new().height(0).into()
        },
        composer(app),
        footer::view(app),
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    row![
        sidebar::view(app),
        container(main)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(main_pane_style),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn main_pane_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.base.color)),
        ..container::Style::default()
    }
}

fn transcript(app: &App) -> Element<'_, Msg> {
    let inner: Element<'_, Msg> = if app.turns.is_empty() {
        empty_transcript(app)
    } else {
        let bubbles: Vec<Element<'_, Msg>> = app
            .turns
            .iter()
            .map(|t| bubble::turn_view(t, &app.theme))
            .collect();
        Column::with_children(bubbles)
            .spacing(SPACE_LG)
            .width(Length::Fill)
            .into()
    };

    let padded = container(inner)
        .width(Length::Fill)
        .max_width(CHAT_MAX)
        .padding(Padding::from([SPACE_XL + SPACE_MD, SPACE_XL]));

    scrollable(
        container(padded)
            .width(Length::Fill)
            .center_x(Length::Fill),
    )
    .height(Length::Fill)
    .into()
}

fn empty_transcript(app: &App) -> Element<'_, Msg> {
    let title = if app.session_id.is_some() {
        "Continue this session"
    } else {
        "Start a conversation"
    };
    let hint = if app.connected {
        "Ask Grok to explore the codebase, fix a bug, or open a plan. \
         Sessions are shared with the Grok TUI for this project."
    } else {
        "Connecting to the agent…"
    };
    let title_row = app
        .session_title
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|t| kit_text::subheading(t.to_string()))
        .unwrap_or_else(|| kit_text::heading(title));

    container(
        column![
            title_row,
            kit_text::body(hint).style(kit_text::muted),
            Space::new().height(SPACE_MD),
            kit_text::caption(short_path(&app.project_root.to_string_lossy()))
                .style(kit_text::muted),
        ]
        .spacing(SPACE_MD)
        .align_x(Alignment::Center)
        .max_width(420.0),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

fn short_path(p: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = p.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    p.to_string()
}

/// Roomier single-line field padding — multi-line feel without a textarea.
const COMPOSER_PAD: Padding = Padding {
    top: 14.0,
    right: 16.0,
    bottom: 14.0,
    left: 16.0,
};

fn composer(app: &App) -> Element<'_, Msg> {
    let gated = app.pending.is_some();
    // Single-line kit text_input: Enter submits. No Shift+Enter newline support.
    let field = if gated {
        text_input("Resolve the pending approval to continue…", &app.draft)
            .size(15)
            .padding(COMPOSER_PAD)
            .style(text_input::style)
            .width(Length::Fill)
    } else if app.streaming {
        // Draft stays editable while streaming; submit is disabled until Stop.
        text_input("Message Grok…", &app.draft)
            .on_input(Msg::DraftChanged)
            .size(15)
            .padding(COMPOSER_PAD)
            .style(text_input::style)
            .width(Length::Fill)
    } else {
        text_input("Message Grok…", &app.draft)
            .on_input(Msg::DraftChanged)
            .on_submit(Msg::Send)
            .size(15)
            .padding(COMPOSER_PAD)
            .style(text_input::style)
            .width(Length::Fill)
    };

    // No Send button — Enter submits. Stop only while a turn is in flight.
    let bar: Element<'_, Msg> = if app.streaming {
        row![
            field,
            kit_btn::labeled("Stop", kit_btn::danger).on_press(Msg::Cancel),
        ]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center)
        .into()
    } else {
        field.into()
    };

    let shell = container(bar)
        .padding(Padding::from([SPACE_SM, SPACE_SM]))
        .width(Length::Fill)
        .style(composer_shell_style);

    container(shell)
        .width(Length::Fill)
        .padding(Padding {
            top: SPACE_MD,
            right: SPACE_XL,
            bottom: SPACE_MD,
            left: SPACE_XL,
        })
        .style(composer_band_style)
        .into()
}

fn composer_band_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.base.color)),
        border: Border {
            color: p.background.stronger.color,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn composer_shell_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: hairline(p, RADIUS_LG),
        ..container::Style::default()
    }
}

