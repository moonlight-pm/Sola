//! Assistant markdown rendered as owned iced widgets (no borrow of parse tree).
//!
//! Subset: paragraphs, headings, lists, bold/italic (as medium weight / muted),
//! inline ``code``, fenced code blocks, links as accent+underline text.

use iced::widget::{column, container, text, Column};
use iced::widget::text::Wrapping;
use iced::{Background, Border, Element, Length, Padding, Theme};
use sola_kit::components::style::{RADIUS_MD, SPACE_MD, SPACE_SM, SPACE_XS};
use sola_kit::components::text as kit_text;
use sola_kit::fonts;

use crate::Msg;

const BODY_PX: f32 = 15.0;
const CODE_PX: f32 = 12.0;

pub(crate) fn render(md: &str, theme: &Theme) -> Element<'static, Msg> {
    let blocks = parse_blocks(md);
    if blocks.is_empty() {
        return plain(md, BODY_PX, false, false);
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
                1 => 20.0,
                2 => 17.0,
                _ => 15.0,
            };
            plain(&t, size, true, false)
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

fn plain(s: &str, size: f32, bold: bool, muted: bool) -> Element<'static, Msg> {
    let font = if bold {
        fonts::ui_medium()
    } else {
        fonts::ui()
    };
    let mut t = text(s.to_string())
        .font(font)
        .size(size)
        .wrapping(Wrapping::Word);
    if muted {
        t = t.style(kit_text::muted);
    }
    t.into()
}

/// Very small inline markup: `code`, **bold**, *italic*, [label](url).
fn inline_rich(s: &str, size: f32, _theme: &Theme) -> Element<'static, Msg> {
    // Fast path: no markup markers.
    if !s.contains('`') && !s.contains('*') && !s.contains('[') {
        return plain(s, size, false, false);
    }

    let mut parts: Vec<Element<'static, Msg>> = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**") {
                let (inner, tail) = after.split_at(end);
                parts.push(plain(inner, size, true, false));
                rest = &tail[2..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('*') {
            if let Some(end) = after.find('*') {
                let (inner, tail) = after.split_at(end);
                parts.push(plain(inner, size, false, true));
                rest = &tail[1..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`') {
                let (inner, tail) = after.split_at(end);
                parts.push(
                    text(inner.to_string())
                        .font(fonts::mono())
                        .size(CODE_PX)
                        .wrapping(Wrapping::Word)
                        .into(),
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
                    parts.push(
                        text(label.to_string())
                            .font(fonts::ui())
                            .size(size)
                            .style(kit_text::accent)
                            .wrapping(Wrapping::Word)
                            .into(),
                    );
                    rest = &url_part[url_end + 1..];
                    continue;
                }
            }
        }
        // Consume until next marker.
        let next = rest
            .find(['*', '`', '['])
            .filter(|&i| i > 0)
            .unwrap_or(rest.len());
        let (chunk, tail) = rest.split_at(next.max(1).min(rest.len()));
        // If we advanced 0 (stuck on marker), take one char.
        let (chunk, tail) = if chunk.is_empty() {
            rest.split_at(1)
        } else {
            (chunk, tail)
        };
        parts.push(plain(chunk, size, false, false));
        rest = tail;
    }

    if parts.len() == 1 {
        return parts.pop().unwrap();
    }
    column(parts).spacing(0).width(Length::Fill).into()
}

fn code_block(code: &str, theme: &Theme) -> Element<'static, Msg> {
    let bg = theme.extended_palette().background.strong.color;
    let border = theme.extended_palette().background.stronger.color;
    container(
        text(code.trim_end().to_string())
            .font(fonts::mono())
            .size(CODE_PX)
            .wrapping(Wrapping::None),
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
