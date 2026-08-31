//! Persistent browser session — open tabs survive process restarts.
//!
//! D8: stored as `profiles/<uuid>/session.json` under the active profile's
//! data dir (tabs are the profile workspace). Not under XDG config.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// One tab to restore. Title is best-effort (helps the strip before load).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SessionTab {
    pub url: String,
    #[serde(default)]
    pub title: String,
    /// Chrome group id; absent = loose.
    #[serde(default)]
    pub group_id: Option<String>,
    /// Session history (back/forward hold menu). Survives chrome restart.
    #[serde(default)]
    pub history: Vec<SessionHistory>,
    #[serde(default)]
    pub history_index: i32,
}

/// One back/forward entry persisted with the tab.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SessionHistory {
    pub url: String,
    #[serde(default)]
    pub title: String,
}

/// Named folder persisted beside the tab list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SessionGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub collapsed: bool,
    /// Pocket fill (`#rrggbb` / `#rrggbbaa`). Absent = kit default well.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// A tab closed this session, restored LIFO by ⌘⇧T.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ClosedTab {
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub group_id: Option<String>,
    /// Strip index at close; restore inserts here (clamped).
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub history: Vec<SessionHistory>,
    #[serde(default)]
    pub history_index: i32,
}

/// Cap for [`BrowserSession::closed`] (Chrome-like). Oldest drop first.
pub const CLOSED_TAB_CAP: usize = 25;

/// Full session snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSession {
    /// Tabs in sidebar order.
    pub tabs: Vec<SessionTab>,
    /// Index into `tabs` for the active tab.
    #[serde(default)]
    pub active_index: usize,
    /// Sidebar width (logical px).
    #[serde(default = "default_sidebar_w")]
    pub sidebar_w: f32,
    /// Group metadata (name / collapsed). Membership is on each tab.
    #[serde(default)]
    pub groups: Vec<SessionGroup>,
    /// Most recently closed last. Survives chrome restart / profile switch.
    #[serde(default)]
    pub closed: Vec<ClosedTab>,
}

fn default_sidebar_w() -> f32 {
    crate::app::SIDEBAR_W_DEFAULT
}

impl Default for BrowserSession {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active_index: 0,
            sidebar_w: default_sidebar_w(),
            groups: Vec::new(),
            closed: Vec::new(),
        }
    }
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
            Ok(s) => {
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
    ) -> (Vec<SessionTab>, usize, f32) {
        let sidebar_w = self
            .sidebar_w
            .clamp(crate::app::SIDEBAR_W_MIN, crate::app::SIDEBAR_W_MAX);

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

        (self.tabs, self.active_index, sidebar_w)
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
    sidebar_w: f32,
    groups: &crate::groups::Groups,
    closed: &[ClosedTab],
) -> BrowserSession {
    let session_tabs: Vec<SessionTab> = tabs
        .iter()
        .map(|t| SessionTab {
            url: if t.url.is_empty() {
                crate::app::BLANK_URL.to_string()
            } else {
                t.url.clone()
            },
            title: t.title.clone(),
            group_id: groups.of_tab(t.id).map(str::to_string),
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
    BrowserSession {
        tabs: session_tabs,
        active_index,
        sidebar_w: sidebar_w.clamp(crate::app::SIDEBAR_W_MIN, crate::app::SIDEBAR_W_MAX),
        groups: groups.to_session(),
        closed: closed.to_vec(),
    }
}

/// Fingerprint for dirty detection (avoid rewriting identical JSON every tick).
pub fn fingerprint(session: &BrowserSession) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "a{}|w{:.0}|",
        session.active_index, session.sidebar_w
    ));
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
        s.push('\x1e');
        if let Some(g) = &t.group_id {
            s.push_str(g);
        }
        s.push('\x1f');
    }
    for g in &session.groups {
        s.push_str(&g.id);
        s.push('\x1e');
        s.push_str(&g.name);
        s.push('\x1e');
        s.push(if g.collapsed { '1' } else { '0' });
        s.push('\x1e');
        if let Some(c) = &g.color {
            s.push_str(c);
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
    fn bootstrap_empty_uses_default() {
        let (tabs, active, _) = BrowserSession::default().bootstrap(None, "about:blank");
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
            sidebar_w: 200.0,
            groups: Vec::new(),
            closed: Vec::new(),
        };
        let (tabs, active, _) =
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
            sidebar_w: 200.0,
            groups: Vec::new(),
            closed: Vec::new(),
        };
        let (tabs, active, _) =
            session.bootstrap(Some("--password-store=basic".into()), "about:blank");
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].url, "https://ok.example/");
        assert_eq!(active, 0);
        assert!(!is_cli_open_url("--password-store=basic"));
        assert!(is_cli_open_url("https://example.com"));
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
            200.0,
            &crate::groups::Groups::default(),
            &[],
        );
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
            sidebar_w: 200.0,
            groups: Vec::new(),
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
            sidebar_w: 200.0,
            groups: Vec::new(),
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
    fn fingerprint_changes_with_group_color() {
        let a = BrowserSession {
            tabs: vec![SessionTab {
                url: "https://a/".into(),
                group_id: Some("work".into()),
                ..SessionTab::default()
            }],
            active_index: 0,
            sidebar_w: 200.0,
            groups: vec![SessionGroup {
                id: "work".into(),
                name: "Work".into(),
                collapsed: false,
                color: None,
            }],
            closed: Vec::new(),
        };
        let mut b = a.clone();
        b.groups[0].color = Some("#3dd6f5".into());
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }
}
