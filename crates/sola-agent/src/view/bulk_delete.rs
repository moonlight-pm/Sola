//! Bulk-delete modal — pick age + safety filters, preview, confirm.

use iced::widget::text::Wrapping;
use iced::widget::{button, checkbox, column, container, row, scrollable, text, Space, Column};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::button as kit_btn;
use sola_kit::components::form::checkbox_style;
use sola_kit::components::style::{
    hairline, RADIUS_LG, RADIUS_MD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS,
};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use crate::sessions::{self, BulkAge};
use crate::view::sidebar;
use crate::{BulkDeletePanel, BulkDeletePhase, Msg};

/// Outer card width — room for path + size without crowding.
const CARD_W: f32 = 560.0;
/// Card padding (kit SPACE_XL is 16; modal wants more air).
const CARD_PAD: f32 = 24.0;
/// Gap between major sections (title / age / filters / list / actions).
const SECTION_GAP: f32 = 18.0;

pub(crate) fn overlay<'a>(base: Element<'a, Msg>, panel: &'a BulkDeletePanel) -> Element<'a, Msg> {
    let card = container(panel_body(panel))
        .width(Length::Fixed(CARD_W))
        .style(card_style);

    let scrim = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(Padding::from([24.0, 24.0]))
        .style(scrim_style);

    iced::widget::stack![base, scrim].into()
}

fn panel_body(panel: &BulkDeletePanel) -> Element<'_, Msg> {
    let n = panel.preview.candidates.len();
    let size = sessions::format_bytes(panel.preview.total_bytes);
    let busy = matches!(panel.phase, BulkDeletePhase::Deleting { .. });

    let header = column![
        kit_text::subheading("Bulk delete sessions"),
        kit_text::body(
            "Permanently remove old Grok sessions from disk. \
             Age is last transcript activity, not last open."
        )
        .style(kit_text::muted),
    ]
    .spacing(SPACE_MD);

    let age_section = column![
        kit_text::caption("Older than").style(kit_text::muted),
        row![
            age_chip(panel, BulkAge::Hours24, busy),
            age_chip(panel, BulkAge::Days7, busy),
            age_chip(panel, BulkAge::Days30, busy),
            age_chip(panel, BulkAge::Any, busy),
        ]
        .spacing(SPACE_MD),
    ]
    .spacing(SPACE_MD);

    let filters_section = column![
        kit_text::caption("Safety filters").style(kit_text::muted),
        column![
            filter_check(
                "Keep pinned sessions",
                panel.criteria.keep_pinned,
                Msg::BulkKeepPinned,
                busy,
            ),
            filter_check(
                "Keep live TUI sessions",
                panel.criteria.keep_live,
                Msg::BulkKeepLive,
                busy,
            ),
            filter_check(
                "Keep currently open session",
                panel.keep_open,
                Msg::BulkKeepOpen,
                busy,
            ),
            filter_check(
                "Only worktree / subagent / OD paths",
                panel.criteria.only_noise_paths,
                Msg::BulkOnlyNoise,
                busy,
            ),
        ]
        .spacing(SPACE_MD),
    ]
    .spacing(SPACE_MD);

    let summary_line = row![
        kit_text::body(format!(
            "{n} session{} match",
            if n == 1 { "" } else { "s" }
        )),
        Space::new().width(Length::Fill),
        kit_text::body(format!("~{size}")).style(kit_text::muted),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let list_section = column![
        kit_text::caption("Matching sessions").style(kit_text::muted),
        summary_line,
        candidate_list(panel, n),
    ]
    .spacing(SPACE_MD);

    let status = status_line(panel);
    let actions = actions_row(panel, n, busy);

    column![
        header,
        age_section,
        filters_section,
        list_section,
        status,
        actions,
    ]
    .spacing(SECTION_GAP)
    .padding(Padding::from([CARD_PAD, CARD_PAD]))
    .into()
}

fn candidate_list(panel: &BulkDeletePanel, n: usize) -> Element<'_, Msg> {
    let inner: Element<'_, Msg> = if panel.preview.candidates.is_empty() {
        container(
            kit_text::caption("No sessions match these filters.").style(kit_text::muted),
        )
        .width(Length::Fill)
        .padding(Padding::from([SPACE_LG, SPACE_MD]))
        .into()
    } else {
        // Card is CARD_W; list pad + scrollbar steal horizontal room.
        // Budget title/path so ellipsis + trailing size both stay readable.
        let max_title = 42usize;
        let max_path = 36usize;
        let rows: Vec<Element<'_, Msg>> = panel
            .preview
            .candidates
            .iter()
            .take(40)
            .map(|c| {
                let title_raw = if c.title.is_empty() {
                    "(untitled)"
                } else {
                    c.title.as_str()
                };
                let title = ellipsize(title_raw, max_title);
                let path = ellipsize(&sidebar::short_path(&c.cwd), max_path);
                let meta = format!(
                    "{} · {}",
                    relative_age(c.updated),
                    sessions::format_bytes(c.bytes)
                );

                // Title (fill, clip) …………… age · size (shrink, never clipped)
                // Path  (fill, clip)
                let title_text = text(title)
                    .font(fonts::ui_medium())
                    .size(13)
                    .wrapping(Wrapping::None)
                    .width(Length::Fill);
                let title_box = container(title_text)
                    .width(Length::Fill)
                    .clip(true);
                let meta_text = text(meta)
                    .size(11)
                    .style(kit_text::muted)
                    .wrapping(Wrapping::None);

                let top = row![title_box, meta_text]
                    .spacing(SPACE_LG)
                    .align_y(Alignment::Center)
                    .width(Length::Fill);

                let path_text = text(path)
                    .size(11)
                    .style(kit_text::muted)
                    .wrapping(Wrapping::None)
                    .width(Length::Fill);
                let path_box = container(path_text).width(Length::Fill).clip(true);

                container(
                    column![top, path_box]
                        .spacing(SPACE_XS + 1.0)
                        .width(Length::Fill),
                )
                .width(Length::Fill)
                .padding(Padding {
                    top: SPACE_SM + 2.0,
                    right: SPACE_SM,
                    bottom: SPACE_SM + 2.0,
                    left: 0.0,
                })
                .into()
            })
            .collect();
        let more: Element<'_, Msg> = if n > 40 {
            kit_text::caption(format!("…and {} more", n - 40))
                .style(kit_text::muted)
                .into()
        } else {
            Space::new().height(0).into()
        };
        // Extra right pad so “N.N MB” clears the scrollbar gutter.
        scrollable(
            column![Column::with_children(rows).width(Length::Fill), more]
                .spacing(SPACE_XS)
                .width(Length::Fill)
                .padding(Padding {
                    top: SPACE_MD,
                    right: SPACE_LG + 4.0,
                    bottom: SPACE_MD,
                    left: SPACE_MD,
                }),
        )
        .height(Length::Fixed(200.0))
        .into()
    };

    container(inner)
        .width(Length::Fill)
        .style(list_panel_style)
        .into()
}

fn status_line(panel: &BulkDeletePanel) -> Element<'_, Msg> {
    match &panel.phase {
        BulkDeletePhase::Idle => Space::new().height(0).into(),
        BulkDeletePhase::Confirm => kit_text::caption(
            "This permanently deletes Grok session history (same as `grok sessions delete`).",
        )
        .style(kit_text::muted)
        .into(),
        BulkDeletePhase::Deleting {
            done,
            total,
            last_id,
        } => {
            let short = if last_id.len() > 8 {
                &last_id[..8]
            } else {
                last_id.as_str()
            };
            kit_text::caption(format!("Deleting… {done}/{total}  ({short}…)"))
                .style(kit_text::muted)
                .into()
        }
        BulkDeletePhase::Done {
            deleted,
            failed,
            errors,
        } => {
            let mut msg = format!("Deleted {deleted}.");
            if *failed > 0 {
                msg.push_str(&format!(" {failed} failed."));
            }
            if let Some(e) = errors.first() {
                msg.push_str(&format!(" ({e})"));
            }
            kit_text::caption(msg).style(kit_text::muted).into()
        }
    }
}

fn filter_check<'a>(
    label: &'a str,
    checked: bool,
    on_toggle: impl Fn(bool) -> Msg + 'a,
    busy: bool,
) -> Element<'a, Msg> {
    let mut cb = checkbox(checked)
        .label(label)
        .size(16.0)
        .spacing(10.0)
        .style(checkbox_style);
    if !busy {
        cb = cb.on_toggle(on_toggle);
    }
    // Give each filter a comfortable hit row height.
    container(cb)
        .width(Length::Fill)
        .padding(Padding::from([4.0, 0.0]))
        .into()
}

fn age_chip(panel: &BulkDeletePanel, age: BulkAge, busy: bool) -> Element<'_, Msg> {
    let selected = panel.criteria.age == age;
    let label = age.label();
    let mut btn = button(text(label).size(12).font(fonts::ui_medium()))
        .padding(Padding::from([8.0, 14.0]))
        .style(move |theme: &Theme, status| age_chip_style(theme, status, selected));
    if !busy {
        btn = btn.on_press(Msg::BulkAge(age));
    }
    btn.into()
}

fn age_chip_style(theme: &Theme, status: button::Status, selected: bool) -> button::Style {
    let p = theme.extended_palette();
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let bg = if selected {
        Color {
            a: 0.95,
            ..p.primary.base.color
        }
    } else if hovered {
        Color {
            a: 0.75,
            ..p.background.strong.color
        }
    } else {
        Color {
            a: 0.55,
            ..p.background.base.color
        }
    };
    let text_color = if selected {
        p.primary.base.text
    } else {
        p.background.base.text
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color,
        border: Border {
            color: Color {
                a: if selected { 0.0 } else { 0.55 },
                ..p.background.stronger.color
            },
            width: if selected { 0.0 } else { 1.0 },
            radius: RADIUS_MD.into(),
        },
        ..button::Style::default()
    }
}

fn actions_row(panel: &BulkDeletePanel, n: usize, busy: bool) -> Element<'_, Msg> {
    // Top hairline separation so actions aren't glued to the list.
    let row_el: Element<'_, Msg> = match &panel.phase {
        BulkDeletePhase::Idle => {
            let mut del = kit_btn::labeled(
                if n == 0 {
                    "Nothing to delete".into()
                } else {
                    format!("Delete {n}…")
                },
                kit_btn::danger_outline,
            );
            if n > 0 && !busy {
                del = del.on_press(Msg::BulkAskConfirm);
            }
            row![
                kit_btn::labeled("Cancel", kit_btn::secondary).on_press(Msg::BulkCancel),
                Space::new().width(Length::Fill),
                del,
            ]
            .spacing(SPACE_MD)
            .align_y(Alignment::Center)
            .into()
        }
        BulkDeletePhase::Confirm => row![
            kit_btn::labeled("Back", kit_btn::secondary).on_press(Msg::BulkBack),
            Space::new().width(Length::Fill),
            kit_btn::labeled(format!("Permanently delete {n}"), kit_btn::danger)
                .on_press(Msg::BulkConfirmDelete),
        ]
        .spacing(SPACE_MD)
        .align_y(Alignment::Center)
        .into(),
        BulkDeletePhase::Deleting { .. } => row![
            Space::new().width(Length::Fill),
            kit_btn::labeled("Deleting…", kit_btn::danger_outline),
        ]
        .spacing(SPACE_MD)
        .into(),
        BulkDeletePhase::Done { .. } => row![
            Space::new().width(Length::Fill),
            kit_btn::labeled("Done", kit_btn::primary).on_press(Msg::BulkCancel),
        ]
        .into(),
    };

    column![
        container(Space::new().height(1.0).width(Length::Fill)).style(divider_style),
        Space::new().height(SPACE_SM),
        row_el,
    ]
    .spacing(0.0)
    .into()
}

fn relative_age(updated: u64) -> String {
    let now = sessions::now_secs();
    let secs = now.saturating_sub(updated);
    if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

fn ellipsize(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let take = max_chars.saturating_sub(1).max(1);
    let t: String = s.chars().take(take).collect();
    format!("{t}…")
}

fn scrim_style(theme: &Theme) -> container::Style {
    let mut c = theme.extended_palette().background.base.color;
    c.a = 0.72;
    container::Style {
        background: Some(Background::Color(c)),
        ..container::Style::default()
    }
}

fn card_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(p.background.weaker.color)),
        border: hairline(p, RADIUS_LG),
        ..container::Style::default()
    }
}

fn list_panel_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.55,
            ..p.background.base.color
        })),
        border: Border {
            color: Color {
                a: 0.5,
                ..p.background.stronger.color
            },
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        ..container::Style::default()
    }
}

fn divider_style(theme: &Theme) -> container::Style {
    let p = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.45,
            ..p.background.stronger.color
        })),
        ..container::Style::default()
    }
}
