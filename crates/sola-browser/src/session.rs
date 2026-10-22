//! Persistent browser session — open tabs survive process restarts.
//!
//! D8: stored as `profiles/<uuid>/session.json` under the active profile's
//! data dir (tabs are the profile workspace). Not under XDG config.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// One tab to restore. Title is best-effort (names the window before load).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTab {
    /// Stable across restarts: the page's window is the shell tab
    /// `sola-browser/<id>`, and keeps its place by it. Minted for tabs saved
    /// before ids existed.
    #[serde(default = "crate::profiles::new_uuid")]
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub title: String,
    /// Session history (back/forward hold menu). Survives chrome restart.
    #[serde(default)]
    pub history: Vec<SessionHistory>,
    #[serde(default)]
    pub history_index: i32,
}

impl Default for SessionTab {
    fn default() -> Self {
        Self {
            id: crate::profiles::new_uuid(),
            url: String::new(),
            title: String::new(),
            history: Vec::new(),
            history_index: 0,
        }
    }
}

/// One back/forward entry persisted with the tab.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SessionHistory {
    pub url: String,
    #[serde(default)]
    pub title: String,
}

/// A tab closed this session, restored LIFO by ⌘⇧T.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ClosedTab {
    /// The page's id, so a reopened page takes its place back.
    #[serde(default)]
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub title: String,
    /// Index in the page list at close; restore inserts here (clamped).
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub history: Vec<SessionHistory>,
    #[serde(default)]
    pub history_index: i32,
}

/// Cap for [`BrowserSession::closed`] (Chrome-like). Oldest drop first.
pub const CLOSED_TAB_CAP: usize = 25;

/// Full session snapshot. Files from before a window per page also carry
/// tab groups and a sidebar width; those keys are ignored.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BrowserSession {
    /// Pages in open order.
    pub tabs: Vec<SessionTab>,
    /// Index into `tabs` for the active tab.
    #[serde(default)]
    pub active_index: usize,
    /// Most recently closed last. Survives chrome restart / profile switch.
    #[serde(default)]
    pub closed: Vec<ClosedTab>,
}


/// Keep the newest [`CLOSED_TAB_CAP`] entries (append = most recent).
pub fn push_closed(stack: &mut Vec<ClosedTab>, tab: ClosedTab) {
    stack.push(tab);
    let extra = stack.len().saturating_sub(CLOSED_TAB_CAP);
    if extra > 0 {
        stack.drain(0..extra);
    }
}

impl BrowserSession {
    /// Load from the active profile's `session.json` (or default if missing).
    pub fn load() -> Self {
        let path = crate::profiles::active().session_path();
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Self {
        match sola_core::config::load_json_or_default::<Self>(path) {
            Ok(mut s) => {
                // A stored NUL title becomes the xdg toplevel title on the
                // first frame and aborts chrome before any page can replace it.
                scrub_control_chars(&mut s);
                if path.exists() {
                    tracing::info!(path = %path.display(), "restored session");
                }
                s
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "failed to load session");
                Self::default()
            }
        }
    }

    /// Save to the active profile's `session.json`.
    pub fn save(&self) {
        let path = crate::profiles::active().session_path();
        self.save_to(&path);
    }

    pub fn save_to(&self, path: &Path) {
        if let Err(e) = sola_core::config::save_json_pretty(path, self) {
            tracing::warn!(path = %path.display(), error = %e, "failed to write session");
        }
    }

    /// Build the tab list + active index for a cold start.
    ///
    /// - Non-empty session → restore those tabs.
    /// - Optional CLI URL → open as a **new** tab and focus it (session kept).
    /// - Empty session and no CLI → one default tab.
    ///
    /// CLI flags (`--…`) must never become tabs: Chromium/CEF switches
    /// (and any leftover `--password-store=basic` on argv after re-exec)
    /// used to be passed through `normalize_url` → `https://--password-store=…`.
    pub fn bootstrap(
        mut self,
        argv_url: Option<String>,
        default_url: &str,
    ) -> (Vec<SessionTab>, usize) {
        scrub_control_chars(&mut self);
        self.tabs
            .retain(|t| !t.url.trim().is_empty() && !is_spurious_switch_url(&t.url));

        if let Some(raw) = argv_url.filter(|s| is_cli_open_url(s)) {
            let url = crate::util::normalize_url(&raw);
            if !url.is_empty() && !is_spurious_switch_url(&url) {
                self.tabs.push(SessionTab {
                    url,
                    ..SessionTab::default()
                });
                self.active_index = self.tabs.len().saturating_sub(1);
            }
        }

        if self.tabs.is_empty() {
            self.tabs.push(SessionTab {
                url: default_url.to_string(),
                ..SessionTab::default()
            });
            self.active_index = 0;
        }

        if self.active_index >= self.tabs.len() {
            self.active_index = self.tabs.len() - 1;
        }

        (self.tabs, self.active_index)
    }
}

/// True if this argv token is meant as a page to open (not a CLI/CEF switch).
pub fn is_cli_open_url(raw: &str) -> bool {
    let t = raw.trim();
    if t.is_empty() || t.starts_with('-') {
        return false;
    }
    // CEF subprocess / utility args never open as pages.
    if t.starts_with("--type=") {
        return false;
    }
    true
}

/// Tabs accidentally created from CLI switches (`https://--password-store=…`).
fn is_spurious_switch_url(url: &str) -> bool {
    let u = url.trim();
    if u.is_empty() {
        return false;
    }
    // normalize_url prepends https:// to bare `--flag` tokens.
    let rest = u
        .strip_prefix("https://")
        .or_else(|| u.strip_prefix("http://"))
        .unwrap_or(u);
    rest.starts_with("--")
        || rest.starts_with("password-store")
        || rest.contains("password-store=")
        || rest.contains("persist-session-cookies")
}

/// Chrome history list restored from `session.json`.
pub fn history_from_session(tab: &SessionTab) -> (Vec<crate::engine::HistoryEntry>, i32) {
    if tab.history.is_empty() {
        return (Vec::new(), 0);
    }
    let entries: Vec<crate::engine::HistoryEntry> = tab
        .history
        .iter()
        .enumerate()
        .map(|(i, h)| crate::engine::HistoryEntry {
            index: i as i32,
            url: h.url.clone(),
            title: h.title.clone(),
        })
        .collect();
    let max = entries.len().saturating_sub(1) as i32;
    (entries, tab.history_index.clamp(0, max))
}

/// Build a session from chrome's cached tab list.
pub fn session_from_tabs(
    tabs: &[crate::engine::TabInfo],
    active: crate::engine::TabId,
    closed: &[ClosedTab],
    page_ids: &std::collections::HashMap<crate::engine::TabId, String>,
) -> BrowserSession {
    let session_tabs: Vec<SessionTab> = tabs
        .iter()
        .map(|t| SessionTab {
            id: page_ids
                .get(&t.id)
                .cloned()
                .unwrap_or_else(crate::profiles::new_uuid),
            url: if t.url.is_empty() {
                crate::app::BLANK_URL.to_string()
            } else {
                t.url.clone()
            },
            title: t.title.clone(),
            history: t
                .history
                .iter()
                .map(|e| SessionHistory {
                    url: e.url.clone(),
                    title: e.title.clone(),
                })
                .collect(),
            history_index: t.history_index,
        })
        .collect();
    let active_index = tabs
        .iter()
        .position(|t| t.id == active)
        .unwrap_or(0)
        .min(session_tabs.len().saturating_sub(1));
    let mut session = BrowserSession {
        tabs: session_tabs,
        active_index,
        closed: closed.to_vec(),
    };
    scrub_control_chars(&mut session);
    session
}

/// Drop ASCII/Unicode controls (including NUL) from titles that can become
/// the Wayland toplevel title or a restored tab label.
fn scrub_control_chars(session: &mut BrowserSession) {
    for tab in &mut session.tabs {
        tab.title = crate::util::wayland_title_text(&tab.title);
        for entry in &mut tab.history {
            entry.title = crate::util::wayland_title_text(&entry.title);
        }
    }
    for closed in &mut session.closed {
        closed.title = crate::util::wayland_title_text(&closed.title);
        for entry in &mut closed.history {
            entry.title = crate::util::wayland_title_text(&entry.title);
        }
    }
}

/// Fingerprint for dirty detection (avoid rewriting identical JSON every tick).
pub fn fingerprint(session: &BrowserSession) -> String {
    let mut s = String::new();
    s.push_str(&format!("a{}|", session.active_index));
    for t in &session.tabs {
        s.push_str(&t.url);
        s.push('\x1e');
        s.push_str(&t.title);
        s.push('\x1e');
        s.push_str(&format!("h{}@{}", t.history_index, t.history.len()));
        s.push('\x1e');
        for h in &t.history {
            s.push_str(&h.url);
            s.push('\x1d');
        }
        s.push('\x1f');
    }
    for c in &session.closed {
        s.push_str(&c.url);
        s.push('\x1e');
        s.push_str(&format!("{}", c.index));
        s.push('\x1f');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_saved_before_ids_get_one() {
        let s: BrowserSession =
            serde_json::from_str(r#"{"tabs":[{"url":"https://a/"},{"url":"https://b/"}]}"#)
                .unwrap();
        assert_eq!(s.tabs[0].id.len(), 36);
        assert_ne!(s.tabs[0].id, s.tabs[1].id);
    }

    #[test]
    fn bootstrap_strips_nul_from_restored_titles() {
        let session = BrowserSession {
            tabs: vec![SessionTab {
                url: "https://digitalarchives.wa.gov/DigitalObject/Download/abc".into(),
                title: "magick-tg-P98v8XUbFBC6uv9Jd58pTwIuN1fnM\0".into(),
                history: vec![SessionHistory {
                    url: "https://digitalarchives.wa.gov/DigitalObject/Download/abc".into(),
                    title: "magick-tg-P98v8XUbFBC6uv9Jd58pTwIuN1fnM\0".into(),
                }],
                history_index: 0,
                ..SessionTab::default()
            }],
            active_index: 0,
            closed: vec![ClosedTab {
                url: "https://example.com/".into(),
                title: "closed\0".into(),
                ..ClosedTab::default()
            }],
        };
        let (tabs, _) = session.bootstrap(None, "about:blank");
        assert_eq!(tabs[0].title, "magick-tg-P98v8XUbFBC6uv9Jd58pTwIuN1fnM");
        assert_eq!(
            tabs[0].history[0].title,
            "magick-tg-P98v8XUbFBC6uv9Jd58pTwIuN1fnM"
        );
        assert!(!tabs[0].title.contains('\0'));
    }

    #[test]
    fn bootstrap_empty_uses_default() {
        let (tabs, active) = BrowserSession::default().bootstrap(None, "about:blank");
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].url, "about:blank");
        assert_eq!(active, 0);
    }

    #[test]
    fn bootstrap_restores_and_appends_argv() {
        let session = BrowserSession {
            tabs: vec![
                SessionTab {
                    url: "https://a.example/".into(),
                    title: "A".into(),
                    ..SessionTab::default()
                },
                SessionTab {
                    url: "https://b.example/".into(),
                    title: "B".into(),
                    ..SessionTab::default()
                },
            ],
            active_index: 1,
            closed: Vec::new(),
        };
        let (tabs, active) =
            session.bootstrap(Some("https://c.example/".into()), "https://fallback/");
        assert_eq!(tabs.len(), 3);
        assert_eq!(tabs[2].url, "https://c.example/");
        assert_eq!(active, 2);
    }

    #[test]
    fn bootstrap_ignores_cli_switches_and_scrubs_bad_tabs() {
        let session = BrowserSession {
            tabs: vec![
                SessionTab {
                    url: "https://ok.example/".into(),
                    title: "Ok".into(),
                    ..SessionTab::default()
                },
                SessionTab {
                    url: "https://--password-store=basic".into(),
                    title: "junk".into(),
                    ..SessionTab::default()
                },
            ],
            active_index: 1,
            closed: Vec::new(),
        };
        let (tabs, active) =
            session.bootstrap(Some("--password-store=basic".into()), "about:blank");
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].url, "https://ok.example/");
        assert_eq!(active, 0);
        assert!(!is_cli_open_url("--password-store=basic"));
        assert!(is_cli_open_url("https://example.com"));
    }

    #[test]
    fn save_snapshot_strips_nul_titles() {
        let tab = crate::engine::TabInfo::chrome(
            crate::engine::TabId(1),
            "https://example.com/",
            "bad\0title",
        );
        let closed = ClosedTab {
            url: "https://example.com/c".into(),
            title: "closed\0".into(),
            ..ClosedTab::default()
        };
        let session = session_from_tabs(
            &[tab],
            crate::engine::TabId(1),
            &[closed],
            &std::collections::HashMap::new(),
        );
        assert_eq!(session.tabs[0].title, "badtitle");
        assert_eq!(session.closed[0].title, "closed");
        assert!(!session.tabs[0].title.contains('\0'));
    }

    #[test]
    fn session_round_trips_history() {
        let tab = crate::engine::TabInfo {
            history: vec![
                crate::engine::HistoryEntry {
                    index: 0,
                    url: "https://a/".into(),
                    title: "A".into(),
                },
                crate::engine::HistoryEntry {
                    index: 1,
                    url: "https://b/".into(),
                    title: "B".into(),
                },
            ],
            history_index: 1,
            ..crate::engine::TabInfo::chrome(crate::engine::TabId(1), "https://b/", "B")
        };
        let session = session_from_tabs(
            &[tab],
            crate::engine::TabId(1),
            &[],
            &std::collections::HashMap::from([(crate::engine::TabId(1), "page-b".to_string())]),
        );
        assert_eq!(session.tabs[0].id, "page-b");
        assert_eq!(session.tabs[0].history.len(), 2);
        assert_eq!(session.tabs[0].history_index, 1);
        let (entries, idx) = history_from_session(&session.tabs[0]);
        assert_eq!(entries.len(), 2);
        assert_eq!(idx, 1);
        assert_eq!(entries[0].url, "https://a/");
    }

    #[test]
    fn fingerprint_changes_with_url() {
        let a = BrowserSession {
            tabs: vec![SessionTab {
                url: "https://a/".into(),
                ..SessionTab::default()
            }],
            active_index: 0,
            closed: Vec::new(),
        };
        let mut b = a.clone();
        b.tabs[0].url = "https://b/".into();
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn closed_stack_is_lifo_and_capped() {
        let mut stack = Vec::new();
        for i in 0..(CLOSED_TAB_CAP + 3) {
            push_closed(
                &mut stack,
                ClosedTab {
                    url: format!("https://n/{i}"),
                    index: i,
                    ..ClosedTab::default()
                },
            );
        }
        assert_eq!(stack.len(), CLOSED_TAB_CAP);
        assert_eq!(stack[0].url, "https://n/3");
        assert_eq!(
            stack.last().unwrap().url,
            format!("https://n/{}", CLOSED_TAB_CAP + 2)
        );
        let last = stack.pop().unwrap();
        assert_eq!(last.url, format!("https://n/{}", CLOSED_TAB_CAP + 2));
    }

    #[test]
    fn fingerprint_changes_with_closed_stack() {
        let a = BrowserSession {
            tabs: vec![SessionTab {
                url: "https://a/".into(),
                ..SessionTab::default()
            }],
            active_index: 0,
            closed: Vec::new(),
        };
        let mut b = a.clone();
        b.closed.push(ClosedTab {
            url: "https://gone/".into(),
            ..ClosedTab::default()
        });
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn files_with_groups_and_a_sidebar_width_still_load() {
        let s: BrowserSession = serde_json::from_str(
            r#"{"tabs":[{"url":"https://a/","group_id":"g1"}],"sidebar_w":220.0,
                "groups":[{"id":"g1","name":"Work","collapsed":false}],
                "closed":[{"url":"https://b/","group_id":"g1","index":0}]}"#,
        )
        .unwrap();
        assert_eq!(s.tabs[0].url, "https://a/");
        assert_eq!(s.closed[0].url, "https://b/");
    }
}
