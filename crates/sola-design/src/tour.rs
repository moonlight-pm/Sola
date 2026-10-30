//! Tours: walking a canvas board to board along its exits and links (see
//! [`crate::scene::Scene::choices`]), and the camera glide between steps.
//! A tour starts from a `[[flow]]`, or by clicking an exit in a page (so
//! Back works). Panning and zooming never end it.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use iced::Size;

use crate::camera::{self, Camera};

/// One camera move between steps.
pub const GLIDE: Duration = Duration::from_millis(420);
/// Room kept under a framed step for the tour bar and the pill.
pub const BAR_RESERVE: f32 = 132.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Tour {
    /// The flow's title, if it came from one.
    pub flow: Option<String>,
    /// Boards visited; the last is the step on screen.
    pub path: Vec<String>,
}

impl Tour {
    pub fn new(flow: Option<String>, start: String) -> Self {
        Self {
            flow,
            path: vec![start],
        }
    }

    pub fn step(&self) -> &str {
        self.path.last().map(String::as_str).unwrap_or_default()
    }

    /// 1-based step number.
    pub fn number(&self) -> usize {
        self.path.len()
    }

    pub fn go(&mut self, to: &str) {
        if self.step() != to {
            self.path.push(to.to_string());
        }
    }

    /// Back one step; false at the start.
    pub fn back(&mut self) -> bool {
        if self.path.len() < 2 {
            return false;
        }
        self.path.pop();
        true
    }

    /// Drop boards that are gone from the canvas; `false` when the step
    /// itself is gone (the tour ends).
    pub fn keep(&mut self, exists: impl Fn(&str) -> bool) -> bool {
        self.path.retain(|b| exists(b));
        self.path.dedup();
        !self.path.is_empty()
    }
}

/// Keys the plane takes for tours and notes while no page has the
/// keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Next,
    Back,
    End,
    /// A fork's choice, 1-based.
    Pick(usize),
    Notes,
}

/// Which note cards are open. On a tour step its notes are open; the notes
/// toggle opens every card, or closes every card while any is open (they
/// stay closed on the next steps). A click on a marker flips that one card
/// from what the toggle says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notes {
    shown: Shown,
    flipped: HashSet<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Shown {
    #[default]
    Step,
    All,
    Hidden,
}

impl Notes {
    /// Is note `n`, on `board` (none for a spot on the canvas), open while
    /// `step` is the tour's step?
    pub fn open(&self, n: usize, board: Option<&str>, step: Option<&str>) -> bool {
        let shown = match self.shown {
            Shown::All => true,
            Shown::Hidden => false,
            Shown::Step => step.is_some() && board == step,
        };
        shown != self.flipped.contains(&n)
    }

    /// A click on note `n`'s marker.
    pub fn flip(&mut self, n: usize) {
        if !self.flipped.remove(&n) {
            self.flipped.insert(n);
        }
    }

    /// The notes toggle, `any_open` saying whether any card is open now.
    pub fn toggle(&mut self, any_open: bool) {
        self.shown = if any_open { Shown::Hidden } else { Shown::All };
        self.flipped.clear();
    }

    /// Forget clicks on notes past `count` (the canvas lost them).
    pub fn keep(&mut self, count: usize) {
        self.flipped.retain(|n| *n <= count);
    }
}

/// A camera glide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tween {
    from: Camera,
    to: Camera,
    start: Instant,
}

impl Tween {
    pub fn new(from: Camera, to: Camera, now: Instant) -> Self {
        Self { from, to, start: now }
    }

    /// The camera at `now`, and whether the glide is over.
    pub fn at(&self, now: Instant, view: Size) -> (Camera, bool) {
        let t = now.duration_since(self.start).as_secs_f32() / GLIDE.as_secs_f32();
        if t >= 1.0 {
            return (self.to, true);
        }
        (camera::lerp(self.from, self.to, camera::ease(t), view), false)
    }
}

#[cfg(test)]
mod tests {
    use iced::Vector;

    use super::*;

    #[test]
    fn steps_go_back_and_survive_relayouts() {
        let mut t = Tour::new(Some("Sign in".into()), "a".into());
        assert_eq!((t.step(), t.number()), ("a", 1));
        assert!(!t.back(), "nothing before the start");
        t.go("b");
        t.go("b");
        t.go("c");
        assert_eq!((t.step(), t.number()), ("c", 3));
        assert!(t.back());
        assert_eq!(t.step(), "b");
        t.go("a");
        assert!(t.keep(|b| b != "b"));
        assert_eq!(t.path, vec!["a".to_string()], "a gone board leaves no repeat");
        assert!(!t.keep(|_| false));
    }

    #[test]
    fn the_toggle_closes_step_notes_and_they_stay_closed() {
        let mut notes = Notes::default();
        let (story, board) = (Some("story"), Some("board"));
        assert!(notes.open(7, story, story), "a step's notes open by themselves");
        assert!(!notes.open(2, board, story) && !notes.open(1, None, story));
        assert!(!notes.open(7, story, None), "off a tour, closed");

        notes.toggle(true);
        assert!(!notes.open(7, story, story), "the toggle closes the step's notes");
        assert!(!notes.open(2, board, board), "and the next step's");
        notes.flip(7);
        assert!(notes.open(7, story, story), "a marker click opens just that one");
        assert!(!notes.open(8, story, story));

        notes.toggle(true);
        assert!(!notes.open(7, story, story), "with one open, the toggle closes it");
        notes.toggle(false);
        assert!(notes.open(2, board, story) && notes.open(1, None, None), "with none open, it opens all");
        notes.flip(1);
        assert!(!notes.open(1, None, None), "a marker click closes one of all");
        notes.keep(0);
        assert!(notes.open(1, None, None), "a note the canvas lost forgets its click");
    }

    #[test]
    fn a_glide_lands_on_its_camera() {
        let now = Instant::now();
        let a = Camera { offset: Vector::new(0.0, 0.0), zoom: 0.2 };
        let b = Camera { offset: Vector::new(-300.0, 40.0), zoom: 1.0 };
        let g = Tween::new(a, b, now);
        let view = Size::new(800.0, 600.0);
        let (mid, done) = g.at(now + GLIDE / 2, view);
        assert!(!done && mid.zoom > a.zoom && mid.zoom < b.zoom);
        assert_eq!(g.at(now + GLIDE, view), (b, true));
    }
}
