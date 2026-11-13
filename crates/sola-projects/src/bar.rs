//! The bar under a session window's title (Joshua, 2026-10-31: "Since we
//! removed the tabs, we no longer see the number of compactions … Maybe a
//! standard bar at the top of the projects window can show some basic
//! stats"): the agent and its model at the left; its turns, compactions and
//! context at the right ([`crate::stats`]). A shell with no agent shows
//! only that. Claude's status line adds (Joshua, 2026-11-01: "credits
//! quota/remaining, model effort, fast (on/off), context should show
//! percent used"; [`crate::gauges`]): the effort and fast mode by the
//! model, the context as % of its window, and what is left of the
//! account's 5-hour and weekly limits (or a gateway's spend). At its right
//! end, the button that drops the session's terminal ([`crate::dropdown`]),
//! lit while it shows.

use std::sync::LazyLock;

use iced::widget::{Space, button, container, row, svg, text};
use iced::{Alignment, Element, Length, Padding, Theme};
use sola_kit::components::icon::{icon_handle, icon_svg};
use sola_kit::components::style::{BAR_H, PAD_CONTROL_SM, SPACE_LG, SPACE_SM, chrome_surface};
use sola_kit::components::{text as kit_text, toolbar};
use sola_kit::fonts;

use crate::Msg;
use crate::stats::{model_label, short_tokens};
use crate::status::PaneStatus;

/// What the bar says, worked out apart from drawing it.
#[derive(Debug, PartialEq, Eq)]
pub struct Line {
    pub agent: String,
    pub model: Option<String>,
    /// By the model: the effort level, `fast` when fast mode is on.
    pub modes: Vec<String>,
    /// `(figure, word)`, left to right.
    pub figures: Vec<(String, String)>,
}

fn agent_name(agent: Option<&str>) -> String {
    match agent {
        Some("claude") => "Claude".into(),
        Some("codex") => "Codex".into(),
        Some("grok") => "Grok".into(),
        Some(other) => other.to_string(),
        None => "Shell".into(),
    }
}

fn plural(n: u64, one: &str, many: &str) -> String {
    if n == 1 { one.into() } else { many.into() }
}

pub fn line(st: Option<&PaneStatus>) -> Line {
    let agent = st.and_then(|s| s.agent.as_deref());
    let mut figures = Vec::new();
    let tracked = matches!(agent, Some("claude" | "codex" | "grok"));
    if let (Some(st), true) = (st, tracked) {
        let s = &st.stats;
        if s.turns > 0 {
            figures.push((s.turns.to_string(), plural(s.turns.into(), "turn", "turns")));
        }
        let c = u64::from(st.compaction_count);
        figures.push((c.to_string(), plural(c, "compaction", "compactions")));
        let g = &st.gauges;
        let word = match g.window.or(s.window) {
            Some(w) => format!("of {} context", short_tokens(w)),
            None => "context".into(),
        };
        match (g.context_pct, s.context) {
            (Some(p), _) => figures.push((format!("{p}%"), word)),
            (None, Some(n)) => figures.push((short_tokens(n), word)),
            (None, None) => {}
        }
        if let Some(used) = g.five_hour {
            figures.push((format!("{}%", 100 - used.min(100)), "left in 5h".into()));
        }
        if let Some(used) = g.seven_day {
            figures.push((format!("{}%", 100 - used.min(100)), "left this week".into()));
        }
        if let Some((used, limit)) = g.spend {
            figures.push((format!("${used}"), format!("of ${limit}")));
        }
    }
    let modes = st
        .map(|s| {
            let g = &s.gauges;
            g.effort
                .iter()
                .cloned()
                .chain(g.fast.filter(|f| *f).map(|_| "fast".to_string()))
                .collect()
        })
        .unwrap_or_default();
    Line {
        agent: agent_name(agent),
        model: st.and_then(|s| s.stats.model.as_deref()).map(model_label),
        modes,
        figures,
    }
}

/// Read once: the bar is drawn with every redraw of its window.
static TERMINAL: LazyLock<svg::Handle> = LazyLock::new(|| icon_handle("lucide/terminal"));

/// Shows or hides session `tab`'s drop-down; lit while it shows.
fn terminal_button<'a>(tab: &str, dropped: bool) -> Element<'a, Msg> {
    button(icon_svg(TERMINAL.clone(), 16))
        .padding(PAD_CONTROL_SM)
        .style(move |theme: &Theme, status| {
            let mut style = toolbar::style(theme, status);
            // At rest it is clear; shown, it looks held.
            if dropped && status == button::Status::Active {
                style.background = Some(theme.extended_palette().background.strong.color.into());
            }
            style
        })
        .on_press(Msg::DropDown(tab.to_string()))
        .into()
}

pub fn view<'a>(st: Option<&PaneStatus>, tab: &str, dropped: bool) -> Element<'a, Msg> {
    let line = line(st);
    let mut left = row![text(line.agent).font(fonts::ui_medium()).size(13)]
        .spacing(SPACE_SM)
        .align_y(Alignment::Center);
    if let Some(model) = line.model {
        left = left.push(text(model).size(13).style(kit_text::muted));
    }
    for mode in line.modes {
        left = left.push(text(mode).size(13).style(kit_text::muted));
    }
    let mut right = row![].spacing(SPACE_LG).align_y(Alignment::Center);
    for (figure, word) in line.figures {
        right = right.push(
            row![
                text(figure).font(fonts::ui_medium()).size(13),
                text(word).size(13).style(kit_text::muted),
            ]
            .spacing(SPACE_SM)
            .align_y(Alignment::Center),
        );
    }
    let right = right.push(terminal_button(tab, dropped));
    let surface = chrome_surface();
    container(
        row![left, Space::new().width(Length::Fill), right]
            .align_y(Alignment::Center)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fixed(BAR_H))
    .padding(Padding::ZERO.left(14).right(14))
    .align_y(Alignment::Center)
    .style(move |_| container::Style {
        background: Some(surface.into()),
        ..container::Style::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::Stats;

    #[test]
    fn a_claude_session_shows_its_figures() {
        let st = PaneStatus {
            agent: Some("claude".into()),
            compaction_count: 3,
            stats: Stats {
                turns: 42,
                context: Some(324_846),
                window: None,
                model: Some("claude-opus-5-5".into()),
            },
            ..PaneStatus::default()
        };
        assert_eq!(
            line(Some(&st)),
            Line {
                agent: "Claude".into(),
                model: Some("Opus 5.5".into()),
                modes: vec![],
                figures: vec![
                    ("42".into(), "turns".into()),
                    ("3".into(), "compactions".into()),
                    ("325k".into(), "context".into()),
                ],
            }
        );
    }

    #[test]
    fn codex_names_its_window_and_one_reads_singular() {
        let st = PaneStatus {
            agent: Some("codex".into()),
            compaction_count: 1,
            stats: Stats { turns: 1, context: Some(9500), window: Some(272_000), model: None },
            ..PaneStatus::default()
        };
        let l = line(Some(&st));
        assert_eq!(
            l.figures,
            vec![
                ("1".into(), "turn".into()),
                ("1".into(), "compaction".into()),
                ("10k".into(), "of 272k context".into()),
            ]
        );
    }

    #[test]
    fn a_shell_shows_only_that() {
        assert_eq!(
            line(None),
            Line { agent: "Shell".into(), model: None, modes: vec![], figures: vec![] }
        );
    }

    #[test]
    fn the_status_line_adds_effort_fast_percent_and_what_is_left() {
        let st = PaneStatus {
            agent: Some("claude".into()),
            compaction_count: 0,
            stats: Stats { turns: 3, context: Some(446_000), window: None, model: Some("claude-opus-5-5".into()) },
            gauges: crate::gauges::Gauges {
                context_pct: Some(45),
                window: Some(1_000_000),
                effort: Some("xhigh".into()),
                fast: Some(true),
                five_hour: Some(32),
                seven_day: Some(18),
                spend: None,
            },
            ..PaneStatus::default()
        };
        let l = line(Some(&st));
        assert_eq!(l.modes, vec!["xhigh".to_string(), "fast".to_string()]);
        assert_eq!(
            l.figures,
            vec![
                ("3".into(), "turns".into()),
                ("0".into(), "compactions".into()),
                ("45%".into(), "of 1M context".into()),
                ("68%".into(), "left in 5h".into()),
                ("82%".into(), "left this week".into()),
            ]
        );
    }
}
