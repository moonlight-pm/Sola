//! The agent islands' look: an island per session in the top row (mark,
//! name, what it wants), a **+N** island past the room, and the card that
//! grows under an island (board 03's waiting card): the ask, its choices or
//! a reply field, and Show.

use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Color, Element, Font, Length, Padding, Theme};
use sola_bus::topics::{AgentIsland, IslandState};
use sola_kit::components::button as kit_button;
use sola_kit::components::text_input::{style as input_style, text_input};
use sola_kit::components::{SidebarIndicator, popover, status_mark};
use sola_kit::fonts;

use crate::app::{Msg, Shell};
use crate::menubar::view::{island, item};
use crate::menubar::{CHROME_SIZE, EDGE, ISLAND_GAP, ISLAND_H, ISLAND_PAD_H};

/// The card's reply field (focused when the card opens with one).
pub const REPLY_INPUT_ID: &str = "agent-island-reply";
/// The card's width (board 03's waiting card).
pub const CARD_WIDTH: f32 = 440.0;
/// An island's widest; past it the title is cut with an ellipsis.
const ISLAND_MAX: f32 = 340.0;
/// The status mark, and the gaps between mark, name and title.
const MARK: f32 = 12.0;
const GAP: f32 = 6.0;
/// The `+N` island.
const MORE_W: f32 = 44.0;
/// Room left before the row's middle (the toast).
const MIDDLE_MARGIN: f32 = 40.0;
/// The title's ink against the name's.
const TITLE_A: f32 = 0.55;
/// The card: its pad, the body's tallest, a line of body.
const CARD_PAD: f32 = 12.0;
const BODY_MAX_H: f32 = 220.0;
const BODY_LINE: f32 = 18.0;
const BODY_SIZE: f32 = 13.0;

/// An island laid out: which, its width, its title cut to fit.
struct Laid<'a> {
    island: &'a AgentIsland,
    width: f32,
    title: String,
}

/// The islands that fit on the left, and how many did not.
fn layout(shell: &Shell) -> (Vec<Laid<'_>>, usize) {
    let output_w = shell.output_size.map_or(1920.0, |(w, _)| w as f32);
    let start = EDGE + ISLAND_H + ISLAND_GAP;
    let room = output_w / 2.0 - MIDDLE_MARGIN - start;
    let all = &shell.agents.islands;
    let mut laid = Vec::new();
    let mut used = 0.0;
    for (i, isl) in all.iter().enumerate() {
        let (width, title) = measure(isl);
        let rest = all.len() - i - 1;
        let reserve = if rest > 0 { ISLAND_GAP + MORE_W } else { 0.0 };
        let gap = if laid.is_empty() { 0.0 } else { ISLAND_GAP };
        if used + gap + width + reserve > room && !laid.is_empty() {
            break;
        }
        used += gap + width;
        laid.push(Laid {
            island: isl,
            width,
            title,
        });
    }
    let more = all.len() - laid.len();
    (laid, more)
}

/// An island's width and its title cut to fit [`ISLAND_MAX`].
fn measure(isl: &AgentIsland) -> (f32, String) {
    let name_w = fonts::line_width(&isl.name, CHROME_SIZE, name_font());
    let fixed = 2.0 * ISLAND_PAD_H + MARK + GAP + name_w + GAP;
    let title = fit(&isl.title, (ISLAND_MAX - fixed).max(0.0));
    let title_w = fonts::line_width(&title, CHROME_SIZE, fonts::chrome());
    ((fixed + title_w).ceil(), title)
}

/// `s` cut with an ellipsis to `max` wide.
fn fit(s: &str, max: f32) -> String {
    let width = |t: &str| fonts::line_width(t, CHROME_SIZE, fonts::chrome());
    if width(s) <= max {
        return s.to_string();
    }
    let mut chars: Vec<char> = s.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let cut: String = chars.iter().collect::<String>().trim_end().to_string() + "…";
        if width(&cut) <= max {
            return cut;
        }
    }
    String::new()
}

fn name_font() -> Font {
    Font {
        weight: iced::font::Weight::Medium,
        ..fonts::chrome()
    }
}

fn mark(state: IslandState) -> Element<'static, Msg> {
    status_mark(match state {
        IslandState::Waiting => SidebarIndicator::Waiting,
        IslandState::Done => SidebarIndicator::Done,
    })
}

/// The islands for the top row's left, after the flower.
pub fn row_islands(shell: &Shell) -> Vec<Element<'_, Msg>> {
    let fg = shell.theme.palette().text;
    let open_key = card_open(shell).then(|| shell.agents.open.clone()).flatten();
    let (laid, more) = layout(shell);
    let mut out: Vec<Element<'_, Msg>> = laid
        .into_iter()
        .map(|l| {
            let content = row![
                mark(l.island.state),
                text(l.island.name.clone()).font(name_font()).size(CHROME_SIZE),
                text(l.title).font(fonts::chrome()).size(CHROME_SIZE).color(Color {
                    a: TITLE_A,
                    ..fg
                }),
            ]
            .spacing(GAP)
            .align_y(Alignment::Center);
            let open = open_key.as_deref() == Some(l.island.key.as_str());
            container(island(
                item(content, Msg::AgentIslandOpen(l.island.key.clone())),
                open,
                ISLAND_PAD_H,
            ))
            .width(Length::Fixed(l.width))
            .into()
        })
        .collect();
    if more > 0 {
        let open = shell.menu_open && shell.open_panel == Some(crate::app::Panel::AgentIslands);
        out.push(
            container(island(
                item(
                    text(format!("+{more}")).font(fonts::chrome()).size(CHROME_SIZE),
                    Msg::AgentIslandList,
                ),
                open,
                ISLAND_PAD_H,
            ))
            .width(Length::Fixed(MORE_W))
            .into(),
        );
    }
    out
}

fn card_open(shell: &Shell) -> bool {
    shell.menu_open && shell.open_panel == Some(crate::app::Panel::AgentIsland)
}

/// Where a card hangs: under island `key`'s left edge, or under the +N
/// island for `None`.
pub fn anchor_x(shell: &Shell, key: Option<&str>) -> f32 {
    let (laid, _) = layout(shell);
    let mut x = EDGE + ISLAND_H + ISLAND_GAP;
    for l in &laid {
        if key == Some(l.island.key.as_str()) {
            return x;
        }
        x += l.width + ISLAND_GAP;
    }
    x
}

/// The card of the island open.
pub fn card(shell: &Shell) -> Element<'_, Msg> {
    let Some(isl) = shell.agents.opened() else {
        return Space::new().into();
    };
    let fg = shell.theme.palette().text;
    let muted = Color { a: TITLE_A, ..fg };
    let ago = ago(isl.since);
    let head = row![
        mark(isl.state),
        text(isl.name.clone()).font(name_font()).size(13),
        text(isl.title.clone())
            .font(fonts::chrome())
            .size(13)
            .color(muted)
            .width(Length::Fill),
        text(if isl.project.is_empty() {
            ago
        } else {
            format!("{} · {ago}", isl.project)
        })
        .font(fonts::chrome())
        .size(11)
        .color(muted),
    ]
    .spacing(GAP)
    .align_y(Alignment::Center);
    let body_font = if isl.title.starts_with("wants to run") {
        fonts::mono()
    } else {
        fonts::ui()
    };
    let body = scrollable(
        text(isl.body.clone())
            .font(body_font)
            .size(BODY_SIZE)
            .width(Length::Fill),
    )
    .height(Length::Fixed(body_height(isl)));
    let mut col = column![head].spacing(10).width(Length::Fill);
    if !isl.body.trim().is_empty() {
        col = col.push(body);
    }
    for (i, choice) in isl.choices.iter().enumerate() {
        let style: fn(&Theme, button::Status) -> button::Style = if i == 0 {
            kit_button::primary
        } else {
            kit_button::secondary
        };
        col = col.push(
            kit_button::labeled(choice.clone(), style)
                .width(Length::Fill)
                .on_press(Msg::AgentIslandChoose(i as u32)),
        );
    }
    if isl.reply {
        let field = text_input(reply_hint(isl), &shell.agents.reply)
            .id(iced::widget::Id::new(REPLY_INPUT_ID))
            .on_input(Msg::AgentIslandReplyInput)
            .on_submit(Msg::AgentIslandReply)
            .style(input_style)
            .padding(Padding::from([6, 8]))
            .size(13)
            .width(Length::Fill);
        col = col.push(field);
    }
    col = col.push(
        row![
            text(if isl.reply {
                "Return sends · Esc closes"
            } else {
                "Esc closes"
            })
            .font(fonts::chrome())
            .size(11)
            .color(muted)
            .width(Length::Fill),
            kit_button::labeled_sm("Show", kit_button::ghost).on_press(Msg::AgentIslandShow),
        ]
        .align_y(Alignment::Center),
    );
    popover(container(col).padding(CARD_PAD))
        .width(Length::Fill)
        .into()
}

fn reply_hint(isl: &AgentIsland) -> &'static str {
    match isl.state {
        IslandState::Done => "Tell it what to do next",
        IslandState::Waiting => "Your own answer",
    }
}

fn body_height(isl: &AgentIsland) -> f32 {
    // A rough count: wrapped lines at the card's width.
    let per_line = ((CARD_WIDTH - 2.0 * CARD_PAD) / (BODY_SIZE * 0.55)).max(20.0) as usize;
    let lines: usize = isl
        .body
        .lines()
        .map(|l| l.chars().count() / per_line + 1)
        .sum();
    (lines.max(1) as f32 * BODY_LINE).min(BODY_MAX_H)
}

/// The overlay's height for the card of `isl`.
pub fn card_height(isl: &AgentIsland) -> f32 {
    let head = 24.0;
    let body = if isl.body.trim().is_empty() {
        0.0
    } else {
        body_height(isl) + 10.0
    };
    let choices = isl.choices.len() as f32 * 40.0;
    let reply = if isl.reply { 44.0 } else { 0.0 };
    let foot = 36.0;
    head + body + choices + reply + foot + 2.0 * CARD_PAD + 24.0
}

/// The +N list: the islands that did not fit, one row each.
pub fn list(shell: &Shell) -> Element<'_, Msg> {
    let fg = shell.theme.palette().text;
    let (laid, _) = layout(shell);
    let rest = shell.agents.islands.iter().skip(laid.len());
    let rows = rest.map(|isl| {
        button(
            row![
                mark(isl.state),
                text(isl.name.clone()).font(name_font()).size(13),
                text(isl.title.clone())
                    .font(fonts::chrome())
                    .size(13)
                    .color(Color { a: TITLE_A, ..fg }),
            ]
            .spacing(GAP)
            .align_y(Alignment::Center),
        )
        .style(kit_button::menu_item)
        .padding(Padding::from([6, 8]))
        .width(Length::Fill)
        .on_press(Msg::AgentIslandOpen(isl.key.clone()))
        .into()
    });
    popover(container(column(rows).spacing(2)).padding(6))
        .width(Length::Fill)
        .into()
}

/// The list overlay's height.
pub fn list_height(shell: &Shell) -> f32 {
    let (laid, more) = layout(shell);
    let _ = laid;
    more as f32 * 32.0 + 36.0
}

/// "now", "3m", "2h".
fn ago(since: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let secs = now.saturating_sub(since) / 1000;
    match secs {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m", secs / 60),
        _ => format!("{}h", secs / 3600),
    }
}
