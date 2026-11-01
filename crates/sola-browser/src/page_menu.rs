//! Pure builders for the page's right-click menu — the desk menu the
//! shell draws (`docs/specs/2026-10-31-sola-vault-design.md`) — and the
//! back/forward history list.
//!
//! The page supplies its own rows (link, picture, navigation, the
//! inspector), which Edit rows apply, and what was clicked; the shell adds
//! Edit and the vault's rows and sends back the pick.

use sola_bus::topics::{DeskEdit, DeskMenuContext, DeskMenuItem};
use sola_core::{KeyChord, KeyCode};

use crate::engine::{HistoryEntry, PageContext};
use crate::util;

/// The ids the page's rows go out with and come back by.
pub mod ids {
    pub const BACKGROUND: &str = "page.background";
    pub const OPEN_LINK: &str = "page.open-link";
    pub const COPY_LINK: &str = "page.copy-link";
    pub const BACK: &str = "page.back";
    pub const FORWARD: &str = "page.forward";
    pub const RELOAD: &str = "page.reload";
    pub const INSPECT: &str = "page.inspect";
    pub const DEVTOOLS: &str = "page.devtools";
    pub const WRAP: &str = "page.wrap";
}

/// One of the page's own rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageMenuKind {
    /// A picture: make it the desktop's background.
    UseAsBackground,
    OpenLink,
    CopyLink,
    Back,
    Forward,
    Reload,
    InspectElement,
    DevTools,
    /// The page becomes its own app.
    Wrap,
    Separator,
}

impl PageMenuKind {
    /// The row's id and label (`None` for a separator).
    pub fn id_label(self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            Self::UseAsBackground => (ids::BACKGROUND, "Use as Desktop Background"),
            Self::OpenLink => (ids::OPEN_LINK, "Open Link in New Window"),
            Self::CopyLink => (ids::COPY_LINK, "Copy Link"),
            Self::Back => (ids::BACK, "Back"),
            Self::Forward => (ids::FORWARD, "Forward"),
            Self::Reload => (ids::RELOAD, "Reload"),
            Self::InspectElement => (ids::INSPECT, "Inspect Element"),
            Self::DevTools => (ids::DEVTOOLS, "Developer Tools"),
            Self::Wrap => (ids::WRAP, "Wrap This Page…"),
            Self::Separator => return None,
        })
    }

    /// The row a pick names.
    pub fn from_id(id: &str) -> Option<Self> {
        [
            Self::UseAsBackground,
            Self::OpenLink,
            Self::CopyLink,
            Self::Back,
            Self::Forward,
            Self::Reload,
            Self::InspectElement,
            Self::DevTools,
            Self::Wrap,
        ]
        .into_iter()
        .find(|k| k.id_label().is_some_and(|(i, _)| i == id))
    }

    /// The key the browser's own menu gives the same action.
    fn shortcut(self) -> Option<KeyChord> {
        match self {
            Self::Back => Some(KeyCode::LEFT.meta()),
            Self::Forward => Some(KeyCode::RIGHT.meta()),
            Self::Reload => Some(KeyCode::R.meta()),
            Self::DevTools => Some(KeyCode::I.meta().alt()),
            _ => None,
        }
    }
}

/// The window the page is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuWindow {
    /// A browser window: it can wrap a page as a web app.
    Browser { wrappable: bool },
    /// A web app's window: it draws no developer tools, so the menu has
    /// no inspector.
    WebApp,
}

/// The page's rows: picture, link, then navigation and the inspector.
/// Cut, Copy and Paste are the shell's Edit rows ([`desk_edit`]).
pub fn page_menu_kinds(ctx: &PageContext, window: MenuWindow) -> Vec<PageMenuKind> {
    let mut rows = Vec::new();
    if ctx.image && ctx.src_url.as_deref().is_some_and(|u| !u.is_empty()) {
        rows.push(PageMenuKind::UseAsBackground);
    }
    if ctx.link_url.as_deref().is_some_and(|u| !u.is_empty()) {
        rows.push(PageMenuKind::OpenLink);
        rows.push(PageMenuKind::CopyLink);
    }
    if !rows.is_empty() {
        rows.push(PageMenuKind::Separator);
    }
    rows.push(PageMenuKind::Back);
    rows.push(PageMenuKind::Forward);
    rows.push(PageMenuKind::Reload);
    if let MenuWindow::Browser { wrappable } = window {
        rows.push(PageMenuKind::Separator);
        rows.push(PageMenuKind::InspectElement);
        rows.push(PageMenuKind::DevTools);
        if wrappable {
            rows.push(PageMenuKind::Separator);
            rows.push(PageMenuKind::Wrap);
        }
    }
    rows
}

/// The page's rows as the desk menu carries them.
pub fn desk_items(ctx: &PageContext, window: MenuWindow) -> Vec<DeskMenuItem> {
    page_menu_kinds(ctx, window)
        .into_iter()
        .map(|kind| match kind.id_label() {
            None => DeskMenuItem::Separator,
            Some((id, label)) => DeskMenuItem::Action {
                id: id.into(),
                label: label.into(),
                shortcut: kind.shortcut(),
                enabled: match kind {
                    PageMenuKind::Back => ctx.can_go_back,
                    PageMenuKind::Forward => ctx.can_go_forward,
                    _ => true,
                },
            },
        })
        .collect()
}

fn has_selection(ctx: &PageContext) -> bool {
    ctx.selection.as_deref().is_some_and(|s| !s.is_empty())
}

/// The Edit rows that apply where the click landed: Cut only with a
/// selection in a field, Paste and Select All only in a field, and no
/// Cut or Copy out of a field Chromium will not copy from (a password
/// field).
pub fn desk_edit(ctx: &PageContext) -> Vec<DeskEdit> {
    let sel = has_selection(ctx) && (!ctx.editable || ctx.can_copy);
    let mut edit = Vec::new();
    if ctx.editable && sel {
        edit.push(DeskEdit::Cut);
    }
    if sel {
        edit.push(DeskEdit::Copy);
    }
    if ctx.editable {
        edit.push(DeskEdit::Paste);
        edit.push(DeskEdit::SelectAll);
    }
    edit
}

/// What was clicked, for the shell (never the text): `site` is the
/// page's address, for the vault's logins. Never `secret`: the vault's
/// rows are the same in every field.
pub fn desk_context(ctx: &PageContext, site: Option<String>) -> DeskMenuContext {
    DeskMenuContext {
        editable: ctx.editable,
        secret: false,
        selection: has_selection(ctx),
        site,
    }
}

/// A page's host, for a log line (never its path or query).
pub fn menu_host(url: &str) -> &str {
    let rest = url.split_once("://").map_or("", |(_, r)| r);
    let auth = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    auth.rsplit_once('@').map_or(auth, |(_, h)| h)
}

/// The page's address as the desk menu may carry it: scheme, host and
/// path — no query or fragment (they hold one-time codes); `None` for a
/// page with no web address.
pub fn menu_site(url: &str) -> Option<String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return None;
    }
    let end = url.find(['?', '#']).unwrap_or(url.len());
    Some(url[..end].to_string())
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

    fn kinds(rows: &[DeskMenuItem]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                DeskMenuItem::Action { id, .. } => id.clone(),
                DeskMenuItem::Separator => "-".into(),
                DeskMenuItem::Submenu { label, .. } => label.clone(),
            })
            .collect()
    }

    const PLAIN: MenuWindow = MenuWindow::Browser { wrappable: false };
    const WRAPPABLE: MenuWindow = MenuWindow::Browser { wrappable: true };

    #[test]
    fn a_web_app_has_no_inspector() {
        let c = PageContext {
            link_url: Some("https://ex/a".into()),
            ..ctx()
        };
        assert_eq!(
            kinds(&desk_items(&c, MenuWindow::WebApp)),
            [ids::OPEN_LINK, ids::COPY_LINK, "-", ids::BACK, ids::FORWARD, ids::RELOAD]
        );
    }

    #[test]
    fn a_plain_page_has_navigation_and_the_inspector() {
        let rows = desk_items(&ctx(), PLAIN);
        assert_eq!(
            kinds(&rows),
            [ids::BACK, ids::FORWARD, ids::RELOAD, "-", ids::INSPECT, ids::DEVTOOLS]
        );
        // No history: Back and Forward are there, greyed.
        assert!(matches!(&rows[0], DeskMenuItem::Action { enabled: false, .. }));
        assert!(desk_edit(&ctx()).is_empty());
    }

    #[test]
    fn a_link_with_a_selection() {
        let c = PageContext {
            link_url: Some("https://ex/a".into()),
            selection: Some("hi".into()),
            can_go_back: true,
            ..ctx()
        };
        let rows = desk_items(&c, WRAPPABLE);
        assert_eq!(
            kinds(&rows),
            [
                ids::OPEN_LINK,
                ids::COPY_LINK,
                "-",
                ids::BACK,
                ids::FORWARD,
                ids::RELOAD,
                "-",
                ids::INSPECT,
                ids::DEVTOOLS,
                "-",
                ids::WRAP
            ]
        );
        assert!(matches!(&rows[3], DeskMenuItem::Action { enabled: true, .. }));
        // Copy only: not in a field.
        assert_eq!(desk_edit(&c), [DeskEdit::Copy]);
    }

    #[test]
    fn a_picture_offers_the_desktop_background_first() {
        let rows = page_menu_kinds(
            &PageContext {
                image: true,
                src_url: Some("https://ex/a.jpg".into()),
                link_url: Some("https://ex/big.jpg".into()),
                ..ctx()
            },
            PLAIN,
        );
        assert_eq!(&rows[..2], &[PageMenuKind::UseAsBackground, PageMenuKind::OpenLink]);
        // No source (a canvas, a blob it cannot name): no row.
        let rows = page_menu_kinds(&PageContext { image: true, ..ctx() }, PLAIN);
        assert!(!rows.contains(&PageMenuKind::UseAsBackground));
    }

    #[test]
    fn edit_rows_follow_the_field() {
        let field = PageContext {
            editable: true,
            ..ctx()
        };
        assert_eq!(desk_edit(&field), [DeskEdit::Paste, DeskEdit::SelectAll]);
        let selected = PageContext {
            selection: Some("x".into()),
            can_copy: true,
            ..field.clone()
        };
        assert_eq!(
            desk_edit(&selected),
            [DeskEdit::Cut, DeskEdit::Copy, DeskEdit::Paste, DeskEdit::SelectAll]
        );
        let c = desk_context(&selected, Some("https://a.com/login".into()));
        assert!(c.editable && !c.secret && c.selection);
        // A password field: Chromium will not copy out of it.
        let password = PageContext {
            can_copy: false,
            ..selected.clone()
        };
        assert_eq!(desk_edit(&password), [DeskEdit::Paste, DeskEdit::SelectAll]);
        // Page text: Copy.
        let text = PageContext {
            selection: Some("x".into()),
            ..ctx()
        };
        assert_eq!(desk_edit(&text), [DeskEdit::Copy]);
    }

    #[test]
    fn rows_come_back_by_id() {
        for k in page_menu_kinds(&PageContext { image: true, src_url: Some("x".into()), link_url: Some("y".into()), ..ctx() }, WRAPPABLE) {
            if let Some((id, _)) = k.id_label() {
                assert_eq!(PageMenuKind::from_id(id), Some(k));
            }
        }
        assert_eq!(PageMenuKind::from_id("nope"), None);
    }

    #[test]
    fn the_site_leaves_its_query_behind() {
        assert_eq!(
            menu_site("https://a.com/cb?code=SECRET#x").as_deref(),
            Some("https://a.com/cb")
        );
        assert_eq!(menu_site("about:blank"), None);
        assert_eq!(menu_host("https://u:p@a.com:8443/x?y"), "a.com:8443");
        assert_eq!(menu_host("about:blank"), "");
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
