//! Assistant markdown rendered as owned iced widgets (no borrow of parse tree).
//!
//! Grok TUI paints agent markdown in a monospace terminal grid
//! (`xai-grok-pager` + `md_style`). Match that face with the system mono font
//! for body, headings, and inline code so the scrollback reads like the CLI.
//!
//! Clarity: match sola-terminal — integer 15px size, Basic shaping, absolute
//! line height from mono metrics (fractional sizes rasterize soft).

use iced::font::Weight;
use iced::widget::text::{LineHeight, Rich, Shaping, Wrapping};
use iced::widget::{container, rich_text, span, text, Column};
use iced::{Background, Border, Color, Element, Font, Length, Never, Padding, Theme};
use sola_kit::components::style::RADIUS_MD;
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use crate::Msg;

/// Match sola-terminal default cell glyph size (15px integer).
const BODY_PX: f32 = 15.0;
const CODE_PX: f32 = 14.0;

fn mono() -> Font {
    fonts::mono()
}

fn mono_medium() -> Font {
    Font {
        weight: Weight::Medium,
        ..fonts::mono()
    }
}

/// Absolute line box from real mono metrics (same approach as term cells).
fn mono_lh(px: f32) -> LineHeight {
    let m = fonts::mono_metrics();
    LineHeight::Absolute((m.line_per_em * px).ceil().into())
}

pub(crate) fn render(md: &str, theme: &Theme) -> Element<'static, Msg> {
    let blocks = parse_blocks(md);
    if blocks.is_empty() {
        return plain(md, BODY_PX, false, false, None);
    }

    let mut col = Column::new().spacing(0.0).width(Length::Fill);
    let mut prev: Option<BlockKind> = None;
    for b in blocks {
        let kind = BlockKind::of(&b);
        let top = gap_before(prev, kind);
        let bottom = gap_after(kind);
        let view = block_view(b, theme);
        col = col.push(
            container(view)
                .width(Length::Fill)
                .padding(Padding {
                    top,
                    right: 0.0,
                    bottom,
                    left: 0.0,
                }),
        );
        prev = Some(kind);
    }
    col.into()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Heading,
    Paragraph,
    ListItem,
    Code,
    Rule,
}

impl BlockKind {
    fn of(b: &Block) -> Self {
        match b {
            Block::Heading { .. } => Self::Heading,
            Block::Paragraph(_) => Self::Paragraph,
            Block::ListItem { .. } => Self::ListItem,
            Block::Code(_) => Self::Code,
            Block::Rule => Self::Rule,
        }
    }
}

fn gap_before(prev: Option<BlockKind>, cur: BlockKind) -> f32 {
    match (prev, cur) {
        (None, _) => 0.0,
        // Extra air above section titles.
        (_, BlockKind::Heading) => 14.0,
        // List stack stays tight.
        (Some(BlockKind::ListItem), BlockKind::ListItem) => 2.0,
        (Some(BlockKind::Paragraph), BlockKind::Paragraph) => 10.0,
        (Some(BlockKind::Code), _) | (_, BlockKind::Code) => 10.0,
        _ => 8.0,
    }
}

fn gap_after(kind: BlockKind) -> f32 {
    match kind {
        BlockKind::Heading => 4.0,
        BlockKind::Code => 2.0,
        _ => 0.0,
    }
}

enum Block {
    Heading { level: u8, text: String },
    Paragraph(String),
    ListItem { depth: usize, text: String },
    Code(String),
    Rule,
}

fn block_view(block: Block, theme: &Theme) -> Element<'static, Msg> {
    match block {
        Block::Heading { level, text: t } => {
            // Terminal markdown: modest size steps (not display-type scale).
            let size = match level {
                1 => 16.0,
                2 => 15.0,
                _ => BODY_PX,
            };
            // Soft system cyan (#3dd6f5 family), not purple — blend with white
            // so long titles stay readable on graphite.
            let accent = theme.extended_palette().primary.base.color;
            let color = Color {
                r: (accent.r * 0.72 + 0.92 * 0.28).min(1.0),
                g: (accent.g * 0.72 + 0.94 * 0.28).min(1.0),
                b: (accent.b * 0.72 + 0.98 * 0.28).min(1.0),
                a: 1.0,
            };
            plain_lh(&t, size, true, false, Some(color))
        }
        Block::Paragraph(t) => inline_rich(&t, BODY_PX, theme),
        Block::ListItem { depth, text: t } => {
            let indent = 14.0 + depth as f32 * 14.0;
            let bullet = text("·")
                .font(mono())
                .size(BODY_PX)
                .line_height(mono_lh(BODY_PX))
                .shaping(Shaping::Basic)
                .style(kit_text::muted);
            let body = inline_rich(&t, BODY_PX, theme);
            container(
                iced::widget::row![bullet, body]
                    .spacing(10.0)
                    .align_y(iced::Alignment::Start)
                    .width(Length::Fill),
            )
            .padding(Padding {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: indent,
            })
            .width(Length::Fill)
            .into()
        }
        Block::Code(code) => code_block(&code, theme),
        Block::Rule => container(
            text("—")
                .size(12.0)
                .font(mono())
                .shaping(Shaping::Basic)
                .style(kit_text::muted)
                .width(Length::Fill),
        )
        .padding(Padding::from([4.0, 0.0]))
        .into(),
    }
}

fn plain(
    s: &str,
    size: f32,
    bold: bool,
    muted: bool,
    color: Option<Color>,
) -> Element<'static, Msg> {
    plain_lh(s, size, bold, muted, color)
}

fn plain_lh(
    s: &str,
    size: f32,
    bold: bool,
    muted: bool,
    color: Option<Color>,
) -> Element<'static, Msg> {
    let font = if bold { mono_medium() } else { mono() };
    let mut t = text(s.to_string())
        .font(font)
        .size(size)
        .line_height(mono_lh(size))
        .shaping(Shaping::Basic)
        .wrapping(Wrapping::Word)
        .width(Length::Fill);
    if let Some(c) = color {
        t = t.style(move |_theme: &Theme| iced::widget::text::Style { color: Some(c) });
    } else if muted {
        t = t.style(kit_text::muted);
    }
    t.into()
}

/// Inline markup: `code`, **bold**, *italic*, [label](url) as one wrapping line.
fn inline_rich(s: &str, size: f32, theme: &Theme) -> Element<'static, Msg> {
    if !s.contains('`') && !s.contains('*') && !s.contains('[') {
        return plain(s, size, false, false, None);
    }

    let p = theme.extended_palette();
    let muted = p.secondary.base.text;
    let accent = p.primary.base.color;
    let fg = p.background.base.text;
    let lh = mono_lh(size);

    let mut spans: Vec<iced::widget::text::Span<'static, Never>> = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**") {
                let (inner, tail) = after.split_at(end);
                spans.push(
                    span(inner.to_string())
                        .font(mono_medium())
                        .size(size)
                        .line_height(lh)
                        .color(fg),
                );
                rest = &tail[2..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('*') {
            if let Some(end) = after.find('*') {
                let (inner, tail) = after.split_at(end);
                spans.push(
                    span(inner.to_string())
                        .font(mono())
                        .size(size)
                        .line_height(lh)
                        .color(muted),
                );
                rest = &tail[1..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`') {
                let (inner, tail) = after.split_at(end);
                spans.push(
                    span(inner.to_string())
                        .font(mono())
                        .size(CODE_PX)
                        .line_height(lh)
                        .color(fg),
                );
                rest = &tail[1..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('[') {
            if let Some(label_end) = after.find("](") {
                let label = &after[..label_end];
                let url_part = &after[label_end + 2..];
                if let Some(url_end) = url_part.find(')') {
                    spans.push(
                        span(label.to_string())
                            .font(mono())
                            .size(size)
                            .line_height(lh)
                            .color(accent)
                            .underline(true),
                    );
                    rest = &url_part[url_end + 1..];
                    continue;
                }
            }
        }
        let next = rest
            .find(['*', '`', '['])
            .filter(|&i| i > 0)
            .unwrap_or(rest.len());
        let (chunk, tail) = rest.split_at(next.max(1).min(rest.len()));
        let (chunk, tail) = if chunk.is_empty() {
            rest.split_at(1)
        } else {
            (chunk, tail)
        };
        spans.push(
            span(chunk.to_string())
                .font(mono())
                .size(size)
                .line_height(lh)
                .color(fg),
        );
        rest = tail;
    }

    if spans.is_empty() {
        return plain(s, size, false, false, None);
    }

    let rich: Rich<'_, Never, Msg> = rich_text(spans)
        .size(size)
        .line_height(mono_lh(size))
        .wrapping(Wrapping::Word)
        .width(Length::Fill)
        .on_link_click(iced::never);
    rich.into()
}

fn code_block(code: &str, theme: &Theme) -> Element<'static, Msg> {
    let bg = theme.extended_palette().background.strong.color;
    let border = Color {
        a: 0.40,
        ..theme.extended_palette().background.stronger.color
    };
    container(
        text(code.trim_end().to_string())
            .font(mono())
            .size(CODE_PX)
            .line_height(mono_lh(CODE_PX))
            .shaping(Shaping::Basic)
            .wrapping(Wrapping::Word)
            .width(Length::Fill),
    )
    .padding(Padding::from([10.0, 12.0]))
    .width(Length::Fill)
    .style(move |_t: &Theme| container::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: border,
            width: 1.0,
            radius: RADIUS_MD.into(),
        },
        ..container::Style::default()
    })
    .into()
}

fn parse_blocks(md: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut lines = md.lines().peekable();
    let mut para = String::new();
    let mut in_code = false;
    let mut code = String::new();

    let flush_para = |para: &mut String, out: &mut Vec<Block>| {
        let t = para.trim();
        if !t.is_empty() {
            out.push(Block::Paragraph(t.to_string()));
        }
        para.clear();
    };

    while let Some(line) = lines.next() {
        if in_code {
            if line.starts_with("```") {
                in_code = false;
                out.push(Block::Code(std::mem::take(&mut code)));
            } else {
                if !code.is_empty() {
                    code.push('\n');
                }
                code.push_str(line);
            }
            continue;
        }
        if line.starts_with("```") {
            flush_para(&mut para, &mut out);
            in_code = true;
            code.clear();
            continue;
        }
        if line.trim() == "---" || line.trim() == "***" {
            flush_para(&mut para, &mut out);
            out.push(Block::Rule);
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            flush_para(&mut para, &mut out);
            let mut level = 1u8;
            let mut r = rest;
            while let Some(rr) = r.strip_prefix('#') {
                level = level.saturating_add(1);
                r = rr;
            }
            out.push(Block::Heading {
                level: level.min(6),
                text: r.trim().to_string(),
            });
            continue;
        }
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            flush_para(&mut para, &mut out);
            let depth = (line.len() - trimmed.len()) / 2;
            out.push(Block::ListItem {
                depth,
                text: rest.to_string(),
            });
            continue;
        }
        if trimmed.is_empty() {
            flush_para(&mut para, &mut out);
            continue;
        }
        if !para.is_empty() {
            para.push(' ');
        }
        para.push_str(line.trim());
    }
    if in_code {
        out.push(Block::Code(code));
    }
    flush_para(&mut para, &mut out);
    out
}
