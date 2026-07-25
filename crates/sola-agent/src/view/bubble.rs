//! Transcript stream — Grok Build–style blocks (not chat cards).
//!
//! Reference: xai-org/grok-build `scrollback/blocks/{user,agent,thinking,tool}`.
//! User: ❯ + soft band. Agent: bare markdown. Thought: collapsed header.
//! Tools: verb + short target; consecutive groupable kinds fold ("Read 3 files").

use iced::widget::{column, container, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme};
use sola_kit::components::style::{RADIUS_MD, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use crate::protocol::{ToolTurn, Turn};
use crate::view::markdown;
use crate::Msg;

const STREAM_MAX: f32 = 960.0;
const USER_BODY_PX: f32 = 14.0;
const TOOL_PX: f32 = 12.5;
const CMD_MAX: usize = 72;

/// Semantic class for verb-group labels (mirrors Grok `VerbGroupKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolKind {
    File,
    Edit,
    Command,
    Search,
    Dir,
    WebFetch,
    WebSearch,
    Other,
}

impl ToolKind {
    /// Eager fold only for non-destructive browse kinds (Grok rule).
    fn groupable(self) -> bool {
        matches!(
            self,
            Self::File | Self::Search | Self::Dir | Self::WebFetch | Self::WebSearch
        )
    }

    fn verb(self, running: bool) -> &'static str {
        let (past, present) = match self {
            Self::File => ("Read", "Reading"),
            Self::Edit => ("Edited", "Editing"),
            Self::Command | Self::Other => ("Ran", "Running"),
            Self::Search | Self::WebSearch => ("Searched", "Searching"),
            Self::Dir => ("Listed", "Listing"),
            Self::WebFetch => ("Fetched", "Fetching"),
        };
        if running { present } else { past }
    }

    fn noun(self, count: usize) -> &'static str {
        let (one, many) = match self {
            Self::File | Self::Edit => ("file", "files"),
            Self::Command => ("command", "commands"),
            Self::Search => ("pattern", "patterns"),
            Self::Dir => ("dir", "dirs"),
            Self::WebFetch | Self::WebSearch => ("website", "websites"),
            Self::Other => ("tool", "tools"),
        };
        if count == 1 { one } else { many }
    }
}

/// Render turns as a Grok-like action stream.
pub(crate) fn turns_view<'a>(turns: &'a [Turn], theme: &Theme) -> Vec<Element<'a, Msg>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < turns.len() {
        match &turns[i] {
            Turn::Tool(t) => {
                let kind = classify_tool(&t.tool);
                if kind.groupable() {
                    let start = i;
                    i += 1;
                    while i < turns.len() {
                        if let Turn::Tool(next) = &turns[i] {
                            if classify_tool(&next.tool) == kind {
                                i += 1;
                                continue;
                            }
                        }
                        break;
                    }
                    let tools: Vec<&ToolTurn> = turns[start..i]
                        .iter()
                        .filter_map(|t| match t {
                            Turn::Tool(tt) => Some(tt),
                            _ => None,
                        })
                        .collect();
                    if tools.len() >= 2 {
                        out.push(verb_group_row(kind, &tools, theme));
                    } else if let Some(one) = tools.first() {
                        out.push(tool_row(one, theme));
                    }
                } else {
                    out.push(tool_row(t, theme));
                    i += 1;
                }
            }
            other => {
                out.push(turn_view(other, theme));
                i += 1;
            }
        }
    }
    out
}

fn turn_view<'a>(turn: &'a Turn, theme: &Theme) -> Element<'a, Msg> {
    match turn {
        Turn::User(s) => user_prompt(s, theme),
        Turn::Assistant(s) => agent_message(s, theme),
        Turn::Thought(s) => thought_line(s),
        Turn::Tool(t) => tool_row(t, theme),
        Turn::Plan(entries) => plan_block(entries, theme),
        Turn::Error(s) => error_block(s),
    }
}

// ── User (Grok: ❯ + light band) ─────────────────────────────────────────────

fn user_prompt(body: &str, theme: &Theme) -> Element<'static, Msg> {
    let p = theme.extended_palette();
    let band = Color {
        r: p.background.weaker.color.r * 0.55 + p.background.strong.color.r * 0.45,
        g: p.background.weaker.color.g * 0.55 + p.background.strong.color.g * 0.45,
        b: p.background.weaker.color.b * 0.55 + p.background.strong.color.b * 0.45,
        a: 1.0,
    };
    let arrow = text("❯")
        .font(fonts::ui_medium())
        .size(USER_BODY_PX)
        .style(kit_text::accent);
    let body = text(body.to_string())
        .font(fonts::ui())
        .size(USER_BODY_PX)
        .wrapping(iced::widget::text::Wrapping::Word)
        .width(Length::Fill);

    container(
        row![arrow, body]
            .spacing(10.0)
            .align_y(Alignment::Start)
            .width(Length::Fill),
    )
    .padding(Padding::from([8.0, 12.0]))
    .width(Length::Fill)
    .max_width(STREAM_MAX)
    .style(move |_t: &Theme| container::Style {
        background: Some(Background::Color(band)),
        border: Border {
            radius: RADIUS_MD.into(),
            ..Default::default()
        },
        ..container::Style::default()
    })
    .into()
}

// ── Agent (bare markdown, no card) ──────────────────────────────────────────

fn agent_message(body: &str, theme: &Theme) -> Element<'static, Msg> {
    container(markdown::render(body, theme))
        .width(Length::Fill)
        .max_width(STREAM_MAX)
        .padding(Padding {
            top: 2.0,
            right: 0.0,
            bottom: 2.0,
            left: 2.0,
        })
        .into()
}

// ── Thought (collapsed: "Thought" / "Thinking…") ────────────────────────────

fn thought_line(body: &str) -> Element<'static, Msg> {
    let label = if body.trim().is_empty() {
        "Thinking…"
    } else {
        // We don't persist elapsed_ms on Turn::Thought yet — match Grok's
        // collapsed header without dumping the reasoning body.
        "Thought"
    };
    let bullet = text("·")
        .font(fonts::ui())
        .size(TOOL_PX)
        .style(kit_text::muted);
    let title = text(label)
        .font(fonts::ui_medium())
        .size(TOOL_PX)
        .style(kit_text::muted);

    row![bullet, title]
        .spacing(8.0)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .padding(Padding::from([2.0, 0.0]))
        .into()
}

// ── Tools ───────────────────────────────────────────────────────────────────

fn tool_row(t: &ToolTurn, _theme: &Theme) -> Element<'static, Msg> {
    let kind = classify_tool(&t.tool);
    let running = is_running(&t.status);
    let failed = is_failed(&t.status);
    let label = format_tool_label(kind, &t.tool, running);

    type StyleFn = fn(&Theme) -> iced::widget::text::Style;
    let (bullet_style, title_style): (StyleFn, StyleFn) = if failed {
        (kit_text::danger, kit_text::danger)
    } else if running {
        (kit_text::warning, kit_text::accent)
    } else {
        // Grok muted_collapsed: done tools are quiet.
        (kit_text::muted, kit_text::muted)
    };

    let bullet = text("·").font(fonts::ui()).size(TOOL_PX).style(bullet_style);

    // Verb is medium; target/path stays mono-ish for paths/commands.
    let title = text(label)
        .font(fonts::ui())
        .size(TOOL_PX)
        .style(title_style)
        .wrapping(iced::widget::text::Wrapping::Word)
        .width(Length::Fill);

    let status: Option<(&str, StyleFn)> = if failed {
        Some(("failed", kit_text::danger))
    } else if running {
        Some(("running", kit_text::warning))
    } else {
        None // Grok doesn't stamp "done" on every line — silence is success.
    };

    let mut line = row![bullet, title]
        .spacing(8.0)
        .align_y(Alignment::Center)
        .width(Length::Fill);

    if let Some((s, style)) = status {
        line = line.push(text(s).font(fonts::ui()).size(11.0).style(style));
    }

    container(line)
        .width(Length::Fill)
        .max_width(STREAM_MAX)
        .padding(Padding::from([1.0, 0.0]))
        .into()
}

fn verb_group_row(kind: ToolKind, tools: &[&ToolTurn], _theme: &Theme) -> Element<'static, Msg> {
    let n = tools.len();
    let running = tools.iter().any(|t| is_running(&t.status));
    let failed = tools.iter().any(|t| is_failed(&t.status));
    let label = format!("{} {n} {}", kind.verb(running), kind.noun(n));

    let style: fn(&Theme) -> iced::widget::text::Style = if failed {
        kit_text::danger
    } else if running {
        kit_text::warning
    } else {
        kit_text::muted
    };

    let bullet = text("·").font(fonts::ui()).size(TOOL_PX).style(style);
    let title = text(label)
        .font(fonts::ui_medium())
        .size(TOOL_PX)
        .style(style);

    row![bullet, title]
        .spacing(8.0)
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .padding(Padding::from([1.0, 0.0]))
        .into()
}

fn classify_tool(title: &str) -> ToolKind {
    let t = title.trim();
    let lower = t.to_ascii_lowercase();

    // ACP / Grok titles
    if lower.starts_with("edit ")
        || lower.starts_with("creating ")
        || lower.starts_with("create ")
        || lower.contains("search_replace")
        || lower == "write"
        || lower.starts_with("write ")
        || lower.contains("str_replace")
    {
        return ToolKind::Edit;
    }
    if lower.starts_with("read ")
        || lower.contains("read_file")
        || lower.starts_with("reading ")
    {
        return ToolKind::File;
    }
    if lower.starts_with("execute")
        || lower.contains("run_terminal")
        || lower.contains("bash")
        || lower.starts_with("$ ")
        || lower.starts_with("run ")
    {
        return ToolKind::Command;
    }
    if lower.starts_with("search")
        || lower.contains("grep")
        || lower.contains("glob")
        || lower.starts_with("searched ")
    {
        return ToolKind::Search;
    }
    if lower.starts_with("list")
        || lower.contains("list_dir")
        || lower.starts_with("listed ")
    {
        return ToolKind::Dir;
    }
    if lower.starts_with("web search")
        || lower.starts_with("x search")
        || lower.contains("web_search")
    {
        return ToolKind::WebSearch;
    }
    if lower.starts_with("fetch")
        || lower.contains("web_fetch")
        || lower.starts_with("fetched ")
    {
        return ToolKind::WebFetch;
    }
    // Heuristic: looks like a bare path → read
    if t.contains('/') && !t.contains(' ') && !t.contains('`') {
        return ToolKind::File;
    }
    ToolKind::Other
}

fn format_tool_label(kind: ToolKind, title: &str, running: bool) -> String {
    let title = title.trim();
    match kind {
        ToolKind::File => {
            let path = strip_verb_prefix(title, &["Read ", "Reading ", "read_file "]);
            format!("{} {}", ToolKind::File.verb(running), short_path(path))
        }
        ToolKind::Edit => {
            let path = strip_verb_prefix(
                title,
                &["Edit ", "Editing ", "Creating ", "Create ", "Write "],
            );
            format!("{} {}", ToolKind::Edit.verb(running), short_path(path))
        }
        ToolKind::Command => {
            let cmd = extract_command(title);
            let cmd = peel_cd_prefix(&cmd);
            let cmd = truncate_chars(&cmd, CMD_MAX);
            if running {
                format!("Running `{cmd}`")
            } else {
                format!("$ {cmd}")
            }
        }
        ToolKind::Search => {
            let q = strip_verb_prefix(title, &["Search ", "Searched ", "Searching ", "Grep "]);
            format!("{} {}", ToolKind::Search.verb(running), truncate_chars(q, 56))
        }
        ToolKind::Dir => {
            let p = strip_verb_prefix(title, &["List ", "Listed ", "Listing ", "list_dir "]);
            format!("{} {}", ToolKind::Dir.verb(running), short_path(p))
        }
        ToolKind::WebFetch => {
            let u = strip_verb_prefix(title, &["Fetch ", "Fetched ", "Fetching "]);
            format!("{} {}", ToolKind::WebFetch.verb(running), truncate_chars(u, 56))
        }
        ToolKind::WebSearch => {
            let q = strip_verb_prefix(
                title,
                &["Web search: ", "Web Search: ", "X search: ", "X Search: "],
            );
            format!("{} {}", ToolKind::WebSearch.verb(running), truncate_chars(q, 56))
        }
        ToolKind::Other => {
            let s = truncate_chars(title, CMD_MAX);
            if s.is_empty() {
                if running {
                    "Running tool".into()
                } else {
                    "Tool".into()
                }
            } else {
                s
            }
        }
    }
}

fn strip_verb_prefix<'a>(title: &'a str, prefixes: &[&str]) -> &'a str {
    for p in prefixes {
        if let Some(rest) = title.strip_prefix(p) {
            return rest.trim();
        }
        // case-insensitive
        if title.len() >= p.len() && title[..p.len()].eq_ignore_ascii_case(p) {
            return title[p.len()..].trim();
        }
    }
    title
}

/// Pull command out of `Execute \`cmd\`` / `Execute "cmd"` / raw title.
fn extract_command(title: &str) -> String {
    let t = title.trim();
    for prefix in ["Execute ", "Running ", "Run "] {
        if let Some(rest) = t.strip_prefix(prefix).or_else(|| {
            if t.len() >= prefix.len() && t[..prefix.len()].eq_ignore_ascii_case(prefix) {
                Some(&t[prefix.len()..])
            } else {
                None
            }
        }) {
            let rest = rest.trim();
            if (rest.starts_with('`') && rest.ends_with('`') && rest.len() >= 2)
                || (rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2)
            {
                return rest[1..rest.len() - 1].to_string();
            }
            return rest.to_string();
        }
    }
    t.trim_start_matches("$ ").to_string()
}

/// Drop a leading `cd <path> &&` / `cd <path>;` so headers stay dense (Grok).
fn peel_cd_prefix(cmd: &str) -> String {
    let c = cmd.trim();
    let lower = c.to_ascii_lowercase();
    if !lower.starts_with("cd ") {
        return c.to_string();
    }
    // Find && or ; after the cd argument (naive, good enough for headers).
    if let Some(idx) = c.find("&&") {
        return c[idx + 2..].trim().to_string();
    }
    if let Some(idx) = c.find(';') {
        let rest = c[idx + 1..].trim();
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    c.to_string()
}

fn short_path(path: &str) -> String {
    let path = path.trim().trim_matches('`').trim_matches('"');
    if path.is_empty() {
        return "…".into();
    }
    // Prefer last two segments for long absolute paths.
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() >= 3 && path.starts_with('/') {
        format!("…/{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else if let Some(base) = parts.last() {
        if parts.len() > 1 && base.len() < 24 {
            // show parent/base when short
            format!("{}/{}", parts[parts.len() - 2], base)
        } else {
            (*base).to_string()
        }
    } else {
        truncate_chars(path, 48)
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    let count = s.chars().count();
    if count <= max {
        return s;
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn is_running(status: &str) -> bool {
    let s = status.to_ascii_lowercase();
    s.is_empty()
        || s == "running"
        || s == "pending"
        || s.contains("in_progress")
        || s.contains("in-progress")
        || s == "inprogress"
}

fn is_failed(status: &str) -> bool {
    let s = status.to_ascii_lowercase();
    s.contains("fail") || s.contains("error")
}

// ── Plan / error ────────────────────────────────────────────────────────────

fn plan_block(entries: &[crate::protocol::PlanEntry], theme: &Theme) -> Element<'static, Msg> {
    let accent = theme.extended_palette().primary.base.color;
    let mut lines = column![text("Next")
        .font(fonts::ui_medium())
        .size(12.5)
        .style(move |_t: &Theme| iced::widget::text::Style {
            color: Some(Color {
                r: (accent.r * 0.75 + 1.0 * 0.25).min(1.0),
                g: (accent.g * 0.75 + 1.0 * 0.25).min(1.0),
                b: (accent.b * 0.75 + 1.0 * 0.25).min(1.0),
                a: 1.0,
            }),
        })]
    .spacing(SPACE_XS);

    for e in entries {
        let mark = match e.status.as_str() {
            "completed" => "✓",
            "in_progress" | "in-progress" => "→",
            _ => "·",
        };
        lines = lines.push(
            text(format!("{mark}  {}", e.content))
                .font(fonts::ui())
                .size(13.0)
                .style(kit_text::muted)
                .wrapping(iced::widget::text::Wrapping::Word),
        );
    }

    container(lines)
        .width(Length::Fill)
        .max_width(STREAM_MAX)
        .padding(Padding::from([4.0, 2.0]))
        .into()
}

fn error_block(msg: &str) -> Element<'static, Msg> {
    container(
        column![
            text("Error")
                .font(fonts::ui_medium())
                .size(12.0)
                .style(kit_text::danger),
            text(msg.to_string())
                .font(fonts::ui())
                .size(13.0)
                .style(kit_text::danger)
                .wrapping(iced::widget::text::Wrapping::Word),
        ]
        .spacing(SPACE_XS),
    )
    .width(Length::Fill)
    .max_width(STREAM_MAX)
    .padding(Padding::from([4.0, 2.0]))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peel_cd_keeps_real_command() {
        assert_eq!(
            peel_cd_prefix("cd /home/josh/proj && cargo test"),
            "cargo test"
        );
        assert_eq!(peel_cd_prefix("echo hi"), "echo hi");
    }

    #[test]
    fn extract_execute_backticks() {
        assert_eq!(
            extract_command("Execute `cargo make build`"),
            "cargo make build"
        );
    }

    #[test]
    fn classify_edit_and_read() {
        assert_eq!(classify_tool("Edit src/main.rs"), ToolKind::Edit);
        assert_eq!(classify_tool("Read crates/foo/bar.rs"), ToolKind::File);
        assert_eq!(
            classify_tool("Execute `ls`"),
            ToolKind::Command
        );
    }

    #[test]
    fn short_path_truncates_deep() {
        let p = short_path("/home/joshua/Workspace/Sola/crates/sola-agent/src/view/bubble.rs");
        assert!(p.contains("bubble.rs"), "{p}");
        assert!(p.len() < 40, "{p}");
    }
}
