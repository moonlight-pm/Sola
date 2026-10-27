//! Link labels a reader can read. Marketing mail is stuffed with tracking
//! URLs: a link whose text is a raw URL, a tracking wrapper, or very long
//! shows its destination (host + path) instead, or "Link" when even that
//! is a tracker. Labelled links (a CTA) keep their words.

use sola_kit::components::prose::{ProseBlock, ProseRun};

/// Rewrite link labels in every block; drop a paragraph that is only
/// unlabelled tracking links.
pub fn humanize(blocks: Vec<ProseBlock>) -> Vec<ProseBlock> {
    blocks
        .into_iter()
        .filter_map(|block| match block {
            ProseBlock::Paragraph(runs) => {
                let runs = label_runs(runs);
                (!only_tracking(&runs)).then_some(ProseBlock::Paragraph(runs))
            }
            ProseBlock::Quote(runs) => Some(ProseBlock::Quote(label_runs(runs))),
            ProseBlock::Heading { level, runs } => Some(ProseBlock::Heading {
                level,
                runs: label_runs(runs),
            }),
            ProseBlock::List { ordered, items } => Some(ProseBlock::List {
                ordered,
                items: items.into_iter().map(label_runs).collect(),
            }),
            other => Some(other),
        })
        .collect()
}

fn label_runs(runs: Vec<ProseRun>) -> Vec<ProseRun> {
    runs.into_iter()
        .map(|mut run| {
            if let Some(url) = &run.url {
                let core = run.text.trim();
                if !core.is_empty() {
                    let label = display_link_label(core, url);
                    if label != core {
                        let lead = &run.text[..run.text.len() - run.text.trim_start().len()];
                        let tail = &run.text[run.text.trim_end().len()..];
                        run.text = format!("{lead}{label}{tail}");
                    }
                }
            }
            run
        })
        .collect()
}

/// ESP footer rows of nothing but "Link" wrappers.
fn only_tracking(runs: &[ProseRun]) -> bool {
    let words: Vec<&ProseRun> = runs.iter().filter(|r| !r.text.trim().is_empty()).collect();
    !words.is_empty()
        && words.iter().all(|r| {
            r.url.as_deref().is_some_and(is_tracking_url) && r.text.trim().eq_ignore_ascii_case("link")
        })
}

pub fn display_link_label(text: &str, url: &str) -> String {
    let t = text.trim();
    if t.is_empty() || looks_like_raw_url(t) || is_tracking_url(t) || t.len() > 48 {
        return destination_label(url);
    }
    t.to_string()
}

/// Host + path, no query. Tracking wrappers stay "Link".
pub fn destination_label(url: &str) -> String {
    if let Some(addr) = url.strip_prefix("mailto:") {
        return addr.split('?').next().unwrap_or(addr).to_string();
    }
    let host = host_of(url);
    if host.is_empty() || is_tracking_host(host) {
        return "Link".into();
    }
    let stripped = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let no_query = stripped.split('?').next().unwrap_or(stripped);
    let no_query = no_query.trim_end_matches('/');
    if no_query.is_empty() || no_query.len() > 56 {
        return host.to_string();
    }
    no_query.to_string()
}

fn host_of(url: &str) -> &str {
    let stripped = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host = stripped.split('/').next().unwrap_or(stripped);
    host.split(':')
        .next()
        .unwrap_or(host)
        .trim_start_matches("www.")
}

fn looks_like_raw_url(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("www.")
}

fn is_tracking_url(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("upn=") || l.contains("utm_") || l.contains("/ls/click") || l.contains("list-manage")
}

fn is_tracking_host(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h.contains("ablink")
        || h.contains("list-manage")
        || h.contains("click.")
        || h.starts_with("click")
        || h.contains("track")
        || h.contains("email.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(text: &str, url: &str) -> ProseRun {
        ProseRun::link(text, url)
    }

    #[test]
    fn a_tracking_href_is_not_the_label() {
        let href = "https://ablink.news.gemini.com/ls/click?upn=u001.SWvH-2F-2Fdx6zVERYLONGTOKEN";
        let out = humanize(vec![ProseBlock::Paragraph(vec![
            ProseRun::text("Trade "),
            link(href, href),
            ProseRun::text(" now"),
        ])]);
        let ProseBlock::Paragraph(runs) = &out[0] else {
            panic!("{out:?}")
        };
        assert_eq!(runs[1].text, "Link");
        assert_eq!(runs[1].url.as_deref(), Some(href));
    }

    #[test]
    fn a_labelled_cta_keeps_its_words_and_spaces() {
        let href = "https://click.list-manage.com/track/click?u=abc&id=def";
        let out = humanize(vec![ProseBlock::Paragraph(vec![link(" Sign in to Wicket ", href)])]);
        let ProseBlock::Paragraph(runs) = &out[0] else {
            panic!("{out:?}")
        };
        assert_eq!(runs[0].text, " Sign in to Wicket ");
    }

    #[test]
    fn a_raw_url_shows_its_destination() {
        let token = "A".repeat(43);
        let url = format!("https://auth.naturalethic.com/login/magic/verify?token={token}");
        assert_eq!(
            display_link_label(&url, &url),
            "auth.naturalethic.com/login/magic/verify"
        );
        assert_eq!(destination_label("mailto:a@b.co?subject=x"), "a@b.co");
    }

    #[test]
    fn rows_of_only_tracking_links_go() {
        let href = "https://click.example.com/ls/click?upn=x";
        let out = humanize(vec![
            ProseBlock::Paragraph(vec![link(href, href), ProseRun::text(" "), link(href, href)]),
            ProseBlock::Paragraph(vec![ProseRun::text("kept")]),
        ]);
        assert_eq!(out.len(), 1);
    }
}
