//! System notification overlay — live desk cards + missed pile.
//!
//! The iced window parks at 2×2 like other shell overlays. Show Frames a
//! tight top-right rect (not the full usable area) so clicks beside the
//! stack reach apps.

pub mod view;

use std::time::{Duration, Instant};

use iced::window;
use sola_bus::topics::AppNotification;
use sola_kit::app::window_settings;

/// Resting card height (padding + three type rows). Stack Frame uses this.
pub const CARD_H: i32 = 76;
pub const CARD_GAP: i32 = 8;
pub const MAX_LIVE: usize = 3;
/// Missed pile is a short session list; older rows drop off the bottom.
pub const MAX_PILE: usize = 20;
/// Compact empty-state overlay (title + one line + pad).
pub const PILE_MIN_HEIGHT: f32 = 96.0;
const PILE_ROW_H: f32 = 48.0;
const PILE_CHROME_H: f32 = 52.0;
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

#[derive(Debug, Clone)]
pub struct NotifyState {
    pub live: Vec<Banner>,
    pub pile: Vec<AppNotification>,
    /// Accent on the menubar bell until the pile is opened (or emptied).
    pub unseen: bool,
    generation: u64,
}

impl Default for NotifyState {
    fn default() -> Self {
        Self {
            live: Vec::new(),
            pile: Vec::new(),
            unseen: false,
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
    pub fn acknowledge(&mut self) {
        self.unseen = false;
    }

    /// Live overlay height for the missed pile: content-sized, capped at the
    /// usable area under the menubar so a full pile (~20) is not clipped.
    pub fn pile_overlay_height(count: usize, usable_h: f32) -> f32 {
        let wanted = if count == 0 {
            PILE_MIN_HEIGHT
        } else {
            PILE_CHROME_H + count as f32 * PILE_ROW_H
        };
        let cap = usable_h.max(PILE_MIN_HEIGHT);
        wanted.clamp(PILE_MIN_HEIGHT, cap)
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
            return Some(self.live.remove(pos).n);
        }
        if let Some(pos) = self.pile.iter().position(|n| n.id == id) {
            let n = self.pile.remove(pos);
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
            return true;
        }
        if let Some(pos) = self.pile.iter().position(|n| n.id == id) {
            self.pile.remove(pos);
            if self.pile.is_empty() {
                self.unseen = false;
            }
            return true;
        }
        false
    }

    fn push_pile(&mut self, n: AppNotification) {
        self.pile.retain(|p| p.id != n.id);
        self.pile.insert(0, n);
        self.unseen = true;
        while self.pile.len() > MAX_PILE {
            self.pile.pop();
        }
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

    #[test]
    fn pile_caps_at_twenty_drops_oldest() {
        let mut s = NotifyState::default();
        let t0 = Instant::now();
        for i in 0..(MAX_LIVE + MAX_PILE + 1) {
            s.push(n(&format!("n{i}"), None), t0);
        }
        assert_eq!(s.live.len(), MAX_LIVE);
        assert_eq!(s.pile.len(), MAX_PILE);
        assert_eq!(s.pile.last().map(|p| p.id.as_str()), Some("n1"));
        assert!(s.pile.iter().all(|p| p.id != "n0"));
    }

    #[test]
    fn pile_overlay_grows_then_caps() {
        assert_eq!(NotifyState::pile_overlay_height(0, 1000.0), PILE_MIN_HEIGHT);
        let twenty = NotifyState::pile_overlay_height(20, 2000.0);
        assert!(twenty > 400.0);
        assert_eq!(
            NotifyState::pile_overlay_height(20, 300.0),
            300.0_f32.max(PILE_MIN_HEIGHT)
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
