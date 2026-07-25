//! Assistant markdown rendered as owned iced widgets (no borrow of parse tree).
//!
//! Subset: paragraphs, headings, lists, bold/italic (as medium weight / muted),
//! inline ``code``, fenced code blocks, links as accent text.
//!
//! Inline markup uses iced `rich_text` + `span` so bold/code stay on one
//! wrapping line (a vertical `column` of fragments was the word-per-line bug).

use iced::font::Weight;
use iced::widget::text::{Rich, Wrapping};
use iced::widget::{container, rich_text, span, text, Column};
use iced::{Background, Border, Color, Element, Font, Length, Never, Padding, Theme};
use sola_kit::components::style::{RADIUS_MD, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use crate::Msg;

const BODY_PX: f32 = 14.5;
const CODE_PX: f32 = 12.5;
const HEADING_ACCENT: Color = Color {
    r: 0.72,
    g: 0.58,
    b: 0.95,
    a: 1.0,
};

pub(crate) fn render(md: &str, theme: &Theme) -> Element<'static, Msg> {
    let blocks = parse_blocks(md);
    if blocks.is_empty() {
        return plain(md, BODY_PX, false, false, None);
    }
    let mut col = Column::new().spacing(SPACE_SM).width(Length::Fill);
    for b in blocks {
        col = col.push(block_view(b, theme));
    }
    col.into()
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
            let size = match level {
                1 => 18.0,
                2 => 16.0,
                _ => 14.5,
            };
            // Grok uses accent/medium for section titles — soft purple.
            plain(&t, size, true, false, Some(HEADING_ACCENT))
        }
        Block::Paragraph(t) => inline_rich(&t, BODY_PX, theme),
        Block::ListItem { depth, text: t } => {
            let indent = "  ".repeat(depth);
            let line = format!("{indent}· {t}");
            inline_rich(&line, BODY_PX, theme)
        }
        Block::Code(code) => code_block(&code, theme),
        Block::Rule => container(kit_text::caption("—").style(kit_text::muted))
            .padding(Padding::from([SPACE_XS, 0.0]))
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
    let font = if bold {
        fonts::ui_medium()
    } else {
        fonts::ui()
    };
    let mut t = text(s.to_string())
        .font(font)
        .size(size)
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

    let mut spans: Vec<iced::widget::text::Span<'static, Never>> = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**") {
                let (inner, tail) = after.split_at(end);
                spans.push(
                    span(inner.to_string())
                        .font(medium_font())
                        .size(size)
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
                        .font(fonts::ui())
                        .size(size)
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
                        .font(fonts::mono())
                        .size(CODE_PX)
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
                            .font(fonts::ui())
                            .size(size)
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
                .font(fonts::ui())
                .size(size)
                .color(fg),
        );
        rest = tail;
    }

    if spans.is_empty() {
        return plain(s, size, false, false, None);
    }

    let rich: Rich<'_, Never, Msg> = rich_text(spans)
        .size(size)
        .wrapping(Wrapping::Word)
        .width(Length::Fill)
        .on_link_click(iced::never);
    rich.into()
}

fn medium_font() -> Font {
    let base = fonts::ui_medium();
    Font {
        weight: Weight::Medium,
        ..base
    }
}

fn code_block(code: &str, theme: &Theme) -> Element<'static, Msg> {
    let bg = theme.extended_palette().background.strong.color;
    let border = Color {
        a: 0.45,
        ..theme.extended_palette().background.stronger.color
    };
    container(
        text(code.trim_end().to_string())
            .font(fonts::mono())
            .size(CODE_PX)
            .wrapping(Wrapping::Word)
            .width(Length::Fill),
    )
    .padding(Padding::from([SPACE_SM, SPACE_MD]))
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
