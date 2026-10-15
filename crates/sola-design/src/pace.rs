//! How often each board may paint (`Cmd::BoardFrameRate`), from what the
//! human can see. Full speed for boards on screen at a readable size;
//! slower when they are tiny on screen, slow when they are off screen or on
//! a canvas that is not shown. A board that is still loading paints at full
//! speed (a settle waits on its frames). Motion nobody can see costs the
//! helper's compositor, the frame socket, and a present of the window.

/// On screen at a readable size.
pub const FULL: u32 = 60;
/// On screen, too small for motion to read.
pub const SMALL: u32 = 10;
/// Off screen, or on a canvas that is not shown.
pub const HIDDEN: u32 = 4;
/// Below this zoom a board's motion does not read.
pub const READABLE_ZOOM: f32 = 0.3;

/// Frame rate for one board.
pub fn fps(on_screen: bool, zoom: f32, loading: bool) -> u32 {
    if loading {
        FULL
    } else if !on_screen {
        HIDDEN
    } else if zoom < READABLE_ZOOM {
        SMALL
    } else {
        FULL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_human_can_see_paints_fastest() {
        assert_eq!(fps(true, 1.0, false), FULL);
        assert_eq!(fps(true, 0.08, false), SMALL);
        assert_eq!(fps(false, 1.0, false), HIDDEN);
        assert_eq!(fps(false, 0.08, true), FULL, "loading boards settle on frames");
    }
}
