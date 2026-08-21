//! Agent UI composition — graphite toolbar + sidebar + chat (sola-agent-ds).

pub(crate) mod approval;
pub(crate) mod bubble;
pub(crate) mod bulk_delete;
pub(crate) mod firstrun;
pub(crate) mod footer;
pub(crate) mod markdown;
pub(crate) mod sidebar;

use iced::widget::scrollable::Viewport;
use iced::widget::text_editor::{self, Binding, KeyPress};
use iced::widget::{Column, Space, button, column, container, row, scrollable, stack, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::style::{
    RADIUS_LG, RADIUS_MD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS, hairline,
};
use sola_kit::components::text as kit_text;
use sola_kit::components::text_input;
use sola_kit::components::text_input::text_input;
use sola_kit::fonts;

use crate::{App, Msg};

const CHAT_MAX: f32 = 1100.0;
const SIDE_PAD: f32 = 28.0;
/// New-session project picker — compact card, not full-window width.
const PICKER_W: f32 = 440.0;
/// Drop the card under the toolbar (New lives top-left).
const PICKER_TOP: f32 = 56.0;

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

    // SidebarPanel owns the kit vertical divider + resize chrome.
    let body_row: Element<'_, Msg> = row![
        sidebar::view(app),
        container(main)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(main_pane_style),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into();

    let shell = column![toolbar(app), body_row]
        .width(Length::Fill)
        .height(Length::Fill);

    // Full-window resize overlay is composed inside SidebarPanel.
    let body: Element<'_, Msg> = shell.into();

    if let Some(panel) = &app.bulk_delete {
        return bulk_delete::overlay(body, panel);
    }
    if let Some(picker) = &app.project_picker {
        return stack_picker(body, picker);
    }
    body
}

fn toolbar(app: &App) -> Element<'_, Msg> {
    let busy = app.streaming || app.pending.is_some();
    let mut new_btn = kit_btn::labeled_sm("+  New", kit_btn::secondary);
    if !busy {
        new_btn = new_btn.on_press(Msg::NewSession);
    }

    let leaf = sidebar::project_leaf(&app.project_root.to_string_lossy());
    let path = sidebar::short_path(&app.project_root.to_string_lossy());
    let connected = app.connected;
    let chip = container(
        row![
            container(Space::new().width(6.0).height(6.0))
                .width(Length::Fixed(6.0))
                .height(Length::Fixed(6.0))
                .style(move |_t: &Theme| container::Style {
                    background: Some(Background::Color(if connected {
                        Color {
                            r: 0.24,
                            g: 0.81,
                            b: 0.56,
                            a: 1.0,
                        }
                    } else {
                        Color {
                            r: 0.91,
                            g: 0.72,
                            b: 0.29,
                            a: 1.0,
                        }
                    })),
                    border: Border {
                        radius: 999.0.into(),
                        ..Default::default()
                    },
                    ..container::Style::default()
                }),
            text(leaf).font(fonts::ui_medium()).size(12),
            text("·").size(12).style(kit_text::muted),
            text(path)
                .font(fonts::mono())
                .size(12)
                .style(kit_text::muted),
        ]
        .spacing(7.0)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([4.0, 10.0]))
    .style(project_chip_style);

    let model = kit_btn::labeled_sm(
        format!("{} · {}", app.backend_label, app.connection_mode.as_str()),
        kit_btn::ghost,
    );

    // Far-right destructive: wipe the open session and start a fresh one
    // in the same project (no picker). Always available — no session just
    // starts new.
    let reset_btn =
        kit_btn::labeled_sm("RESET", kit_btn::danger_outline).on_press(Msg::ResetSession);

    // Fixed height only — never center_y(Length::Fill), which overrides
    // height to Fill and steals half the window in a Fill column.
    container(
        row![
            new_btn,
            chip,
            Space::new().width(Length::Fill),
            model,
            reset_btn,
        ]
        .spacing(10.0)
        .align_y(Alignment::Center)
        .height(Length::Fill)
        .padding(Padding::from([0.0, 12.0])),
    )
    .width(Length::Fill)
    .height(Length::Fixed(40.0))
    .align_y(Alignment::Center)
    .style(toolbar_style)
    .into()
}

fn stack_picker<'a>(base: Element<'a, Msg>, picker: &'a crate::ProjectPicker) -> Element<'a, Msg> {
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
        Space::new().width(Length::Fill),
        kit_btn::labeled("Cancel", kit_btn::secondary).on_press(Msg::PickerCancel),
        kit_btn::labeled("Use", kit_btn::primary).on_press(Msg::PickerUse),
    ]
    .spacing(SPACE_MD);

    // Fixed width on the *container* (column max_width alone still lets the
    // card paint full-bleed). Centered horizontally, parked under the toolbar
    // near "+ New".
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
        .padding(Padding::from([SPACE_XL, SPACE_XL])),
    )
    .width(Length::Fixed(PICKER_W))
    .style(picker_card_style);

    let overlay = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .align_y(Alignment::Start)
        .padding(Padding {
            top: PICKER_TOP,
            right: 24.0,
            bottom: 24.0,
            left: 24.0,
        })
        .style(picker_scrim_style);

    stack![base, overlay].into()
}

fn main_pane_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    // Slightly deeper than canvas — design mixes bg with black.
    let c = Color {
        r: p.background.base.color.r * 0.88,
        g: p.background.base.color.g * 0.88,
        b: p.background.base.color.b * 0.88,
        a: 1.0,
    };
    container::Style {
        background: Some(Background::Color(c)),
        ..container::Style::default()
    }
}

fn toolbar_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.96,
            ..p.background.weaker.color
        })),
        border: Border {
            color: Color {
                a: 0.45,
                ..p.background.stronger.color
            },
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn project_chip_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.55,
            ..p.background.base.color
        })),
        border: Border {
            color: Color {
                a: 0.55,
                ..p.background.stronger.color
            },
            width: 1.0,
            radius: RADIUS_PILL_CHIP.into(),
        },
        ..container::Style::default()
    }
}

const RADIUS_PILL_CHIP: f32 = 999.0;

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
        let older: Element<'_, Msg> = if app.loading_older {
            container(kit_text::caption("Loading earlier messages…").style(kit_text::muted))
                .width(Length::Fill)
                .center_x(Length::Fill)
                .padding(Padding::from([SPACE_SM, 0.0]))
                .into()
        } else {
            container(
                button(kit_text::caption("Load earlier messages").style(kit_text::muted))
                    .on_press(Msg::LoadOlderHistory)
                    .padding(Padding::from([SPACE_SM, SPACE_MD]))
                    .style(|theme: &Theme, status| {
                        let p = theme.extended_palette();
                        let bg = match status {
                            button::Status::Hovered | button::Status::Pressed => Color {
                                a: 0.65,
                                ..p.background.strong.color
                            },
                            _ => Color::TRANSPARENT,
                        };
                        button::Style {
                            background: Some(Background::Color(bg)),
                            text_color: p.secondary.base.text,
                            border: Border {
                                radius: RADIUS_MD.into(),
                                ..Default::default()
                            },
                            ..button::Style::default()
                        }
                    }),
            )
            .width(Length::Fill)
            .center_x(Length::Fill)
            .padding(Padding::from([SPACE_SM, 0.0]))
            .into()
        };
        bubbles.push(older);
    }

    let inner: Element<'_, Msg> = if app.content_loading && app.turns.is_empty() {
        // Keep the pane blank while content is en route — no loading label.
        // Selection already flipped; content arrives on the next message.
        Space::new().width(Length::Fill).height(Length::Fill).into()
    } else if app.turns.is_empty() && bubbles.is_empty() {
        empty_transcript(app)
    } else if app.turns.is_empty() {
        // Gaps are applied per-block in bubble::turns_view (kind-aware).
        Column::with_children(bubbles)
            .spacing(0.0)
            .width(Length::Fill)
            .into()
    } else {
        for el in bubble::turns_view(&app.turns, &app.theme, app.streaming) {
            bubbles.push(el);
        }
        Column::with_children(bubbles)
            .spacing(0.0)
            .width(Length::Fill)
            .into()
    };

    let padded = container(inner)
        .width(Length::Fill)
        .max_width(CHAT_MAX)
        .padding(Padding {
            top: 20.0,
            right: SIDE_PAD,
            bottom: 12.0,
            left: SIDE_PAD,
        });

    scrollable(container(padded).width(Length::Fill).center_x(Length::Fill))
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

/// Composer line metrics (match DS: min ~44px, grow to ~160px).
const COMPOSER_LINE_PX: f32 = 20.0;
const COMPOSER_MIN_H: f32 = 44.0;
const COMPOSER_MAX_H: f32 = 160.0;
const COMPOSER_V_PAD: f32 = 12.0;

fn composer(app: &App) -> Element<'_, Msg> {
    let gated = app.pending.is_some();
    let lines = app.draft.line_count().max(1);
    let height =
        ((lines as f32) * COMPOSER_LINE_PX + COMPOSER_V_PAD).clamp(COMPOSER_MIN_H, COMPOSER_MAX_H);

    let placeholder = if gated {
        "Resolve the pending approval to continue…"
    } else {
        "Ask Sola Agent…"
    };

    // Multi-line editor. Enter → submit is decided in `DraftAction` using
    // `shift_held` (Wayland often omits SHIFT on the Enter event itself).
    // Shift+Enter still produces Edit::Enter with shift_held=true → newline.
    let mut editor = text_editor::TextEditor::new(&app.draft)
        .placeholder(placeholder)
        .size(14.0)
        .padding(Padding::from([6.0, 4.0]))
        .height(Length::Fixed(height))
        .style(composer_editor_style)
        .key_binding(|key_press| composer_key_binding(key_press, app.shift_held));

    if !gated {
        editor = editor.on_action(Msg::DraftAction);
    }

    let field: Element<'_, Msg> = editor.into();

    let actions: Element<'_, Msg> = if app.streaming {
        kit_btn::labeled_sm("Stop", kit_btn::danger_outline)
            .on_press(Msg::Cancel)
            .into()
    } else if gated {
        Space::new().width(0).into()
    } else {
        let mut send = kit_btn::labeled_sm("Send", kit_btn::primary);
        if !app.draft.text().trim().is_empty() {
            send = send.on_press(Msg::Send);
        }
        send.into()
    };

    let shell = container(
        row![field, actions]
            .spacing(10.0)
            .align_y(Alignment::End)
            .padding(Padding::from([10.0, 12.0])),
    )
    .width(Length::Fill)
    .style(composer_shell_style);

    container(shell)
        .width(Length::Fill)
        .padding(Padding {
            top: 10.0,
            right: SIDE_PAD,
            bottom: 12.0,
            left: SIDE_PAD,
        })
        .style(composer_band_style)
        .into()
}

/// Map Enter → Edit::Enter always; App turns plain Enter into submit.
/// Shift detection prefers tracked `shift_held` over the (often empty) mask.
fn composer_key_binding(key_press: KeyPress, shift_held: bool) -> Option<Binding<Msg>> {
    use iced::keyboard;
    use iced::keyboard::key::Named;

    let is_enter = matches!(key_press.key.as_ref(), keyboard::Key::Named(Named::Enter))
        || matches!(
            key_press.modified_key.as_ref(),
            keyboard::Key::Named(Named::Enter)
        );

    if is_enter {
        // Always produce Edit::Enter via the default binding path semantics.
        // App::update intercepts when !shift_held and submits instead.
        let _ = shift_held; // documented for readers; intercept is in update
        return Some(Binding::Enter);
    }

    Binding::from_key_press(key_press)
}

fn composer_band_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.base.color)),
        border: Border {
            color: Color {
                a: 0.45,
                ..p.background.stronger.color
            },
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

fn composer_shell_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.92,
            ..p.background.weaker.color
        })),
        border: Border {
            color: Color {
                a: 0.85,
                ..p.background.stronger.color
            },
            width: 1.0,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

fn composer_editor_style(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let p = theme.extended_palette();
    let _ = status;
    text_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: RADIUS_MD.into(),
        },
        placeholder: Color {
            a: 0.75,
            ..p.secondary.base.color
        },
        value: p.background.base.text,
        selection: p.primary.weak.color,
    }
}
