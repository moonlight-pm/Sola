//! System notifications: live desk cards, and the apps lit in the top
//! row's notifications island.
//!
//! The iced window parks at 2×2 like other shell overlays. Show Frames a
//! tight top-right rect (not the full usable area) so clicks beside the
//! stack reach apps.
//!
//! An app that notifies while you are not in it is lit: its icon shows in
//! the island until you are in it again (`Shell::apply_focus`).

pub mod view;

use std::time::{Duration, Instant};

use iced::window;
use sola_bus::topics::AppNotification;
use sola_kit::app::window_settings;

/// Resting card height (padding + three type rows). Stack Frame uses this.
pub const CARD_H: i32 = 76;
/// An app asking to be a handler: three type rows and the buttons.
pub const PROMPT_H: i32 = 118;
pub const CARD_GAP: i32 = 8;
pub const MAX_LIVE: usize = 3;
/// Persist cards (calendar reminders) stay on the desk past [`MAX_LIVE`].
/// Past this many, the oldest one goes.
pub const PERSIST_CAP: usize = 6;
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

/// An app with news since you were last in it: its icon shows in the
/// top row's notifications island.
#[derive(Debug, Clone)]
pub struct Lit {
    pub app_id: String,
    /// Its newest notification: clicking the icon is clicking that. `None`
    /// for mail lit by its unread count going up.
    pub newest: Option<AppNotification>,
}

#[derive(Debug, Clone, Default)]
pub struct NotifyState {
    pub live: Vec<Banner>,
    /// Lit apps, the one lit last first (the island's leftmost).
    pub lit: Vec<Lit>,
    generation: u64,
    /// An app asking to be the handler for a scheme or a type, waiting for
    /// your answer; above the live cards.
    pub prompt: Option<crate::handlers::Prompt>,
}

impl NotifyState {
    pub fn visible(&self) -> bool {
        !self.live.is_empty() || self.prompt.is_some()
    }

    /// Light `app_id`, with `newest` as what its icon opens. An app already
    /// lit keeps its place in the island.
    pub fn light(&mut self, app_id: &str, newest: Option<AppNotification>) {
        match self
            .lit
            .iter_mut()
            .find(|l| l.app_id.eq_ignore_ascii_case(app_id))
        {
            Some(l) => {
                if newest.is_some() {
                    l.newest = newest;
                }
            }
            None => self.lit.insert(
                0,
                Lit {
                    app_id: app_id.to_string(),
                    newest,
                },
            ),
        }
    }

    pub fn lit(&self, app_id: &str) -> Option<&Lit> {
        self.lit
            .iter()
            .find(|l| l.app_id.eq_ignore_ascii_case(app_id))
    }

    /// You are in `app_id`: its light goes out.
    pub fn put_out(&mut self, app_id: &str) {
        self.lit.retain(|l| !l.app_id.eq_ignore_ascii_case(app_id));
    }

    pub fn stack_height(&self) -> i32 {
        let cards = self.live.len() as i32;
        let prompt = i32::from(self.prompt.is_some());
        if cards + prompt == 0 {
            return CARD_H;
        }
        cards * CARD_H + prompt * PROMPT_H + (cards + prompt - 1) * CARD_GAP
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
            if let Some(pos) = self.live.iter().position(|b| !b.n.persists()) {
                self.live.remove(pos);
                continue;
            }
            if self.live.len() >= PERSIST_CAP {
                self.live.remove(0);
                continue;
            }
            break;
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
        if b.leaving || b.n.persists() {
            return false;
        }
        b.leaving = true;
        b.leave_at = Some(now);
        true
    }

    pub fn finish_leave(&mut self, now: Instant) -> bool {
        let before = self.live.len();
        self.live.retain(|b| {
            !b.leaving
                || b
                    .leave_at
                    .is_some_and(|t| now.saturating_duration_since(t) < LEAVE)
        });
        self.live.len() != before
    }

    /// A card or a lit icon clicked: the card goes; the notification to
    /// raise its app with.
    pub fn take(&mut self, id: &str) -> Option<AppNotification> {
        if let Some(pos) = self.live.iter().position(|b| b.n.id == id) {
            return Some(self.live.remove(pos).n);
        }
        self.lit
            .iter()
            .filter_map(|l| l.newest.as_ref())
            .find(|n| n.id == id)
            .cloned()
    }

    /// × without raising: the card goes, and its app's light when this was
    /// its newest.
    pub fn dismiss(&mut self, id: &str) -> bool {
        let before = (self.live.len(), self.lit.len());
        self.live.retain(|b| b.n.id != id);
        self.lit
            .retain(|l| l.newest.as_ref().is_none_or(|n| n.id != id));
        before != (self.live.len(), self.lit.len())
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
            app_id: "sola-projects".into(),
            source: "Projects".into(),
            title: "done".into(),
            body: String::new(),
            tag: tag.map(|s| s.into()),
            tab_id: None,
            url: None,
        }
    }

    #[test]
    fn a_card_expires_and_is_gone() {
        let mut s = NotifyState::default();
        let t0 = Instant::now();
        let g = s.push(n("a", None), t0);
        assert_eq!(s.live.len(), 1);
        assert!(s.begin_leave(g, t0));
        assert!(!s.finish_leave(t0), "still leaving");
        assert!(s.finish_leave(t0 + LEAVE));
        assert!(s.live.is_empty());
        assert!(s.take("a").is_none());
    }

    #[test]
    fn overflow_drops_the_oldest_card() {
        let mut s = NotifyState::default();
        let t0 = Instant::now();
        for i in 0..4 {
            s.push(n(&format!("n{i}"), None), t0);
        }
        assert_eq!(s.live.len(), MAX_LIVE);
        assert!(s.live.iter().all(|b| b.n.id != "n0"));
    }

    fn slack(id: &str) -> AppNotification {
        AppNotification {
            app_id: "web-slack".into(),
            source: "Slack".into(),
            title: id.into(),
            ..n(id, None)
        }
    }

    #[test]
    fn a_new_light_goes_left_and_a_lit_app_keeps_its_place() {
        let mut s = NotifyState::default();
        s.light("web-slack", Some(slack("a")));
        s.light("sola-calendar", None);
        assert_eq!(s.lit[0].app_id, "sola-calendar");
        s.light("web-slack", Some(slack("b")));
        assert_eq!(s.lit.len(), 2);
        assert_eq!(s.lit[1].app_id, "web-slack");
        assert_eq!(s.lit("WEB-SLACK").and_then(|l| l.newest.as_ref()).map(|n| n.id.as_str()), Some("b"));
        // Mail's count going up keeps the notification it has.
        s.light("web-slack", None);
        assert!(s.lit("web-slack").unwrap().newest.is_some());
        s.put_out("web-slack");
        assert!(s.lit("web-slack").is_none());
        assert_eq!(s.lit.len(), 1);
    }

    #[test]
    fn a_lit_icon_raises_with_its_newest() {
        let mut s = NotifyState::default();
        s.light("web-slack", Some(slack("b")));
        assert_eq!(s.take("b").map(|n| n.app_id), Some("web-slack".into()));
    }

    #[test]
    fn dismissing_the_newest_card_puts_its_app_out() {
        let mut s = NotifyState::default();
        let t0 = Instant::now();
        s.push(slack("a"), t0);
        s.light("web-slack", Some(slack("a")));
        s.push(slack("b"), t0);
        s.light("web-slack", Some(slack("b")));
        assert!(s.dismiss("a"));
        assert!(s.lit("web-slack").is_some(), "b is still news");
        assert!(s.dismiss("b"));
        assert!(s.lit("web-slack").is_none());
        assert!(s.live.is_empty());
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
    fn persist_stays_and_is_not_evicted_by_a_normal_card() {
        let mut s = NotifyState::default();
        let t0 = Instant::now();
        let g = s.push(n("stay", Some("sola-persist:ev:0")), t0);
        assert!(
            !s.begin_leave(g, t0),
            "a persist card ignores the 6s expire"
        );
        assert_eq!(s.live.len(), 1);
        for i in 0..MAX_LIVE {
            s.push(n(&format!("n{i}"), None), t0);
        }
        assert!(s.live.iter().any(|b| b.n.id == "stay"));
        assert!(s.live.iter().all(|b| b.n.id != "n0"));
    }

    #[test]
    fn past_the_persist_cap_the_oldest_goes() {
        let mut s = NotifyState::default();
        let t0 = Instant::now();
        for i in 0..(PERSIST_CAP + 1) {
            let tag = format!("sola-persist:{i}");
            s.push(n(&format!("p{i}"), Some(&tag)), t0);
        }
        assert_eq!(s.live.len(), PERSIST_CAP);
        assert!(s.live.iter().all(|b| b.n.id != "p0"));
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
