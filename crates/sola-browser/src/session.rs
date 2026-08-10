//! Persistent browser session — open tabs survive process restarts.
//!
//! Stored as pretty JSON under `~/.config/sola/browser-session.json` via
//! [`sola_core::config::JsonConfig`]. Tabs are durable product state (D4),
//! not ephemeral chrome.

use sola_core::config::JsonConfig;
use serde::{Deserialize, Serialize};

/// One tab to restore. Title is best-effort (helps the strip before load).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTab {
    pub url: String,
    #[serde(default)]
    pub title: String,
}

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
        }
    }
}

impl JsonConfig for BrowserSession {
    const FILE_NAME: &'static str = "browser-session.json";
}

impl BrowserSession {
    /// Build the tab list + active index for a cold start.
    ///
    /// - Non-empty session → restore those tabs.
    /// - Optional CLI URL → open as a **new** tab and focus it (session kept).
    /// - Empty session and no CLI → one default tab.
    pub fn bootstrap(
        mut self,
        argv_url: Option<String>,
        default_url: &str,
    ) -> (Vec<SessionTab>, usize, f32) {
        let sidebar_w = self
            .sidebar_w
            .clamp(crate::app::SIDEBAR_W_MIN, crate::app::SIDEBAR_W_MAX);

        self.tabs.retain(|t| !t.url.trim().is_empty());

        if let Some(raw) = argv_url {
            let url = crate::util::normalize_url(&raw);
            if !url.is_empty() {
                self.tabs.push(SessionTab {
                    url,
                    title: String::new(),
                });
                self.active_index = self.tabs.len().saturating_sub(1);
            }
        }

        if self.tabs.is_empty() {
            self.tabs.push(SessionTab {
                url: default_url.to_string(),
                title: String::new(),
            });
            self.active_index = 0;
        }

        if self.active_index >= self.tabs.len() {
            self.active_index = self.tabs.len() - 1;
        }

        (self.tabs, self.active_index, sidebar_w)
    }
}

/// Build a session from chrome's cached tab list.
pub fn session_from_tabs(
    tabs: &[crate::engine::TabInfo],
    active: crate::engine::TabId,
    sidebar_w: f32,
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
        s.push('\x1f');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_empty_uses_default() {
        let (tabs, active, _) =
            BrowserSession::default().bootstrap(None, "https://www.wikipedia.org");
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].url, "https://www.wikipedia.org");
        assert_eq!(active, 0);
    }

    #[test]
    fn bootstrap_restores_and_appends_argv() {
        let session = BrowserSession {
            tabs: vec![
                SessionTab {
                    url: "https://a.example/".into(),
                    title: "A".into(),
                },
                SessionTab {
                    url: "https://b.example/".into(),
                    title: "B".into(),
                },
            ],
            active_index: 1,
            sidebar_w: 200.0,
        };
        let (tabs, active, _) =
            session.bootstrap(Some("https://c.example/".into()), "https://fallback/");
        assert_eq!(tabs.len(), 3);
        assert_eq!(tabs[2].url, "https://c.example/");
        assert_eq!(active, 2);
    }

    #[test]
    fn fingerprint_changes_with_url() {
        let a = BrowserSession {
            tabs: vec![SessionTab {
                url: "https://a/".into(),
                title: String::new(),
            }],
            active_index: 0,
            sidebar_w: 200.0,
        };
        let mut b = a.clone();
        b.tabs[0].url = "https://b/".into();
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }
}
