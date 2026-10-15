//! Tours: walking a canvas board to board along its exits and links (see
//! [`crate::scene::Scene::choices`]), and the camera glide between steps.
//! A tour starts from a `[[flow]]`, or by clicking an exit in a page (so
//! Back works). Panning and zooming never end it.

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
