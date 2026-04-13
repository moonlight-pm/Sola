pub const SIDEBAR_WIDTH: i32 = 220;
pub const TOPBAR_HEIGHT: i32 = 32;

pub struct ContentArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn content_area(window_width: i32, window_height: i32) -> ContentArea {
    ContentArea {
        x: SIDEBAR_WIDTH,
        y: TOPBAR_HEIGHT,
        width: (window_width - SIDEBAR_WIDTH).max(0),
        height: (window_height - TOPBAR_HEIGHT).max(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_area_calculation() {
        let area = content_area(1920, 1080);
        assert_eq!(area.x, 220);
        assert_eq!(area.y, 32);
        assert_eq!(area.width, 1700);
        assert_eq!(area.height, 1048);
    }

    #[test]
    fn content_area_small_window() {
        let area = content_area(100, 30);
        assert_eq!(area.width, 0);
        assert_eq!(area.height, 0);
    }
}
