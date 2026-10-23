//! Where the top row's right-hand islands put their items, walked right to
//! left from the output's edge with the view's own metrics: the x a panel
//! hangs from. Font-metric math, not a post-layout measurement — good
//! enough to drop a panel under its item (the caller clamps).

use sola_kit::fonts;

use super::{CHROME_SIZE, EDGE, ICON_SIZE, ISLAND_GAP, ISLAND_PAD_H, ITEM_GAP, STAT_INNER_SPACING};

/// What the right side shows: the islands and items that come and go.
pub struct Right<'a> {
    pub output_w: f32,
    /// The clock as drawn.
    pub clock: &'a str,
    pub gpu: bool,
    pub spectrum: bool,
}

/// Left edges of the right side's items.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edges {
    pub clock: f32,
    pub rx: f32,
    pub tx: f32,
    pub cpu: f32,
    /// Equals `mem` when there is no GPU.
    pub gpu: f32,
    pub mem: f32,
    /// The spectrum island's left edge (meaningless without one).
    pub spectrum: f32,
    /// Bluetooth, the extras island's last item.
    pub bluetooth: f32,
}

pub fn edges(r: &Right<'_>) -> Edges {
    edges_with(r, &|s| fonts::line_width(s, CHROME_SIZE, fonts::chrome()))
}

fn edges_with(r: &Right<'_>, text: &dyn Fn(&str) -> f32) -> Edges {
    let stat = |label: &str| text(label) + STAT_INNER_SPACING + crate::stats::pixel::GRAPH_W;
    // Walk left; `x` is the left edge of what was just placed.
    let mut x = r.output_w - EDGE - ISLAND_PAD_H - text(r.clock);
    let clock = x;
    x -= ISLAND_PAD_H + ISLAND_GAP + ISLAND_PAD_H + stat("TX");
    let tx = x;
    x -= ITEM_GAP + stat("RX");
    let rx = x;
    x -= ISLAND_PAD_H + ISLAND_GAP + ISLAND_PAD_H + stat("MEM");
    let mem = x;
    let gpu = if r.gpu {
        x -= ITEM_GAP + stat("GPU");
        x
    } else {
        mem
    };
    x -= ITEM_GAP + stat("CPU");
    let cpu = x;
    x -= ISLAND_PAD_H + ISLAND_GAP;
    let spectrum = if r.spectrum {
        x -= ISLAND_PAD_H + crate::audio::wave::SPECTRUM_W + ISLAND_PAD_H;
        let left = x;
        x -= ISLAND_GAP;
        left
    } else {
        x + ISLAND_GAP
    };
    let bluetooth = x - ISLAND_PAD_H - ICON_SIZE as f32;
    Edges {
        clock,
        rx,
        tx,
        cpu,
        gpu,
        mem,
        spectrum,
        bluetooth,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every label 10 px wide.
    fn ten(_: &str) -> f32 {
        10.0
    }

    #[test]
    fn items_walk_left_island_by_island() {
        let r = Right {
            output_w: 1000.0,
            clock: "14:32",
            gpu: true,
            spectrum: true,
        };
        let e = edges_with(&r, &ten);
        let stat = 10.0 + STAT_INNER_SPACING + crate::stats::pixel::GRAPH_W;
        assert_eq!(e.clock, 1000.0 - EDGE - ISLAND_PAD_H - 10.0);
        assert_eq!(e.tx, e.clock - 2.0 * ISLAND_PAD_H - ISLAND_GAP - stat);
        assert_eq!(e.rx, e.tx - ITEM_GAP - stat);
        assert_eq!(e.mem, e.rx - 2.0 * ISLAND_PAD_H - ISLAND_GAP - stat);
        assert_eq!(e.cpu, e.gpu - ITEM_GAP - stat);
        let spectrum_right = e.cpu - ISLAND_PAD_H - ISLAND_GAP;
        assert_eq!(e.spectrum, spectrum_right - crate::audio::wave::SPECTRUM_W - 2.0 * ISLAND_PAD_H);
        assert_eq!(e.bluetooth, e.spectrum - ISLAND_GAP - ISLAND_PAD_H - ICON_SIZE as f32);
    }

    #[test]
    fn without_gpu_or_spectrum_the_rest_close_up() {
        let all = Right {
            output_w: 1000.0,
            clock: "14:32",
            gpu: true,
            spectrum: true,
        };
        let bare = Right {
            gpu: false,
            spectrum: false,
            ..all
        };
        let (a, b) = (edges_with(&all, &ten), edges_with(&bare, &ten));
        assert_eq!(b.gpu, b.mem);
        assert!(b.cpu > a.cpu);
        assert!(b.bluetooth > a.bluetooth);
    }
}
