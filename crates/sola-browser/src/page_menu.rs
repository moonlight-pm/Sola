//! Pure builders for the page context menu and back/forward history list.

use crate::engine::{HistoryEntry, PageContext};
use crate::util;

/// One row in the page (right-click) menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageMenuKind {
    OpenLink,
    CopyLink,
    Copy,
    Cut,
    Paste,
    Back,
    Forward,
    Reload,
    Separator,
}

/// Browser-standard page menu: link / selection / edit, then nav.
pub fn page_menu_kinds(ctx: &PageContext) -> Vec<PageMenuKind> {
    let mut rows = Vec::new();
    let has_link = ctx.link_url.as_deref().is_some_and(|u| !u.is_empty());
    let has_sel = ctx.selection.as_deref().is_some_and(|s| !s.is_empty());
    if has_link {
        rows.push(PageMenuKind::OpenLink);
        rows.push(PageMenuKind::CopyLink);
    }
    if ctx.editable {
        if has_sel {
            rows.push(PageMenuKind::Cut);
            rows.push(PageMenuKind::Copy);
        }
        rows.push(PageMenuKind::Paste);
    } else if has_sel {
        rows.push(PageMenuKind::Copy);
    }
    if !rows.is_empty() {
        rows.push(PageMenuKind::Separator);
    }
    rows.push(PageMenuKind::Back);
    rows.push(PageMenuKind::Forward);
    rows.push(PageMenuKind::Reload);
    rows
}

/// History entries on one side of the current page, closest first.
///
/// Back: previous pages (`index < current`). Forward: later pages.
pub fn history_jump_items(
    entries: &[HistoryEntry],
    current: i32,
    forward: bool,
    limit: usize,
) -> Vec<(i32, String)> {
    let mut items: Vec<(i32, String)> = entries
        .iter()
        .filter(|e| {
            if forward {
                e.index > current
            } else {
                e.index < current
            }
        })
        .map(|e| (e.index, history_label(e)))
        .collect();
    if !forward {
        items.reverse();
    }
    items.truncate(limit.max(1));
    items
}

/// Keep chrome-owned history when CEF only has the current page (restart).
/// Use the live CEF stack when it still covers the prior list (same session).
pub fn merge_tab_history(
    prior: &[HistoryEntry],
    prior_index: i32,
    live: &[HistoryEntry],
    live_url: &str,
    live_title: &str,
) -> (Vec<HistoryEntry>, i32) {
    let url = if live_url.is_empty() {
        live.first().map(|e| e.url.as_str()).unwrap_or("")
    } else {
        live_url
    };
    if prior.is_empty() {
        if live.is_empty() {
            if url.is_empty() {
                return (Vec::new(), 0);
            }
            return (
                vec![HistoryEntry {
                    index: 0,
                    url: url.to_string(),
                    title: live_title.to_string(),
                }],
                0,
            );
        }
        return reindex(live.to_vec(), url);
    }
    if live_covers_prior(prior, live) {
        return reindex(live.to_vec(), url);
    }
    if let Some(e) = prior.iter().find(|e| e.url == url) {
        return (prior.to_vec(), e.index);
    }
    let mut out: Vec<HistoryEntry> = prior
        .iter()
        .filter(|e| e.index <= prior_index)
        .cloned()
        .collect();
    if out.last().map(|e| e.url.as_str()) != Some(url) && !url.is_empty() {
        let next = out.last().map(|e| e.index + 1).unwrap_or(0);
        out.push(HistoryEntry {
            index: next,
            url: url.to_string(),
            title: live_title.to_string(),
        });
    }
    let cur = out.last().map(|e| e.index).unwrap_or(0);
    (out, cur)
}

fn live_covers_prior(prior: &[HistoryEntry], live: &[HistoryEntry]) -> bool {
    if live.len() < prior.len() || prior.is_empty() {
        return false;
    }
    let prior_urls: Vec<&str> = prior.iter().map(|e| e.url.as_str()).collect();
    let live_urls: Vec<&str> = live.iter().map(|e| e.url.as_str()).collect();
    live_urls
        .windows(prior_urls.len())
        .any(|w| w == prior_urls.as_slice())
}

fn reindex(mut entries: Vec<HistoryEntry>, current_url: &str) -> (Vec<HistoryEntry>, i32) {
    let mut current = 0;
    for (i, e) in entries.iter_mut().enumerate() {
        e.index = i as i32;
        if e.url == current_url {
            current = i as i32;
        }
    }
    if current == 0 && !current_url.is_empty() {
        if let Some((i, _)) = entries
            .iter()
            .enumerate()
            .rev()
            .find(|(_, e)| e.url == current_url)
        {
            current = i as i32;
        }
    }
    (entries, current)
}

fn history_label(e: &HistoryEntry) -> String {
    let raw = if !e.title.is_empty() {
        e.title.as_str()
    } else {
        e.url.as_str()
    };
    util::truncate(raw, 48)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::HistoryEntry;

    fn ctx() -> PageContext {
        PageContext::default()
    }

    fn entry(index: i32, title: &str) -> HistoryEntry {
        HistoryEntry {
            index,
            url: format!("https://ex/{index}"),
            title: title.into(),
        }
    }

    #[test]
    fn page_menu_plain_has_nav() {
        let rows = page_menu_kinds(&ctx());
        assert_eq!(
            rows,
            vec![
                PageMenuKind::Back,
                PageMenuKind::Forward,
                PageMenuKind::Reload
            ]
        );
    }

    #[test]
    fn page_menu_link_and_selection() {
        let rows = page_menu_kinds(&PageContext {
            link_url: Some("https://ex/a".into()),
            selection: Some("hi".into()),
            ..ctx()
        });
        assert_eq!(
            rows,
            vec![
                PageMenuKind::OpenLink,
                PageMenuKind::CopyLink,
                PageMenuKind::Copy,
                PageMenuKind::Separator,
                PageMenuKind::Back,
                PageMenuKind::Forward,
                PageMenuKind::Reload,
            ]
        );
    }

    #[test]
    fn page_menu_editable_without_selection() {
        let rows = page_menu_kinds(&PageContext {
            editable: true,
            ..ctx()
        });
        assert!(rows.contains(&PageMenuKind::Paste));
        assert!(!rows.contains(&PageMenuKind::Cut));
    }

    #[test]
    fn history_back_is_closest_first() {
        let entries = vec![
            entry(0, "a"),
            entry(1, "b"),
            entry(2, "here"),
            entry(3, "d"),
        ];
        let back = history_jump_items(&entries, 2, false, 12);
        assert_eq!(back, vec![(1, "b".into()), (0, "a".into())]);
        let fwd = history_jump_items(&entries, 2, true, 12);
        assert_eq!(fwd, vec![(3, "d".into())]);
    }

    #[test]
    fn merge_keeps_prior_when_cef_has_only_current() {
        let prior = vec![entry(0, "a"), entry(1, "b"), entry(2, "here")];
        let (out, idx) = merge_tab_history(&prior, 2, &[entry(0, "here")], "https://ex/2", "here");
        assert_eq!(out.len(), 3);
        assert_eq!(idx, 2);
        assert_eq!(out[0].url, "https://ex/0");
    }

    #[test]
    fn merge_appends_new_url_after_restore() {
        let prior = vec![entry(0, "a"), entry(1, "b")];
        let (out, idx) = merge_tab_history(&prior, 1, &[], "https://new/", "New");
        assert_eq!(out.len(), 3);
        assert_eq!(out[2].url, "https://new/");
        assert_eq!(idx, 2);
    }

    #[test]
    fn merge_prefers_live_stack_when_it_covers() {
        let prior = vec![entry(0, "a"), entry(1, "b")];
        let live = vec![entry(0, "a"), entry(1, "b"), entry(2, "c")];
        let (out, idx) = merge_tab_history(&prior, 1, &live, "https://ex/2", "c");
        assert_eq!(out.len(), 3);
        assert_eq!(idx, 2);
    }
}
