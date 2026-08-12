//! Shared tab / profile-workspace cache policy.
//!
//! Active-profile tabs stay in the live strip (already hidden when not
//! painting). Switching profiles **parks** the whole workspace (CEF browsers
//! + chrome snapshot) so returning does not reload pages.
//!
//! Eviction is one policy for everything parked — not a special case per
//! feature. Active workspace is never auto-evicted.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::engine::{TabId, TabInfo};

/// Max profiles kept parked (not counting the one currently live).
pub const MAX_PARKED_PROFILES: usize = 4;

/// Max CEF tabs alive process-wide (live strip + all parks).
pub const MAX_TOTAL_TABS: usize = 48;

/// Parked workspace idle time before it becomes eligible for eviction.
pub const PARK_IDLE: Duration = Duration::from_secs(30 * 60);

/// Chrome-side snapshot of a profile workspace (tab strip + focus).
#[derive(Debug, Clone)]
pub struct WorkspaceSnapshot {
    pub tabs: Vec<TabInfo>,
    pub active: TabId,
    pub sidebar_w: f32,
    pub last_used: Instant,
}

impl WorkspaceSnapshot {
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }
}

/// Decide which parked profile ids to drop given current parks + live tab count.
///
/// Order of pressure relief:
/// 1. Parks idle longer than [`PARK_IDLE`] (oldest idle first)
/// 2. If still over [`MAX_PARKED_PROFILES`], drop LRU parks
/// 3. If still over [`MAX_TOTAL_TABS`], drop LRU parks until under budget
pub fn eviction_victims(
    parked: &HashMap<String, WorkspaceSnapshot>,
    live_tab_count: usize,
    now: Instant,
) -> Vec<String> {
    let mut victims = Vec::new();
    let mut remaining: Vec<(String, Instant, usize)> = parked
        .iter()
        .map(|(id, s)| (id.clone(), s.last_used, s.tab_count()))
        .collect();
    // LRU first (oldest last_used first).
    remaining.sort_by_key(|(_, t, _)| *t);

    let mut parked_tabs: usize = remaining.iter().map(|(_, _, n)| *n).sum();

    // 1) Idle expiry
    let mut i = 0;
    while i < remaining.len() {
        let idle = now.saturating_duration_since(remaining[i].1);
        if idle >= PARK_IDLE {
            let (id, _, n) = remaining.remove(i);
            parked_tabs = parked_tabs.saturating_sub(n);
            victims.push(id);
        } else {
            i += 1;
        }
    }

    // 2) Too many parks
    while remaining.len() > MAX_PARKED_PROFILES {
        let (id, _, n) = remaining.remove(0);
        parked_tabs = parked_tabs.saturating_sub(n);
        victims.push(id);
    }

    // 3) Tab budget
    while live_tab_count + parked_tabs > MAX_TOTAL_TABS && !remaining.is_empty() {
        let (id, _, n) = remaining.remove(0);
        parked_tabs = parked_tabs.saturating_sub(n);
        victims.push(id);
    }

    victims
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(tabs: usize, last_used: Instant) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            tabs: (0..tabs)
                .map(|i| TabInfo {
                    id: TabId(i as u64),
                    url: format!("https://x/{i}"),
                    title: String::new(),
                    is_loading: false,
                    can_go_back: false,
                    can_go_forward: false,
                })
                .collect(),
            active: TabId(0),
            sidebar_w: 200.0,
            last_used,
        }
    }

    #[test]
    fn drops_idle_parks() {
        let now = Instant::now();
        let mut m = HashMap::new();
        m.insert("old".into(), snap(2, now - PARK_IDLE - Duration::from_secs(1)));
        m.insert("fresh".into(), snap(2, now));
        let v = eviction_victims(&m, 2, now);
        assert_eq!(v, vec!["old".to_string()]);
    }

    #[test]
    fn drops_lru_when_too_many_parks() {
        let now = Instant::now();
        let mut m = HashMap::new();
        for i in 0..6 {
            m.insert(
                format!("p{i}"),
                snap(
                    1,
                    now - Duration::from_secs(100 - i as u64), // p0 oldest
                ),
            );
        }
        let v = eviction_victims(&m, 1, now);
        // 6 parks → need to drop to MAX_PARKED_PROFILES (4) → 2 victims, LRU first
        assert!(v.len() >= 2);
        assert!(v.contains(&"p0".to_string()));
    }

    #[test]
    fn drops_for_tab_budget() {
        let now = Instant::now();
        let mut m = HashMap::new();
        // live 40 + park 20 = 60 > 48
        m.insert("big".into(), snap(20, now - Duration::from_secs(5)));
        let v = eviction_victims(&m, 40, now);
        assert_eq!(v, vec!["big".to_string()]);
    }
}
