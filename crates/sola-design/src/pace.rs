//! How often each board paints (`Cmd::BoardPace`). Under CPU OSR every
//! frame copies the whole board surface, so only what someone looks at runs
//! at full rate; the rest sleeps like a background browser tab.
//!
//! - A board doing work (loading, settling, answering a call) is `Live`.
//! - On the canvas on screen, a board in view is `Live` while Design has
//!   focus and `Slow` while it does not.
//! - Any other board of a canvas an agent called on lately is `Slow`.
//! - Every other board is `Asleep`. It keeps its last picture on the plane.

use iced::{Point, Rectangle, Size};
use sola_browser::engine::BoardPace;

use crate::camera::Camera;

/// Screen px around the plane that count as in view, so a short pan finds
/// the next boards already awake.
const MARGIN_PX: f32 = 256.0;

/// What a board's pace depends on.
#[derive(Debug, Clone, Copy, Default)]
pub struct Look {
    /// Loading, settling, or answering a call.
    pub busy: bool,
    /// On the canvas on screen and in view (or the plane is not laid out).
    pub in_view: bool,
    /// Design has keyboard focus.
    pub focused: bool,
    /// An agent called on its canvas lately.
    pub agent: bool,
}

pub fn pace(look: Look) -> BoardPace {
    if look.busy || (look.in_view && look.focused) {
        BoardPace::Live
    } else if look.in_view || look.agent {
        BoardPace::Slow
    } else {
        BoardPace::Asleep
    }
}

/// `board` (canvas units) is within [`MARGIN_PX`] of the plane.
pub fn in_view(board: Rectangle, camera: &Camera, plane: Size) -> bool {
    let tl = camera.to_canvas(Point::new(-MARGIN_PX, -MARGIN_PX));
    let br = camera.to_canvas(Point::new(plane.width + MARGIN_PX, plane.height + MARGIN_PX));
    let view = Rectangle::new(tl, Size::new(br.x - tl.x, br.y - tl.y));
    board.intersects(&view)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(busy: bool, in_view: bool, focused: bool, agent: bool) -> Look {
        Look {
            busy,
            in_view,
            focused,
            agent,
        }
    }

    #[test]
    fn work_runs_at_full_rate_anywhere() {
        assert_eq!(pace(look(true, false, false, false)), BoardPace::Live);
        assert_eq!(pace(look(true, true, false, false)), BoardPace::Live);
    }

    #[test]
    fn in_view_follows_window_focus() {
        assert_eq!(pace(look(false, true, true, false)), BoardPace::Live);
        assert_eq!(pace(look(false, true, false, false)), BoardPace::Slow);
    }

    #[test]
    fn unseen_boards_sleep_unless_an_agent_works_there() {
        assert_eq!(pace(look(false, false, true, false)), BoardPace::Asleep);
        assert_eq!(pace(look(false, false, true, true)), BoardPace::Slow);
    }

    #[test]
    fn view_is_the_plane_plus_a_margin() {
        let camera = Camera {
            offset: iced::Vector::new(0.0, 0.0),
            zoom: 0.5,
        };
        let plane = Size::new(1000.0, 800.0);
        // The plane shows canvas 0..2000 × 0..1600; the margin adds 512.
        let at = |x: f32| Rectangle::new(Point::new(x, 0.0), Size::new(100.0, 100.0));
        assert!(in_view(at(1900.0), &camera, plane));
        assert!(in_view(at(2400.0), &camera, plane));
        assert!(!in_view(at(2600.0), &camera, plane));
        assert!(!in_view(at(-700.0), &camera, plane));
    }
}
