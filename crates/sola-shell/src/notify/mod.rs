//! System notification overlay — live desk cards + missed pile.
//!
//! The iced window parks at 2×2 like other shell overlays. Show Frames a
//! tight top-right rect (not the full usable area) so clicks beside the
//! stack reach apps.

pub mod view;

use std::collections::HashSet;
use std::time::{Duration, Instant};

use iced::window;
use sola_bus::topics::AppNotification;
use sola_kit::app::window_settings;

/// Resting card height (padding + three type rows). Stack Frame uses this.
pub const CARD_H: i32 = 76;
pub const CARD_GAP: i32 = 8;
pub const MAX_LIVE: usize = 3;
/// Compact empty-state overlay (title + one line + pad).
pub const PILE_MIN_HEIGHT: f32 = 96.0;
const PILE_CHROME_H: f32 = 48.0;
const GROUP_ROW_H: f32 = 56.0;
const ITEM_ROW_H: f32 = 40.0;
const REMAINDER_ROW_H: f32 = 28.0;
/// Newest items shown when a group is expanded; the rest stay counted.
pub const EXPAND_SHOW: usize = 30;
/// Groups this small list every message (bell count matches rows).
/// Bigger groups collapse to one app row.
pub const COLLAPSE_AFTER: usize = 4;
pub const HOLD: Duration = Duration::from_secs(6);
pub const ENTER: Duration = Duration::from_millis(180);
pub const LEAVE: Duration = Duration::from_millis(140);
pub const TICK: Duration = Duration::from_millis(16);

/// One live banner.
#[derive(Debug, Clone)]
pub struct Banner {
    pub n: AppNotification,
    pub generation: u64,
    pub entered_at: Instant,
    pub leaving: bool,
    pub leave_at: Option<Instant>,
}

/// One app's missed items, newest first. Built from [`NotifyState::pile`].
#[derive(Debug, Clone)]
pub struct PileGroup<'a> {
    pub app_id: &'a str,
    pub items: Vec<&'a AppNotification>,
}

#[derive(Debug, Clone)]
pub struct NotifyState {
    pub live: Vec<Banner>,
    pub pile: Vec<AppNotification>,
    /// Accent on the menubar bell until the pile is opened (or emptied).
    pub unseen: bool,
    /// App ids whose pile group is expanded in the panel.
    expanded: HashSet<String>,
    /// Notification ids that no longer count toward Super+Tab marks.
    /// Pile history stays until × / activate; the badge is unseen attention.
    badge_seen: HashSet<String>,
    generation: u64,
}

impl Default for NotifyState {
    fn default() -> Self {
        Self {
            live: Vec::new(),
            pile: Vec::new(),
            unseen: false,
            expanded: HashSet::new(),
            badge_seen: HashSet::new(),
            generation: 0,
        }
    }
}

impl NotifyState {
    pub fn visible(&self) -> bool {
        !self.live.is_empty()
    }

    pub fn pile_count(&self) -> u32 {
        self.pile.len() as u32
    }

    /// Clicking the bell: icon returns to normal chrome (pile may still be there).
    /// Also marks every current item seen for Super+Tab — looking at the
    /// list is enough; visiting the app is the other path.
    pub fn acknowledge(&mut self) {
        self.unseen = false;
        self.ack_all();
    }

    fn badge_unseen(&self, id: &str) -> bool {
        !self.badge_seen.contains(id)
    }

    /// Pending Super+Tab attention: unseen live + pile for `app_id`.
    pub fn attention_count(&self, app_id: &str) -> u32 {
        let live = self
            .live
            .iter()
            .filter(|b| b.n.app_id.eq_ignore_ascii_case(app_id) && self.badge_unseen(&b.n.id))
            .count();
        let piled = self
            .pile
            .iter()
            .filter(|n| n.app_id.eq_ignore_ascii_case(app_id) && self.badge_unseen(&n.id))
            .count();
        (live + piled) as u32
    }

    /// Looking at `app_id` (raise / Super+Tab land) clears its mark.
    pub fn ack_app(&mut self, app_id: &str) {
        for b in &self.live {
            if b.n.app_id.eq_ignore_ascii_case(app_id) {
                self.badge_seen.insert(b.n.id.clone());
            }
        }
        for n in &self.pile {
            if n.app_id.eq_ignore_ascii_case(app_id) {
                self.badge_seen.insert(n.id.clone());
            }
        }
    }

    pub fn ack_all(&mut self) {
        for b in &self.live {
            self.badge_seen.insert(b.n.id.clone());
        }
        for n in &self.pile {
            self.badge_seen.insert(n.id.clone());
        }
    }

    fn prune_badge_seen(&mut self) {
        self.badge_seen.retain(|id| {
            self.live.iter().any(|b| b.n.id == *id) || self.pile.iter().any(|n| n.id == *id)
        });
    }

    /// Apps in recency order (newest item in the group first).
    pub fn groups(&self) -> Vec<PileGroup<'_>> {
        let mut groups: Vec<PileGroup<'_>> = Vec::new();
        for n in &self.pile {
            if let Some(g) = groups
                .iter_mut()
                .find(|g| g.app_id.eq_ignore_ascii_case(&n.app_id))
            {
                g.items.push(n);
            } else {
                groups.push(PileGroup {
                    app_id: n.app_id.as_str(),
                    items: vec![n],
                });
            }
        }
        groups
    }

    pub fn group_len(&self, app_id: &str) -> usize {
        self.pile
            .iter()
            .filter(|p| p.app_id.eq_ignore_ascii_case(app_id))
            .count()
    }

    pub fn group_collapsible(&self, app_id: &str) -> bool {
        self.group_len(app_id) > COLLAPSE_AFTER
    }

    pub fn group_expanded(&self, app_id: &str) -> bool {
        self.expanded
            .iter()
            .any(|id| id.eq_ignore_ascii_case(app_id))
    }

    /// Expand / collapse a noisy app group. Small groups always list.
    pub fn toggle_group(&mut self, app_id: &str) {
        if !self.group_collapsible(app_id) {
            return;
        }
        if self.group_expanded(app_id) {
            self.expanded.retain(|id| !id.eq_ignore_ascii_case(app_id));
        } else {
            self.expanded.insert(app_id.to_string());
        }
    }

    /// Drop every missed item for `app_id` without raising. Live cards stay.
    pub fn dismiss_app(&mut self, app_id: &str) -> usize {
        let before = self.pile.len();
        self.pile.retain(|n| !n.app_id.eq_ignore_ascii_case(app_id));
        self.prune_expanded();
        self.prune_badge_seen();
        if self.pile.is_empty() {
            self.unseen = false;
        }
        before - self.pile.len()
    }

    fn prune_expanded(&mut self) {
        self.expanded.retain(|id| {
            self.pile
                .iter()
                .filter(|n| n.app_id.eq_ignore_ascii_case(id))
                .count()
                > 1
        });
    }

    /// Overlay height from *visible* rows (collapsed groups), not item count.
    pub fn overlay_height(&self, usable_h: f32) -> f32 {
        let wanted = if self.pile.is_empty() {
            PILE_MIN_HEIGHT
        } else {
            PILE_CHROME_H + self.visible_stack_h()
        };
        let cap = usable_h.max(PILE_MIN_HEIGHT);
        wanted.clamp(PILE_MIN_HEIGHT, cap)
    }

    fn visible_stack_h(&self) -> f32 {
        let mut h = 0.0;
        for (i, g) in self.groups().iter().enumerate() {
            if i > 0 {
                h += 8.0;
            }
            if g.items.len() <= COLLAPSE_AFTER {
                h += g.items.len() as f32 * GROUP_ROW_H;
                continue;
            }
            h += GROUP_ROW_H;
            if self.group_expanded(g.app_id) {
                let show = g.items.len().min(EXPAND_SHOW);
                h += show as f32 * ITEM_ROW_H;
                if g.items.len() > EXPAND_SHOW {
                    h += REMAINDER_ROW_H;
                }
            }
        }
        h
    }

    pub fn stack_height(&self) -> i32 {
        let n = self.live.len().max(1) as i32;
        n * CARD_H + (n - 1) * CARD_GAP
    }

    /// 0 at drop start, 1 at rest. Newest live banner drives the window y.
    pub fn enter_t(&self, now: Instant) -> f32 {
        let Some(b) = self.live.last() else {
            return 1.0;
        };
        if b.leaving {
            return 1.0;
        }
        let dt = now.saturating_duration_since(b.entered_at);
        (dt.as_secs_f32() / ENTER.as_secs_f32()).clamp(0.0, 1.0)
    }

    pub fn needs_tick(&self, now: Instant) -> bool {
        self.live.iter().any(|b| {
            if b.leaving {
                return true;
            }
            now.saturating_duration_since(b.entered_at) < ENTER
        })
    }

    /// Push a notification. Returns the generation to schedule `Expire` with.
    pub fn push(&mut self, mut n: AppNotification, now: Instant) -> u64 {
        if n.id.trim().is_empty() {
            self.generation = self.generation.wrapping_add(1);
            n.id = format!("n-{}", self.generation);
        }
        if let Some(tag) = n.tag.as_deref() {
            if let Some(pos) = self
                .live
                .iter()
                .position(|b| b.n.app_id == n.app_id && b.n.tag.as_deref() == Some(tag))
            {
                let generation = self.live[pos].generation;
                self.live[pos].n = n;
                self.live[pos].entered_at = now;
                self.live[pos].leaving = false;
                self.live[pos].leave_at = None;
                return generation;
            }
        }
        while self.live.len() >= MAX_LIVE {
            let old = self.live.remove(0);
            self.push_pile(old.n);
        }
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.live.push(Banner {
            n,
            generation,
            entered_at: now,
            leaving: false,
            leave_at: None,
        });
        generation
    }

    /// Timeout: start leave, then [`finish_leave`].
    pub fn begin_leave(&mut self, generation: u64, now: Instant) -> bool {
        let Some(b) = self.live.iter_mut().find(|b| b.generation == generation) else {
            return false;
        };
        if b.leaving {
            return false;
        }
        b.leaving = true;
        b.leave_at = Some(now);
        true
    }

    pub fn finish_leave(&mut self, now: Instant) -> bool {
        let mut moved = false;
        let mut keep = Vec::new();
        let mut piled = Vec::new();
        for b in self.live.drain(..) {
            if b.leaving {
                let done = b
                    .leave_at
                    .map(|t| now.saturating_duration_since(t) >= LEAVE)
                    .unwrap_or(true);
                if done {
                    piled.push(b.n);
                    moved = true;
                    continue;
                }
            }
            keep.push(b);
        }
        self.live = keep;
        for n in piled {
            self.push_pile(n);
        }
        moved
    }

    /// Click raise: drop from live and pile, return the payload.
    pub fn take(&mut self, id: &str) -> Option<AppNotification> {
        if let Some(pos) = self.live.iter().position(|b| b.n.id == id) {
            let n = self.live.remove(pos).n;
            self.prune_badge_seen();
            return Some(n);
        }
        if let Some(pos) = self.pile.iter().position(|n| n.id == id) {
            let n = self.pile.remove(pos);
            self.prune_expanded();
            self.prune_badge_seen();
            if self.pile.is_empty() {
                self.unseen = false;
            }
            return Some(n);
        }
        None
    }

    /// × without raising.
    pub fn dismiss(&mut self, id: &str) -> bool {
        if let Some(pos) = self.live.iter().position(|b| b.n.id == id) {
            self.live.remove(pos);
            self.prune_badge_seen();
            return true;
        }
        if let Some(pos) = self.pile.iter().position(|n| n.id == id) {
            self.pile.remove(pos);
            self.prune_expanded();
            self.prune_badge_seen();
            if self.pile.is_empty() {
                self.unseen = false;
            }
            return true;
        }
        false
    }

    fn push_pile(&mut self, n: AppNotification) {
        self.pile.retain(|p| p.id != n.id);
        if let Some(tag) = n.tag.as_deref() {
            self.pile.retain(|p| {
                !(p.app_id.eq_ignore_ascii_case(&n.app_id) && p.tag.as_deref() == Some(tag))
            });
        }
        self.pile.insert(0, n);
        self.unseen = true;
        self.prune_badge_seen();
    }
}

pub fn open_window() -> (window::Id, iced::Task<window::Id>) {
    let mut settings = window_settings("sola-shell");
    let p = crate::zoning::OVERLAY_PARK as f32;
    settings.size = iced::Size::new(p, p);
    settings.position = iced::window::Position::Specific(iced::Point::new(
        crate::zoning::OVERLAY_PARK_X as f32,
        crate::zoning::OVERLAY_PARK_Y as f32,
    ));
    settings.resizable = false;
    settings.decorations = false;
    settings.transparent = true;
    window::open(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(id: &str, tag: Option<&str>) -> AppNotification {
        AppNotification {
            id: id.into(),
            app_id: "sola-workspaces".into(),
            source: "Workspaces".into(),
            title: "done".into(),
            body: String::new(),
            tag: tag.map(|s| s.into()),
            tab_id: None,
            url: None,
        }
    }

    #[test]
    fn expire_goes_to_pile_click_does_not() {
        let mut s = NotifyState::default();
        let t0 = Instant::now();
        let g = s.push(n("a", None), t0);
        assert_eq!(s.live.len(), 1);
        assert_eq!(s.pile_count(), 0);
        assert!(s.begin_leave(g, t0));
        s.finish_leave(t0 + LEAVE);
        assert!(s.live.is_empty());
        assert_eq!(s.pile_count(), 1);
        assert!(s.take("a").is_some());
        assert_eq!(s.pile_count(), 0);
    }

    #[test]
    fn dismiss_skips_pile() {
        let mut s = NotifyState::default();
        let t0 = Instant::now();
        s.push(n("a", None), t0);
        assert!(s.dismiss("a"));
        assert!(s.live.is_empty());
        assert_eq!(s.pile_count(), 0);
    }

    #[test]
    fn tag_replaces_in_flight() {
        let mut s = NotifyState::default();
        let t0 = Instant::now();
        s.push(n("a", Some("job")), t0);
        s.push(
            AppNotification {
                title: "again".into(),
                ..n("b", Some("job"))
            },
            t0,
        );
        assert_eq!(s.live.len(), 1);
        assert_eq!(s.live[0].n.title, "again");
        assert_eq!(s.live[0].n.id, "b");
    }

    #[test]
    fn overflow_live_goes_to_pile() {
        let mut s = NotifyState::default();
        let t0 = Instant::now();
        for i in 0..4 {
            s.push(n(&format!("n{i}"), None), t0);
        }
        assert_eq!(s.live.len(), MAX_LIVE);
        assert_eq!(s.pile_count(), 1);
        assert_eq!(s.pile[0].id, "n0");
        assert!(s.unseen);
    }

    #[test]
    fn opening_pile_clears_bell_accent() {
        let mut s = NotifyState::default();
        let t0 = Instant::now();
        let g = s.push(n("a", None), t0);
        assert!(!s.unseen, "live banner is not the missed-pile chip");
        assert!(s.begin_leave(g, t0));
        s.finish_leave(t0 + LEAVE);
        assert!(s.unseen);
        s.acknowledge();
        assert!(!s.unseen);
        assert_eq!(s.pile_count(), 1);
        let g = s.push(n("b", None), t0);
        assert!(s.begin_leave(g, t0));
        s.finish_leave(t0 + LEAVE);
        assert!(s.unseen);
    }

    fn pile_into(s: &mut NotifyState, note: AppNotification) {
        let t0 = Instant::now();
        let g = s.push(note, t0);
        assert!(s.begin_leave(g, t0));
        s.finish_leave(t0 + LEAVE);
    }

    fn slack(id: &str) -> AppNotification {
        AppNotification {
            app_id: "slack".into(),
            source: "Slack".into(),
            title: id.into(),
            ..n(id, None)
        }
    }

    #[test]
    fn pile_keeps_more_than_twenty() {
        let mut s = NotifyState::default();
        let t0 = Instant::now();
        for i in 0..(MAX_LIVE + 40) {
            s.push(n(&format!("n{i}"), None), t0);
        }
        assert_eq!(s.live.len(), MAX_LIVE);
        assert_eq!(s.pile.len(), 40);
        assert!(s.pile.iter().any(|p| p.id == "n0"));
    }

    #[test]
    fn groups_by_app_newest_first() {
        let mut s = NotifyState::default();
        pile_into(&mut s, slack("a"));
        pile_into(&mut s, slack("b"));
        pile_into(
            &mut s,
            AppNotification {
                app_id: "sola-browser".into(),
                source: "news.ycombinator.com".into(),
                title: "thread".into(),
                ..n("c", None)
            },
        );
        pile_into(&mut s, slack("d"));
        let groups = s.groups();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].app_id, "slack");
        assert_eq!(
            groups[0]
                .items
                .iter()
                .map(|n| n.id.as_str())
                .collect::<Vec<_>>(),
            vec!["d", "b", "a"]
        );
        assert_eq!(groups[1].app_id, "sola-browser");
        assert_eq!(groups[1].items.len(), 1);
    }

    #[test]
    fn overlay_height_follows_groups_not_items() {
        let mut s = NotifyState::default();
        for i in 0..20 {
            pile_into(&mut s, slack(&format!("s{i}")));
        }
        let h = s.overlay_height(2000.0);
        assert!(
            h < 160.0,
            "collapsed flood should be one group row, got {h}"
        );
        s.toggle_group("slack");
        let open = s.overlay_height(2000.0);
        assert!(open > h, "expand should grow the overlay");
        let capped = s.overlay_height(180.0);
        assert_eq!(capped, 180.0);
        assert_eq!(
            NotifyState::default().overlay_height(1000.0),
            PILE_MIN_HEIGHT
        );
    }

    #[test]
    fn attention_counts_live_and_pile() {
        let mut s = NotifyState::default();
        let t0 = Instant::now();
        s.push(slack("live"), t0);
        pile_into(&mut s, slack("missed"));
        assert_eq!(s.attention_count("slack"), 2);
        assert_eq!(s.attention_count("sola-mail"), 0);
    }

    #[test]
    fn badge_drops_on_ack_without_draining_pile() {
        let mut s = NotifyState::default();
        pile_into(&mut s, slack("a"));
        pile_into(&mut s, slack("b"));
        pile_into(&mut s, n("ws", None));
        assert_eq!(s.attention_count("slack"), 2);
        s.ack_app("slack");
        assert_eq!(s.attention_count("slack"), 0);
        assert_eq!(s.pile_count(), 3, "pile is history, not the badge");
        assert_eq!(s.attention_count("sola-workspaces"), 1);
        s.acknowledge();
        assert_eq!(s.attention_count("sola-workspaces"), 0);
        assert_eq!(s.pile_count(), 3);
        pile_into(&mut s, slack("c"));
        assert_eq!(s.attention_count("slack"), 1, "new item badges again");
    }

    #[test]
    fn toggle_group_and_dismiss_app() {
        let mut s = NotifyState::default();
        for i in 0..(COLLAPSE_AFTER + 1) {
            pile_into(&mut s, slack(&format!("s{i}")));
        }
        pile_into(&mut s, n("browser", None));
        assert!(s.group_collapsible("slack"));
        s.toggle_group("slack");
        assert!(s.group_expanded("slack"));
        assert_eq!(s.dismiss_app("slack"), COLLAPSE_AFTER + 1);
        assert!(!s.group_expanded("slack"));
        assert_eq!(s.pile_count(), 1);
    }

    #[test]
    fn pile_tag_replaces_same_app() {
        let mut s = NotifyState::default();
        pile_into(
            &mut s,
            AppNotification {
                title: "first".into(),
                tag: Some("done-p1".into()),
                ..slack("a")
            },
        );
        pile_into(
            &mut s,
            AppNotification {
                title: "again".into(),
                tag: Some("done-p1".into()),
                ..slack("b")
            },
        );
        assert_eq!(s.pile_count(), 1);
        assert_eq!(s.pile[0].title, "again");
        assert_eq!(s.pile[0].id, "b");
    }

    #[test]
    fn small_group_lists_every_row() {
        let mut s = NotifyState::default();
        pile_into(&mut s, slack("a"));
        pile_into(&mut s, slack("b"));
        pile_into(&mut s, slack("c"));
        assert!(!s.group_collapsible("slack"));
        let h = s.overlay_height(2000.0);
        assert!(
            h > 150.0,
            "three listed rows, not one collapsed header: {h}"
        );
    }

    #[test]
    fn empty_id_is_assigned() {
        let mut s = NotifyState::default();
        let g = s.push(
            AppNotification {
                id: String::new(),
                ..n("", None)
            },
            Instant::now(),
        );
        assert!(!s.live[0].n.id.is_empty());
        assert_eq!(s.live[0].generation, g);
    }
}
