//! The agent islands' look: an island per session in the top row (mark,
//! name, what it wants) and a **+N** island past the room, listing the rest.
//! Click one and its session's window comes up. Hover one and a close takes
//! the mark's place, so nothing moves: it lets the island go.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, hover, row, text};
use iced::{Alignment, Border, Color, Element, Font, Length, Padding, Theme};
use sola_bus::topics::{AgentIsland, IslandState};
use sola_kit::components::button as kit_button;
use sola_kit::components::{SidebarIndicator, icon_colored, popover, status_mark};
use sola_kit::fonts;

use crate::app::{Msg, Shell};
use crate::menubar::view::{island, item};
use crate::menubar::{CHROME_SIZE, ISLAND_GAP, ISLAND_PAD_H};

/// The +N list's width: an island's widest, and the row and panel pads.
pub const LIST_WIDTH: f32 = ISLAND_MAX + 2.0 * (LIST_ROW_PAD_H + LIST_PAD) + LEAD + GAP;
/// An island's widest; past it, what it says is cut with an ellipsis.
const ISLAND_MAX: f32 = 340.0;
/// The status mark, and the slot it sits in: the close that takes its place
/// is a bigger target than the mark is a picture.
const MARK: f32 = 12.0;
const LEAD: f32 = 20.0;
/// Between the words.
const GAP: f32 = 6.0;
/// Before the slot and after it: the mark itself stays [`ISLAND_PAD_H`] in
/// and [`GAP`] from the words.
const LEAD_PAD: f32 = ISLAND_PAD_H - (LEAD - MARK) / 2.0;
const LEAD_GAP: f32 = GAP - (LEAD - MARK) / 2.0;
/// The `+N` island.
const MORE_W: f32 = 44.0;
/// Room left before the row's middle (the toast).
const MIDDLE_MARGIN: f32 = 40.0;
/// What it says after who, against who.
const TITLE_A: f32 = 0.55;
/// The close's wash under the pointer.
const CLOSE_WASH_A: f32 = 0.16;
/// The +N list: a row, its pad, the gap between rows, the panel's pad.
const LIST_ROW_H: f32 = LEAD + 2.0 * LIST_ROW_PAD_V;
const LIST_ROW_PAD_V: f32 = 6.0;
const LIST_ROW_PAD_H: f32 = 8.0;
const LIST_ROW_GAP: f32 = 2.0;
const LIST_PAD: f32 = 6.0;

/// An island laid out: which, its width, its words cut to fit.
struct Laid<'a> {
    island: &'a AgentIsland,
    width: f32,
    head: String,
    tab: Option<String>,
    want: String,
}

/// The islands that fit on the left, and how many did not.
fn layout(shell: &Shell) -> (Vec<Laid<'_>>, usize) {
    let output_w = shell.output_size.map_or(1920.0, |(w, _)| w as f32);
    let start = crate::menubar::workspaces::row_end(shell) + ISLAND_GAP;
    let room = output_w / 2.0 - MIDDLE_MARGIN - start;
    let all = &shell.agents.islands;
    let mut laid: Vec<Laid<'_>> = Vec::new();
    let mut used = 0.0;
    for (i, isl) in all.iter().enumerate() {
        let l = measure(isl);
        let rest = all.len() - i - 1;
        let reserve = if rest > 0 { ISLAND_GAP + MORE_W } else { 0.0 };
        let gap = if laid.is_empty() { 0.0 } else { ISLAND_GAP };
        if used + gap + l.width + reserve > room && !laid.is_empty() {
            break;
        }
        used += gap + l.width;
        laid.push(l);
    }
    let more = all.len() - laid.len();
    (laid, more)
}

/// Who an island is: its project in medium, then its tab, in parentheses on
/// the island (`✓ Sola (root)`; Joshua, 2026-10-24). The tab alone without a
/// project; no tab when they say the same.
fn who(isl: &AgentIsland) -> (&str, Option<&str>) {
    let project = isl.project.trim();
    let tab = isl.name.trim();
    if project.is_empty() {
        (tab, None)
    } else if tab.is_empty() || tab.eq_ignore_ascii_case(project) {
        (project, None)
    } else {
        (project, Some(tab))
    }
}

/// What the island adds after who: what a waiting session wants. A
/// finished one says nothing more; its check is the word.
fn want(isl: &AgentIsland) -> &str {
    match isl.state {
        IslandState::Waiting => isl.title.trim(),
        IslandState::Done => "",
    }
}

/// `room` shared by a tab and what it wants, wide as each needs: neither
/// is cut while the other can give way, and two long ones get half each.
fn share(room: f32, tab: f32, want: f32) -> (f32, f32) {
    let half = room / 2.0;
    if tab + want <= room {
        (tab, want)
    } else if tab <= half {
        (tab, room - tab)
    } else if want <= half {
        (room - want, want)
    } else {
        (half, half)
    }
}

/// An island's words, cut to fit [`ISLAND_MAX`], and its width.
fn measure(isl: &AgentIsland) -> Laid<'_> {
    let width = |t: &str, f: Font| fonts::line_width(t, CHROME_SIZE, f);
    let chrome = fonts::chrome();
    let cut = |t: &str, room: f32| fonts::fit_line(t, CHROME_SIZE, chrome, room);
    let (head, tab) = who(isl);
    let want = want(isl);
    let mut fixed = LEAD_PAD + LEAD + LEAD_GAP + width(head, name_font()) + ISLAND_PAD_H;
    if tab.is_some() {
        fixed += GAP + width("()", chrome);
    }
    if !want.is_empty() {
        fixed += GAP;
    }
    let room = (ISLAND_MAX - fixed).max(0.0);
    let tab_w = tab.map_or(0.0, |t| width(t, chrome));
    let (tab_room, want_room) = share(room, tab_w, width(want, chrome));
    let tab = tab.map(|t| format!("({})", cut(t, tab_room)));
    let want = if want.is_empty() {
        String::new()
    } else {
        cut(want, want_room)
    };
    // The words as they will be: a cut one shapes differently.
    let mut total = LEAD_PAD + LEAD + LEAD_GAP + width(head, name_font()) + ISLAND_PAD_H;
    if let Some(tab) = &tab {
        total += GAP + width(tab, chrome);
    }
    if !want.is_empty() {
        total += GAP + width(&want, chrome);
    }
    Laid {
        island: isl,
        width: total.ceil(),
        head: head.to_string(),
        tab,
        want,
    }
}

/// `lead` in the mark's slot, then who and what it wants on one line.
fn words<'a>(l: &Laid<'_>, lead: Element<'a, Msg>, muted: Color) -> Element<'a, Msg> {
    let line = |s: &str, font: Font| {
        text(s.to_string())
            .font(font)
            .size(CHROME_SIZE)
            .wrapping(Wrapping::None)
    };
    let mut said = row![line(&l.head, name_font())]
        .spacing(GAP)
        .align_y(Alignment::Center);
    if let Some(tab) = &l.tab {
        said = said.push(line(tab, fonts::chrome()).color(muted));
    }
    if !l.want.is_empty() {
        said = said.push(line(&l.want, fonts::chrome()).color(muted));
    }
    let slot = container(lead)
        .width(Length::Fixed(LEAD))
        .align_x(Alignment::Center);
    row![slot, said]
        .spacing(LEAD_GAP)
        .align_y(Alignment::Center)
        .into()
}

pub(crate) fn name_font() -> Font {
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

/// The close: lets island `key` go. The session goes on.
fn close(key: &str, ink: Color) -> Element<'static, Msg> {
    button(
        container(icon_colored("lucide/x", MARK as u16, ink))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .width(Length::Fixed(LEAD))
    .height(Length::Fixed(LEAD))
    .padding(0)
    .style(|theme: &Theme, status| {
        let under = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: under.then(|| {
                Color {
                    a: CLOSE_WASH_A,
                    ..theme.palette().text
                }
                .into()
            }),
            text_color: theme.palette().text,
            border: Border {
                radius: (LEAD / 2.0).into(),
                ..Border::default()
            },
            shadow: Default::default(),
            snap: false,
        }
    })
    .on_press(Msg::AgentIslandDismiss(key.to_string()))
    .into()
}

/// An island: at rest its mark; under the pointer the same island lifted,
/// the close in the mark's place.
fn pill<'a>(l: &Laid<'_>, fg: Color) -> Element<'a, Msg> {
    let muted = Color { a: TITLE_A, ..fg };
    let key = &l.island.key;
    let face = |lead: Element<'a, Msg>, lifted: bool| {
        let body = container(words(l, lead, muted)).padding(Padding {
            left: LEAD_PAD,
            right: ISLAND_PAD_H,
            ..Padding::ZERO
        });
        island(item(body, Msg::AgentIslandShow(key.clone())), lifted, 0.0)
    };
    container(hover(face(mark(l.island.state), false), face(close(key, fg), true)))
        .width(Length::Fixed(l.width))
        .into()
}

/// The islands for the top row's left, after the flower.
pub fn row_islands(shell: &Shell) -> Vec<Element<'_, Msg>> {
    let fg = shell.theme.palette().text;
    let (laid, more) = layout(shell);
    let mut out: Vec<Element<'_, Msg>> = laid.iter().map(|l| pill(l, fg)).collect();
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

/// The right edge of the top row's left group: the flower, then the
/// islands laid out, then the +N island.
pub fn left_group_end(shell: &Shell) -> f32 {
    let more = layout(shell).1;
    let x = list_x(shell);
    if more > 0 {
        x + MORE_W
    } else {
        x - ISLAND_GAP
    }
}

/// Where the +N list hangs: under the +N island's left edge.
pub fn list_x(shell: &Shell) -> f32 {
    let mut x = crate::menubar::workspaces::row_end(shell) + ISLAND_GAP;
    for l in &layout(shell).0 {
        x += l.width + ISLAND_GAP;
    }
    x
}

/// How many islands did not fit: what the +N list shows.
pub fn more(shell: &Shell) -> usize {
    layout(shell).1
}

/// The +N list: the islands that did not fit. Click a row for its window;
/// the close lets it go and leaves the list up for the next.
pub fn list(shell: &Shell) -> Element<'_, Msg> {
    let fg = shell.theme.palette().text;
    let muted = Color { a: TITLE_A, ..fg };
    let shown = shell.agents.islands.len() - more(shell);
    let rows = shell.agents.islands.iter().skip(shown).map(|isl| {
        let l = measure(isl);
        let go = button(words(&l, mark(isl.state), muted))
            .style(kit_button::menu_item)
            .padding(Padding::from([LIST_ROW_PAD_V, LIST_ROW_PAD_H]))
            .width(Length::Fill)
            .on_press(Msg::AgentIslandShow(isl.key.clone()));
        row![go, close(&isl.key, fg)]
            .spacing(GAP)
            .align_y(Alignment::Center)
            .into()
    });
    popover(container(column(rows).spacing(LIST_ROW_GAP)).padding(LIST_PAD))
        .width(Length::Fill)
        .into()
}

/// The list overlay's height.
pub fn list_height(shell: &Shell) -> f32 {
    more(shell) as f32 * (LIST_ROW_H + LIST_ROW_GAP) + 36.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn island(project: &str, name: &str, state: IslandState, title: &str) -> AgentIsland {
        AgentIsland {
            key: "p".into(),
            name: name.into(),
            project: project.into(),
            state,
            title: title.into(),
            since: 0,
        }
    }

    #[test]
    fn an_island_names_its_project_then_its_tab() {
        let done = island("Sola", "root", IslandState::Done, "finished");
        assert_eq!(who(&done), ("Sola", Some("root")));
        assert_eq!(want(&done), "");
        let asks = island("Sola", "kit-foundation", IslandState::Waiting, "wants to run ls");
        assert_eq!(want(&asks), "wants to run ls");
        // No project: the tab alone; the same name: once.
        assert_eq!(who(&island("", "root", IslandState::Done, "")), ("root", None));
        assert_eq!(who(&island("Oath", "oath", IslandState::Done, "")), ("Oath", None));
    }

    #[test]
    fn a_tab_and_what_it_wants_share_the_room() {
        // Both fit: as they are.
        assert_eq!(share(100.0, 30.0, 40.0), (30.0, 40.0));
        // A short one keeps its width; the long one gets the rest.
        assert_eq!(share(100.0, 20.0, 400.0), (20.0, 80.0));
        assert_eq!(share(100.0, 400.0, 20.0), (80.0, 20.0));
        // Two long ones: half each, so what it wants never vanishes.
        assert_eq!(share(100.0, 400.0, 400.0), (50.0, 50.0));
    }

    #[test]
    fn a_long_session_name_is_cut_to_the_islands_widest() {
        let long = "sc-16061 [review] - Link awaiting-charges emails to their notification";
        let done = island("Illuno", long, IslandState::Done, "finished");
        let l = measure(&done);
        assert!(l.width <= ISLAND_MAX + 1.0, "{} wide", l.width);
        let tab = l.tab.as_deref().unwrap();
        assert!(tab.starts_with("(sc-16061") && tab.ends_with("…)"), "{tab}");
        // Waiting with as long a name still says what it wants.
        let asks = island("Illuno", long, IslandState::Waiting, "wants to run cargo make build");
        let l = measure(&asks);
        assert!(l.width <= ISLAND_MAX + 1.0, "{} wide", l.width);
        assert!(l.want.starts_with("wants to"), "{}", l.want);
    }

    #[test]
    fn a_short_island_is_as_wide_as_its_words() {
        let done = island("Oath", "root", IslandState::Done, "finished");
        let l = measure(&done);
        assert_eq!(l.tab.as_deref(), Some("(root)"));
        assert!(l.width < ISLAND_MAX / 2.0, "{} wide", l.width);
    }
}
