//! The sheet's parts (`design/vault/vault.css`): plates, item rows,
//! grouped field rows, the code's ring, icon buttons — on the theme's
//! colours and the kit's mixes, no literals.

use std::sync::OnceLock;

use iced::widget::canvas::{self, Canvas, Frame, Path, Stroke};
use iced::widget::{Space, button, column, container, row, svg, text};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Point, Radians, Rectangle,
    Renderer, Theme, mouse,
};
use sola_kit::components::icon::icon_handle;
use sola_kit::components::select::enamel;
use sola_kit::components::style::{
    ON_FILL_DARK, RADIUS_LG, RADIUS_MD, RADIUS_SM, alpha, mix_white,
};
use sola_vault_proto::{ItemKind, ItemSummary};

use super::super::rows;
use super::super::update::VaultMsg;
use crate::app::Msg;

pub(crate) const ROW_H: f32 = 44.0;
pub(crate) const ROW_GAP: f32 = 2.0;
pub(crate) const CAPS_H: f32 = 26.0;

pub(crate) fn msg(m: VaultMsg) -> Msg {
    Msg::Vault(m)
}

// ── Colours ──────────────────────────────────────────────────────────

pub(crate) fn ink(t: &Theme) -> Color {
    t.extended_palette().background.base.text
}

/// Secondary ink (`fg_muted`).
pub(crate) fn ink2(t: &Theme) -> Color {
    t.extended_palette().secondary.base.text
}

/// Tertiary ink: muted, quieter.
pub(crate) fn ink3(t: &Theme) -> Color {
    alpha(ink2(t), 0.72)
}

/// The sheet's face (`bg_raised`).
pub(crate) fn face(t: &Theme) -> Color {
    t.extended_palette().background.weak.color
}

fn row_hover(t: &Theme) -> Color {
    mix_white(face(t), 0.05)
}

fn group_fill(t: &Theme) -> Color {
    mix_white(face(t), 0.03)
}

pub(crate) fn group_edge(t: &Theme) -> Color {
    mix_white(face(t), 0.07)
}

pub(crate) fn rule_color(t: &Theme) -> Color {
    mix_white(face(t), 0.06)
}

// ── Icons ────────────────────────────────────────────────────────────

/// A lucide glyph, loaded once.
pub(crate) fn glyph(name: &'static str) -> svg::Handle {
    static CACHE: OnceLock<std::sync::Mutex<Vec<(&'static str, svg::Handle)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut c = cache.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((_, h)) = c.iter().find(|(n, _)| *n == name) {
        return h.clone();
    }
    let h = icon_handle(&format!("lucide/{name}"));
    c.push((name, h.clone()));
    h
}

/// A glyph in a theme colour.
pub(crate) fn icon<'a>(name: &'static str, size: u16, color: fn(&Theme) -> Color) -> Element<'a, Msg> {
    svg(glyph(name))
        .width(Length::Fixed(f32::from(size)))
        .height(Length::Fixed(f32::from(size)))
        .style(move |t: &Theme, _| svg::Style {
            color: Some(color(t)),
        })
        .into()
}

/// A head's or a row's 28 px icon button.
pub(crate) fn icon_button<'a>(name: &'static str, size: u16, on: Option<VaultMsg>) -> Element<'a, Msg> {
    let b = button(
        container(icon(name, size, ink2))
            .center_x(Length::Fixed(28.0))
            .center_y(Length::Fixed(28.0)),
    )
    .padding(0)
    .width(Length::Fixed(28.0))
    .height(Length::Fixed(28.0))
    .style(sola_kit::components::toolbar::style);
    match on {
        Some(m) => b.on_press(msg(m)).into(),
        None => b.into(),
    }
}

/// A key cap (`↵`).
pub(crate) fn kbd<'a>(label: &'a str) -> Element<'a, Msg> {
    container(text(label).size(10.5).font(sola_kit::fonts::ui_medium()).style(|t: &Theme| {
        text::Style {
            color: Some(ink2(t)),
        }
    }))
    .padding(Padding::from([1, 5]))
    .style(|t: &Theme| container::Style {
        border: Border {
            color: alpha(ink(t), 0.16),
            width: 1.0,
            radius: 4.0.into(),
        },
        ..container::Style::default()
    })
    .into()
}

pub(crate) fn label<'a>(s: impl text::IntoFragment<'a>, size: f32, color: fn(&Theme) -> Color) -> text::Text<'a, Theme> {
    text(s).size(size).style(move |t: &Theme| text::Style {
        color: Some(color(t)),
    })
}

/// A section's heading over rows (`ON THIS SITE`).
pub(crate) fn caps<'a>(s: &str, trailing: Option<Element<'a, Msg>>) -> Element<'a, Msg> {
    let mut r = row![
        label(s.to_uppercase(), 10.0, ink3)
            .font(sola_kit::fonts::ui_medium())
            .width(Length::Fill)
    ]
    .align_y(Alignment::Center);
    if let Some(t) = trailing {
        r = r.push(t);
    }
    container(r)
        .padding(Padding {
            top: 8.0,
            right: 6.0,
            bottom: 4.0,
            left: 6.0,
        })
        .height(Length::Fixed(CAPS_H))
        .align_y(Alignment::End)
        .into()
}

// ── Plates ───────────────────────────────────────────────────────────

/// An item's plate: its first letter on its enamel (a login), or a grey
/// plate with its kind's glyph.
pub(crate) fn plate<'a>(name: &str, kind: ItemKind, size: f32) -> Element<'a, Msg> {
    if kind == ItemKind::Login {
        letter_plate(name, size)
    } else {
        kind_plate(kind_glyph(kind), size)
    }
}

pub(crate) fn letter_plate<'a>(seed: &str, size: f32) -> Element<'a, Msg> {
    let fill = enamel(&seed.to_lowercase());
    let letter = rows::initial(seed);
    container(
        text(letter)
            .size(size * 0.45)
            .font(sola_kit::fonts::ui_medium())
            .color(ON_FILL_DARK),
    )
    .center_x(Length::Fixed(size))
    .center_y(Length::Fixed(size))
    .style(move |_t: &Theme| container::Style {
        background: Some(Background::Color(fill)),
        border: Border {
            radius: (size * 0.26).into(),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

pub(crate) fn kind_plate<'a>(glyph_name: &'static str, size: f32) -> Element<'a, Msg> {
    container(icon(glyph_name, (size * 0.5) as u16, ink2))
        .center_x(Length::Fixed(size))
        .center_y(Length::Fixed(size))
        .style(move |t: &Theme| container::Style {
            background: Some(Background::Color(t.extended_palette().background.strong.color)),
            border: Border {
                color: group_edge(t),
                width: 1.0,
                radius: (size * 0.26).into(),
            },
            ..container::Style::default()
        })
        .into()
}

pub(crate) fn kind_glyph(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Login => "key-round",
        ItemKind::Card => "credit-card",
        ItemKind::Identity => "user",
        ItemKind::SecureNote => "sticky-note",
        ItemKind::SshKey => "file-key",
        ItemKind::BankAccount => "wallet",
        ItemKind::DriversLicense | ItemKind::Passport => "id-card",
    }
}

// ── Item rows ────────────────────────────────────────────────────────

/// What a row shows at its end.
pub(crate) enum Trailing<'a> {
    None,
    /// The chosen row's key hint (`Fill ↵`).
    Hint(&'a str),
    /// A › that opens the record.
    Open(VaultMsg),
    /// A quiet ›.
    Chevron,
}

/// One item: plate, name with its marks, subtitle. `on` is the chosen row.
pub(crate) fn item_row<'a>(
    item: &ItemSummary,
    on: bool,
    trailing: Trailing<'a>,
    press: Option<VaultMsg>,
) -> Element<'a, Msg> {
    let name = if item.name.is_empty() {
        item.kind.label().to_string()
    } else {
        item.name.clone()
    };
    let sub = if item.subtitle.is_empty() {
        item.kind.label().to_string()
    } else {
        item.subtitle.clone()
    };
    item_row_parts(plate(&item.name, item.kind, 28.0), name, sub, item.has_passkey, item.has_totp, on, trailing, press)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn item_row_parts<'a>(
    plate: Element<'a, Msg>,
    name: String,
    sub: String,
    passkey: bool,
    code: bool,
    on: bool,
    trailing: Trailing<'a>,
    press: Option<VaultMsg>,
) -> Element<'a, Msg> {
    let mut title = row![
        label(name, 13.0, ink)
            .font(sola_kit::fonts::ui_medium())
            .wrapping(text::Wrapping::None)
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if passkey {
        title = title.push(icon("fingerprint-pattern", 12, ink3));
    }
    if code {
        title = title.push(icon("clock", 12, ink3));
    }
    let text_col = column![
        title,
        label(sub, 11.5, ink2).wrapping(text::Wrapping::None)
    ]
    .spacing(3)
    .width(Length::Fill)
    .clip(true);
    let end: Element<'a, Msg> = match trailing {
        Trailing::None => Space::new().width(0).into(),
        Trailing::Hint(h) => row![label(h, 11.0, ink2), kbd("↵")]
            .spacing(6)
            .align_y(Alignment::Center)
            .into(),
        Trailing::Open(m) => icon_button("chevron-right", 15, Some(m)),
        Trailing::Chevron => icon("chevron-right", 15, ink3),
    };
    let body = row![plate, text_col, end]
        .spacing(10)
        .align_y(Alignment::Center)
        .height(Length::Fixed(ROW_H))
        .padding(Padding {
            top: 0.0,
            right: 8.0,
            bottom: 0.0,
            left: 6.0,
        });
    let b = button(body)
        .padding(0)
        .width(Length::Fill)
        .style(move |t: &Theme, status| row_style(t, status, on));
    match press {
        Some(m) => b.on_press(msg(m)).into(),
        None => b.into(),
    }
}

fn row_style(t: &Theme, status: button::Status, on: bool) -> button::Style {
    let bg = if on {
        Some(sola_kit::theme::selection())
    } else {
        match status {
            button::Status::Hovered | button::Status::Pressed => Some(row_hover(t)),
            _ => None,
        }
    };
    button::Style {
        background: bg.map(Background::Color),
        text_color: ink(t),
        border: Border {
            radius: RADIUS_MD.into(),
            ..Border::default()
        },
        shadow: Default::default(),
        snap: false,
    }
}

// ── Groups of rows (the record) ─────────────────────────────────────

/// Settings' group: rows on a raised plate, a rule between them.
pub(crate) fn group<'a>(rows: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
    let mut col = column![].width(Length::Fill);
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            col = col.push(
                container(
                    container(Space::new().height(1.0).width(Length::Fill))
                        .style(|t: &Theme| container::Style {
                            background: Some(Background::Color(rule_color(t))),
                            ..container::Style::default()
                        }),
                )
                .padding(Padding {
                    left: 12.0,
                    ..Padding::ZERO
                }),
            );
        }
        col = col.push(r);
    }
    container(col)
        .width(Length::Fill)
        .style(|t: &Theme| container::Style {
            background: Some(Background::Color(group_fill(t))),
            border: Border {
                color: group_edge(t),
                width: 1.0,
                radius: RADIUS_LG.into(),
            },
            ..container::Style::default()
        })
        .into()
}

/// A record row: label over value, its tools at the right.
pub(crate) fn field_row<'a>(
    caption: String,
    value: Element<'a, Msg>,
    tools: Vec<Element<'a, Msg>>,
    copied: bool,
) -> Element<'a, Msg> {
    let caption = label(caption, 11.0, if copied { success } else { ink2 });
    let mut tool_row = row![].spacing(2).align_y(Alignment::Center);
    for t in tools {
        tool_row = tool_row.push(t);
    }
    container(
        row![
            column![caption, value].spacing(3).width(Length::Fill),
            tool_row
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 6.0,
        right: 6.0,
        bottom: 6.0,
        left: 12.0,
    })
    .height(Length::Shrink)
    .width(Length::Fill)
    .into()
}

pub(crate) fn success(t: &Theme) -> Color {
    t.extended_palette().success.base.color
}

pub(crate) fn warning(t: &Theme) -> Color {
    t.extended_palette().warning.base.color
}

pub(crate) fn danger(t: &Theme) -> Color {
    t.extended_palette().danger.base.color
}

/// "✓ Copied" in a row's tools.
pub(crate) fn copied_mark<'a>() -> Element<'a, Msg> {
    row![icon("check", 12, success), label("Copied", 11.0, success)]
        .spacing(5)
        .align_y(Alignment::Center)
        .padding(Padding {
            right: 6.0,
            ..Padding::ZERO
        })
        .into()
}

/// A message on the sheet: a warning with its action, or plain text.
pub(crate) fn notice<'a>(body: String, action: Option<(&'a str, VaultMsg)>, warn: bool) -> Element<'a, Msg> {
    let mut r = row![].spacing(10).align_y(Alignment::Start);
    if warn {
        r = r.push(icon("triangle-alert", 14, warning));
    }
    r = r.push(
        label(body, 12.0, if warn { ink } else { ink2 })
            .wrapping(text::Wrapping::Word)
            .width(Length::Fill),
    );
    if let Some((a, m)) = action {
        r = r.push(
            button(label(a, 12.0, ink).font(sola_kit::fonts::ui_medium()))
                .padding(0)
                .style(|_t: &Theme, _s| button::Style::default())
                .on_press(msg(m)),
        );
    }
    let c = container(r).padding(Padding::from([8, 10])).width(Length::Fill);
    if !warn {
        return c.into();
    }
    c.style(|t: &Theme| {
        let w = warning(t);
        container::Style {
            background: Some(Background::Color(alpha(w, 0.10))),
            border: Border {
                color: alpha(w, 0.28),
                width: 1.0,
                radius: RADIUS_MD.into(),
            },
            ..container::Style::default()
        }
    })
    .into()
}

/// An error under a field.
pub(crate) fn error_line<'a>(e: &str) -> Element<'a, Msg> {
    label(e.to_string(), 11.0, danger)
        .wrapping(text::Wrapping::Word)
        .into()
}

/// The empty state: a glyph, a title, a line, maybe an action.
pub(crate) fn empty<'a>(
    glyph_name: &'static str,
    title: &str,
    body: &str,
    action: Option<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let mut col = column![
        icon(glyph_name, 20, ink3),
        Space::new().height(4.0),
        label(title.to_string(), 13.0, ink).font(sola_kit::fonts::ui_medium()),
        label(body.to_string(), 12.0, ink2)
            .wrapping(text::Wrapping::Word)
            .align_x(Alignment::Center)
            .width(Length::Fixed(260.0)),
    ]
    .spacing(6)
    .align_x(Alignment::Center);
    if let Some(a) = action {
        col = col.push(Space::new().height(4.0)).push(a);
    }
    container(col)
        .padding(Padding {
            top: 24.0,
            right: 16.0,
            bottom: 28.0,
            left: 16.0,
        })
        .center_x(Length::Fill)
        .into()
}

/// A quiet text link (`Not you? Use another account`).
pub(crate) fn link<'a>(lead: &str, act: &str, m: VaultMsg) -> Element<'a, Msg> {
    container(
        button(
            row![label(lead.to_string(), 12.0, ink2), label(act.to_string(), 12.0, ink).font(sola_kit::fonts::ui_medium())]
                .spacing(4),
        )
        .padding(Padding::from([2, 4]))
        .style(|t: &Theme, status| button::Style {
            background: matches!(status, button::Status::Hovered)
                .then(|| Background::Color(row_hover(t))),
            border: Border {
                radius: RADIUS_SM.into(),
                ..Border::default()
            },
            ..button::Style::default()
        })
        .on_press(msg(m)),
    )
    .center_x(Length::Fill)
    .into()
}

// ── The code's ring ──────────────────────────────────────────────────

/// Time left on a code: an arc of its period, the warning colour in its
/// last five seconds, the seconds inside.
pub(crate) fn ring<'a>(left: u32, period: u32) -> Element<'a, Msg> {
    Canvas::new(Ring { left, period })
        .width(Length::Fixed(22.0))
        .height(Length::Fixed(22.0))
        .into()
}

struct Ring {
    left: u32,
    period: u32,
}

impl canvas::Program<Msg> for Ring {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let center = frame.center();
        let radius = bounds.width.min(bounds.height) / 2.0 - 1.5;
        let urgent = self.left <= 5;
        let arc_color = if urgent { warning(theme) } else { ink2(theme) };
        frame.stroke(
            &Path::circle(center, radius),
            Stroke::default()
                .with_width(2.0)
                .with_color(rule_color(theme)),
        );
        let frac = if self.period == 0 {
            0.0
        } else {
            (self.left as f32 / self.period as f32).clamp(0.0, 1.0)
        };
        if frac > 0.0 {
            let start = -std::f32::consts::FRAC_PI_2;
            let arc = Path::new(|p| {
                p.arc(canvas::path::Arc {
                    center,
                    radius,
                    start_angle: Radians(start),
                    end_angle: Radians(start + frac * std::f32::consts::TAU),
                });
            });
            frame.stroke(&arc, Stroke::default().with_width(2.0).with_color(arc_color));
        }
        frame.fill_text(canvas::Text {
            content: self.left.to_string(),
            position: Point::new(center.x, center.y),
            color: if urgent { warning(theme) } else { ink2(theme) },
            size: 9.0.into(),
            font: sola_kit::fonts::ui_medium(),
            align_x: iced::widget::text::Alignment::Center,
            align_y: iced::alignment::Vertical::Center,
            ..canvas::Text::default()
        });
        vec![frame.into_geometry()]
    }
}
