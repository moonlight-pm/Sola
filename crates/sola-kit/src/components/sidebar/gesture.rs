//! Kit-owned sidebar gesture: hover, resize, and ReorderStrip outcomes.
//!
//! Reorder dest lives in [`super::strip`] (Morph2 hole + FLIP). This
//! module is hover + the divider, plus forwarding [`Msg::Outcome`].

use super::{PANEL_W_MAX, PANEL_W_MIN};
use iced::Subscription;
use iced::event::{self, Event as IcedEvent};
use iced::mouse;

/// Opaque hover / resize state. Hold one per sidebar.
#[derive(Debug, Default)]
pub struct State {
    hover: Option<String>,
    divider: Option<DividerDrag>,
}

#[derive(Debug, Clone, Copy)]
struct DividerDrag {
    anchor_x: f32,
    anchor_w: f32,
}

/// Messages the panel emits. Forward every one into [`State::update`].
#[derive(Debug, Clone)]
pub enum Msg {
    PressDivider {
        width: f32,
    },
    Pointer {
        x: f32,
        y: f32,
    },
    Release,
    Hover(Option<String>),
    /// ReorderStrip already resolved an outcome.
    Outcome(Event),
}

/// Semantic outcome after [`State::update`].
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Activate { id: String },
    ToggleSection { id: String },
    Drop(Drop),
    Resize { width: f32 },
}

/// A finished drag of one visible row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drop {
    pub id: String,
    pub dest: Dest,
}

/// Where the dragged row should land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dest {
    /// Join `section`. `before` is the member to sit in front of (`None` = append).
    Join {
        section: String,
        before: Option<String>,
    },
    /// Become a singleton. `before` is the next singleton item (`None` = end).
    Loose { before: Option<String> },
    /// Park as a singleton immediately before this named group.
    BeforeGroup { id: String },
    /// Header drag: place this group block before `before` (group id or
    /// item id). `None` is the end of the strip. Groups and loose rows mix.
    BlockBefore { before: Option<String> },
    /// Header drag: grouped section ids in the new order (legacy).
    Sections(Vec<String>),
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hover(&self) -> Option<&str> {
        self.hover.as_deref()
    }

    pub fn resizing(&self) -> bool {
        self.divider.is_some()
    }

    pub fn reordering(&self) -> bool {
        false
    }

    /// True while the resize divider is live.
    pub fn capturing(&self) -> bool {
        self.divider.is_some()
    }

    /// Pointer samples while the divider is captured (the overlay would
    /// steal widget `on_move` / `on_release`).
    pub fn subscription(&self) -> Subscription<Msg> {
        if !self.capturing() {
            return Subscription::none();
        }
        event::listen_with(|ev, _status, _| match ev {
            IcedEvent::Mouse(mouse::Event::CursorMoved { position }) => Some(Msg::Pointer {
                x: position.x,
                y: position.y,
            }),
            IcedEvent::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                Some(Msg::Release)
            }
            _ => None,
        })
    }

    pub fn update(&mut self, msg: Msg) -> Option<Event> {
        match msg {
            Msg::Hover(id) => {
                self.hover = id;
                None
            }
            Msg::PressDivider { width } => {
                self.divider = Some(DividerDrag {
                    anchor_x: f32::NAN,
                    anchor_w: width,
                });
                None
            }
            Msg::Pointer { x, y: _ } => {
                let x = super::px(x);
                let Some(d) = &mut self.divider else {
                    return None;
                };
                if d.anchor_x.is_nan() {
                    d.anchor_x = x;
                }
                let desired = d.anchor_w + (x - d.anchor_x);
                let width = desired.clamp(PANEL_W_MIN, PANEL_W_MAX);
                Some(Event::Resize { width })
            }
            Msg::Outcome(ev) => Some(ev),
            Msg::Release => {
                self.divider = None;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divider_emits_resize() {
        let mut s = State::new();
        assert!(s.update(Msg::PressDivider { width: 160.0 }).is_none());
        assert!(s.update(Msg::Pointer { x: 200.0, y: 0.0 }).is_some());
        match s.update(Msg::Pointer { x: 220.0, y: 0.0 }) {
            Some(Event::Resize { width }) => assert!((width - 180.0).abs() < 0.5),
            other => panic!("{other:?}"),
        }
        assert!(s.update(Msg::Release).is_none());
        assert!(!s.capturing());
    }

    #[test]
    fn outcome_forwards() {
        let mut s = State::new();
        let ev = Event::Activate { id: "a1".into() };
        assert_eq!(s.update(Msg::Outcome(ev.clone())), Some(ev));
    }
}
