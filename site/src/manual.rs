//! `docs/manual` as one page: each chapter numbered, every heading an anchor,
//! a link to another chapter turned into an anchor on the page, any other
//! relative link sent to the file in the repository. Raw HTML in the Markdown
//! is shown as text.

use std::collections::HashMap;

use pulldown_cmark::{
    CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd, html,
};

use crate::site::escape;

const REPOSITORY: &str = "https://github.com/moonlight-pm/Sola";

macro_rules! chapter {
    ($name:literal) => {
        ($name, include_str!(concat!("../../docs/manual/", $name, ".md")))
    };
}

/// Chapters in page order. The table in `docs/manual/README.md` lists the
/// same, and a test holds the two together.
pub const CHAPTERS: &[(&str, &str)] = &[
    chapter!("introduction"),
    chapter!("security"),
    chapter!("architecture"),
    chapter!("process-manager"),
    chapter!("bus"),
    chapter!("call"),
    chapter!("compositor"),
    chapter!("session"),
    chapter!("sola-shell"),
    chapter!("theme"),
    chapter!("solactl"),
    chapter!("applications"),
];

pub struct Manual {
    /// Chapters and their sections, as nested lists.
    pub contents: String,
    pub body: String,
}

struct Heading {
    level: HeadingLevel,
    id: String,
    number: String,
    /// The heading's inline HTML (code spans kept).
    title: String,
}

pub fn render() -> Manual {
    let mut contents = String::from("<ol class=\"toc\">");
    let mut body = String::new();
    for (i, (name, text)) in CHAPTERS.iter().enumerate() {
        let events = bare_items(Parser::new_ext(text, Options::ENABLE_TABLES).collect());
        let headings = headings(name, i + 1, &events);

        contents.push_str("<li>");
        let mut sections = String::new();
        for h in &headings {
            let link = format!(
                "<a href=\"#{}\"><span class=\"number\">{}</span><span class=\"title\">{}</span></a>",
                h.id, h.number, h.title
            );
            match h.level {
                HeadingLevel::H1 => contents.push_str(&link),
                HeadingLevel::H2 => sections.push_str(&format!("<li>{link}</li>")),
                _ => {}
            }
        }
        contents.push_str(&format!("<ol>{sections}</ol></li>"));

        body.push_str("<section class=\"chapter\">");
        html::push_html(&mut body, rewrite(name, events, &headings).into_iter());
        body.push_str("</section>\n");
    }
    contents.push_str("</ol>");
    Manual { contents, body }
}

/// A list item's first paragraph is its text: no `<p>`, in a loose list as in
/// a tight one.
fn bare_items(events: Vec<Event>) -> Vec<Event> {
    let mut out = Vec::with_capacity(events.len());
    let mut opened_item = false;
    let mut in_lead = false;
    for event in events {
        if opened_item && matches!(event, Event::Start(Tag::Paragraph)) {
            in_lead = true;
            opened_item = false;
            continue;
        }
        opened_item = matches!(event, Event::Start(Tag::Item));
        if in_lead && matches!(event, Event::End(TagEnd::Paragraph)) {
            in_lead = false;
            continue;
        }
        out.push(event);
    }
    out
}

/// Every heading in a chapter: its id (`<chapter>` for the title,
/// `<chapter>-<slug>` below it, then `-1`, `-2` for a repeat) and its number
/// (`3`, `3.2`, `3.2.1`).
fn headings(chapter: &str, n: usize, events: &[Event]) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut counts = [0usize; 2];
    let mut i = 0;
    while i < events.len() {
        if let Event::Start(Tag::Heading { level, .. }) = &events[i] {
            let start = i + 1;
            while !matches!(events[i], Event::End(TagEnd::Heading(_))) {
                i += 1;
            }
            let inner = &events[start..i];
            let number = match level {
                HeadingLevel::H1 => n.to_string(),
                HeadingLevel::H2 => {
                    counts = [counts[0] + 1, 0];
                    format!("{n}.{}", counts[0])
                }
                HeadingLevel::H3 => {
                    counts[1] += 1;
                    format!("{n}.{}.{}", counts[0], counts[1])
                }
                _ => String::new(),
            };
            let id = if *level == HeadingLevel::H1 {
                chapter.to_string()
            } else {
                let base = slug(&plain(inner));
                let seen = used.entry(base.clone()).or_insert(0);
                *seen += 1;
                match *seen {
                    1 => format!("{chapter}-{base}"),
                    repeat => format!("{chapter}-{base}-{}", repeat - 1),
                }
            };
            let mut title = String::new();
            html::push_html(&mut title, inner.iter().cloned().map(escape_raw));
            out.push(Heading {
                level: *level,
                id,
                number,
                title,
            });
        }
        i += 1;
    }
    out
}

/// The chapter's events as they go on the page: headings one level down (the
/// page's h1 is the manual's title) with their number and anchor, tables in a
/// scrolling wrapper, code blocks highlighted, links rewritten, raw HTML as
/// text.
fn rewrite<'a>(chapter: &str, events: Vec<Event<'a>>, headings: &[Heading]) -> Vec<Event<'a>> {
    let mut out = Vec::with_capacity(events.len() + headings.len() * 3);
    let mut next = headings.iter();
    let mut current: Option<&Heading> = None;
    let mut code: Option<(String, String)> = None;
    for event in events {
        if let Some((_, body)) = &mut code {
            if let Event::Text(text) = &event {
                body.push_str(text);
                continue;
            }
        }
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let h = next.next().expect("headings() saw this heading");
                current = Some(h);
                out.push(Event::Start(Tag::Heading {
                    level: down(level),
                    id: Some(CowStr::from(h.id.clone())),
                    classes: vec![],
                    attrs: vec![],
                }));
                if !h.number.is_empty() {
                    out.push(Event::InlineHtml(
                        format!("<span class=\"number\">{}</span>", h.number).into(),
                    ));
                }
            }
            Event::End(TagEnd::Heading(level)) => {
                if let Some(h) = current.take() {
                    out.push(Event::InlineHtml(
                        format!(
                            "<a class=\"anchor\" href=\"#{}\" aria-label=\"Link to this section\">§</a>",
                            h.id
                        )
                        .into(),
                    ));
                }
                out.push(Event::End(TagEnd::Heading(down(level))));
            }
            Event::Start(Tag::Table(align)) => {
                out.push(Event::Html("<div class=\"table-wrap\">".into()));
                out.push(Event::Start(Tag::Table(align)));
            }
            Event::End(TagEnd::Table) => {
                out.push(Event::End(TagEnd::Table));
                out.push(Event::Html("</div>".into()));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) if !info.trim().is_empty() => {
                        info.trim().to_string()
                    }
                    _ => "text".to_string(),
                };
                code = Some((language, String::new()));
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((language, body)) = code.take() {
                    out.push(Event::Html(code_block(&language, &body).into()));
                }
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest_url = CowStr::from(link(chapter, &dest_url));
                out.push(Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                }));
            }
            other => out.push(escape_raw(other)),
        }
    }
    out
}

fn escape_raw(event: Event) -> Event {
    match event {
        Event::Html(s) | Event::InlineHtml(s) => Event::Text(s),
        other => other,
    }
}

fn down(level: HeadingLevel) -> HeadingLevel {
    match level {
        HeadingLevel::H1 => HeadingLevel::H2,
        HeadingLevel::H2 => HeadingLevel::H3,
        HeadingLevel::H3 => HeadingLevel::H4,
        HeadingLevel::H4 => HeadingLevel::H5,
        _ => HeadingLevel::H6,
    }
}

/// A code block. Shell, Nix, and TOML colour their comments.
fn code_block(language: &str, text: &str) -> String {
    let text = text.strip_suffix('\n').unwrap_or(text);
    let body = match language {
        "bash" | "sh" | "nix" | "toml" => comments(text),
        _ => escape(text),
    };
    format!(
        "<pre class=\"code lang-{}\"><code>{body}</code></pre>\n",
        escape(language)
    )
}

/// A `#` at the start of a word runs to the end of the line.
fn comments(text: &str) -> String {
    text.split('\n')
        .map(|line| {
            let start = line
                .match_indices('#')
                .map(|(at, _)| at)
                .find(|&at| at == 0 || matches!(line.as_bytes()[at - 1], b' ' | b'\t'));
            match start {
                Some(at) => format!(
                    "{}<span class=\"tk-cm\">{}</span>",
                    escape(&line[..at]),
                    escape(&line[at..])
                ),
                None => escape(line),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where a link in a chapter goes on the page: a section of this chapter or
/// another chapter is an anchor here, a relative path is the file in the
/// repository.
fn link(chapter: &str, dest: &str) -> String {
    if dest.starts_with("http://") || dest.starts_with("https://") || dest.starts_with("mailto:") {
        return dest.to_string();
    }
    if let Some(fragment) = dest.strip_prefix('#') {
        return format!("#{chapter}-{fragment}");
    }
    let (file, fragment) = dest.split_once('#').unwrap_or((dest, ""));
    if let Some(name) = file.strip_suffix(".md") {
        if CHAPTERS.iter().any(|(n, _)| *n == name) {
            return match fragment {
                "" => format!("#{name}"),
                fragment => format!("#{name}-{fragment}"),
            };
        }
    }
    let path = resolve(file);
    let kind = if path.ends_with('/') { "tree" } else { "blob" };
    match fragment {
        "" => format!("{REPOSITORY}/{kind}/master/{path}"),
        fragment => format!("{REPOSITORY}/{kind}/master/{path}#{fragment}"),
    }
}

/// `relative`, from `docs/manual`, with `.` and `..` resolved. A trailing `/`
/// is kept.
fn resolve(relative: &str) -> String {
    let joined = format!("docs/manual/{relative}");
    let mut kept: Vec<&str> = Vec::new();
    for segment in joined.split('/') {
        match segment {
            ".." => {
                kept.pop();
            }
            "." | "" => {}
            segment => kept.push(segment),
        }
    }
    let path = kept.join("/");
    if relative.ends_with('/') {
        format!("{path}/")
    } else {
        path
    }
}

/// A heading's text without markup.
fn plain(events: &[Event]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Text(t) | Event::Code(t) => Some(t.as_ref()),
            _ => None,
        })
        .collect()
}

/// GitHub's anchor for a heading, the form the manual's own `#section` links
/// use: lower case, punctuation dropped, spaces to hyphens.
fn slug(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_ascii_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapters_follow_the_manual_readme() {
        let readme = include_str!("../../docs/manual/README.md");
        let listed: Vec<&str> = readme
            .lines()
            .filter_map(|l| l.strip_prefix("| ["))
            .filter_map(|l| l.split_once("](")?.1.split_once(".md)"))
            .map(|(name, _)| name)
            .collect();
        let ours: Vec<&str> = CHAPTERS.iter().map(|(n, _)| *n).collect();
        assert_eq!(listed, ours, "docs/manual/README.md lists these chapters");
    }

    #[test]
    fn each_chapter_has_one_title() {
        for (name, text) in CHAPTERS {
            let titles = Parser::new_ext(text, Options::ENABLE_TABLES)
                .filter(|e| {
                    matches!(
                        e,
                        Event::Start(Tag::Heading {
                            level: HeadingLevel::H1,
                            ..
                        })
                    )
                })
                .count();
            assert_eq!(titles, 1, "{name} has {titles} level-one headings");
        }
    }

    #[test]
    fn links_resolve() {
        assert_eq!(link("bus", "#topics"), "#bus-topics");
        assert_eq!(link("bus", "call.md"), "#call");
        assert_eq!(link("bus", "call.md#owners"), "#call-owners");
        assert_eq!(link("bus", "https://example.com/"), "https://example.com/");
        assert_eq!(
            link("bus", "sola-mail.md"),
            "https://github.com/moonlight-pm/Sola/blob/master/docs/manual/sola-mail.md"
        );
        assert_eq!(
            link("bus", "../specs/"),
            "https://github.com/moonlight-pm/Sola/tree/master/docs/specs/"
        );
    }

    #[test]
    fn every_anchor_has_a_target() {
        let m = render();
        let page = format!("{}{}", m.contents, m.body);
        for href in page.split("href=\"").skip(1) {
            let dest = href.split('"').next().unwrap();
            if let Some(id) = dest.strip_prefix('#') {
                assert!(
                    page.contains(&format!("id=\"{id}\"")),
                    "#{id} has no target"
                );
            } else {
                assert!(
                    dest.contains("://") || dest.starts_with("mailto:"),
                    "{dest} leads nowhere on this site"
                );
            }
        }
    }

    #[test]
    fn repeated_headings_get_a_counter() {
        let events: Vec<Event> =
            Parser::new("# T\n## Same\n## Same\n## Same\n### Other, one\n").collect();
        let ids: Vec<String> = headings("c", 1, &events).into_iter().map(|h| h.id).collect();
        assert_eq!(ids, ["c", "c-same", "c-same-1", "c-same-2", "c-other-one"]);
    }

    #[test]
    fn comments_are_marked_and_text_escaped() {
        assert_eq!(
            comments("a <b>  # c\nx#y\n# z"),
            "a &lt;b&gt;  <span class=\"tk-cm\"># c</span>\nx#y\n<span class=\"tk-cm\"># z</span>"
        );
    }

    #[test]
    fn a_list_items_first_paragraph_is_bare() {
        let mut out = String::new();
        let events = bare_items(Parser::new("- one\n\n  more\n\n- two\n").collect());
        html::push_html(&mut out, events.into_iter());
        assert_eq!(
            out,
            "<ul>\n<li>one\n<p>more</p>\n</li>\n<li>two</li>\n</ul>\n"
        );
    }
}
