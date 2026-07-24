//! Agent UI composition — two-pane kit layout with chat column.

pub(crate) mod approval;
pub(crate) mod bubble;
pub(crate) mod firstrun;
pub(crate) mod footer;
pub(crate) mod markdown;
pub(crate) mod sidebar;

use iced::widget::{button, column, container, mouse_area, row, scrollable, stack, Space, Column};
use iced::widget::scrollable::Viewport;
use iced::{mouse, Alignment, Background, Border, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{
    hairline, RADIUS_LG, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input;
use sola_kit::components::text_input::text_input;

use crate::{App, Msg};

/// Comfortable chat column width on large displays (Phase E raised from 720).
const CHAT_MAX: f32 = 1100.0;

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

    // Draggable vertical divider between sidebar and main (monitor pattern).
    let divider = mouse_area(
        container(Space::new().width(Length::Fill).height(Length::Fill))
            .style(divider_style)
            .width(Length::Fixed(6.0))
            .height(Length::Fill),
    )
    .interaction(mouse::Interaction::ResizingHorizontally)
    .on_press(Msg::DividerPress);

    let body: Element<'_, Msg> = row![
        container(sidebar::view(app))
            .width(Length::Fixed(app.sidebar_w))
            .height(Length::Fill),
        divider,
        container(main)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(main_pane_style),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into();

    // While dragging, a full-window overlay keeps the resize cursor and
    // prevents siblings from stealing hit-testing mid-drag.
    let body: Element<'_, Msg> = if app.dragging_divider {
        stack![
            body,
            mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                .interaction(mouse::Interaction::ResizingHorizontally),
        ]
        .into()
    } else {
        body
    };

    if let Some(picker) = &app.project_picker {
        return stack_picker(body, picker);
    }
    body
}

fn stack_picker<'a>(base: Element<'a, Msg>, picker: &'a crate::ProjectPicker) -> Element<'a, Msg> {
    // Dimmed overlay with a centered card for project selection.
    let recent: Vec<Element<'a, Msg>> = picker
        .recent
        .iter()
        .take(8)
        .map(|cwd| {
            let label = sidebar::short_path(cwd);
            button(kit_text::body(label))
                .style(kit_btn::ghost)
                .width(Length::Fill)
                .on_press(Msg::PickerPick(cwd.clone()))
                .into()
        })
        .collect();

    let field = text_input("Project directory…", &picker.draft)
        .on_input(Msg::PickerDraft)
        .on_submit(Msg::PickerUse)
        .size(14)
        .style(text_input::style)
        .width(Length::Fill);

    let actions = row![
        kit_btn::labeled("Cancel", kit_btn::secondary).on_press(Msg::PickerCancel),
        kit_btn::labeled("Use", kit_btn::primary).on_press(Msg::PickerUse),
    ]
    .spacing(SPACE_MD);

    let card = container(
        column![
            kit_text::subheading("New session"),
            kit_text::body("Choose a project directory for this conversation.")
                .style(kit_text::muted),
            field,
            if recent.is_empty() {
                Element::from(Space::new().height(0))
            } else {
                column![
                    kit_text::caption("Recent").style(kit_text::muted),
                    Column::with_children(recent).spacing(SPACE_XS),
                ]
                .spacing(SPACE_SM)
                .into()
            },
            actions,
        ]
        .spacing(SPACE_LG)
        .padding(Padding::from([SPACE_XL, SPACE_XL]))
        .max_width(480.0),
    )
    .style(picker_card_style);

    let overlay = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(picker_scrim_style);

    stack![base, overlay].into()
}

fn main_pane_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.base.color)),
        ..container::Style::default()
    }
}

fn divider_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.stronger.color)),
        ..container::Style::default()
    }
}

fn picker_scrim_style(theme: &Theme) -> container::Style {
    let mut c = theme.extended_palette().background.base.color;
    c.a = 0.72;
    container::Style {
        background: Some(Background::Color(c)),
        ..container::Style::default()
    }
}

fn picker_card_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: hairline(p, RADIUS_LG),
        ..container::Style::default()
    }
}

fn transcript(app: &App) -> Element<'_, Msg> {
    let mut bubbles: Vec<Element<'_, Msg>> = Vec::new();

    if app.has_older_history {
        let label = if app.loading_older {
            "Loading older messages…"
        } else {
            "Scroll up for older messages"
        };
        bubbles.push(
            container(kit_text::caption(label).style(kit_text::muted))
                .width(Length::Fill)
                .center_x(Length::Fill)
                .padding(Padding::from([SPACE_SM, 0.0]))
                .into(),
        );
    }

    let inner: Element<'_, Msg> = if app.turns.is_empty() && bubbles.is_empty() {
        empty_transcript(app)
    } else if app.turns.is_empty() {
        Column::with_children(bubbles)
            .spacing(SPACE_LG)
            .width(Length::Fill)
            .into()
    } else {
        for t in &app.turns {
            bubbles.push(bubble::turn_view(t, &app.theme));
        }
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
    .id(crate::transcript_scroll_id())
    .height(Length::Fill)
    .on_scroll(|vp: Viewport| {
        let rel = vp.relative_offset();
        Msg::TranscriptScrolled(rel.y)
    })
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
            kit_text::caption(sidebar::short_path(&app.project_root.to_string_lossy()))
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

    // No rounded shell — field sits flat in the band.
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

    container(bar)
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

