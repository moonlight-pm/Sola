//! The stat panels' readout: a value in 5 × 7 dots (Instrument's face,
//! `design/instrument/instrument.js` F57), right-aligned in a fixed number
//! of cells so 83% and 100% take the same room and nothing shifts. Drawn on
//! the panel's LED grid ([`super::pixel::PANEL`]) with every unlit dot in
//! the field still showing, like the meters.

use iced::{Color, Element};

use super::pixel;

/// Glyph rows, top to bottom; `#` lit. Widths vary (`.` is one column);
/// a dark column separates glyphs.
fn glyph(c: char) -> &'static [&'static str; 7] {
    match c {
        '0' => &[".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
        '1' => &["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."],
        '2' => &[".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"],
        '3' => &["#####", "...#.", "..#..", "...#.", "....#", "#...#", ".###."],
        '4' => &["...#.", "..##.", ".#.#.", "#..#.", "#####", "...#.", "...#."],
        '5' => &["#####", "#....", "####.", "....#", "....#", "#...#", ".###."],
        '6' => &["..##.", ".#...", "#....", "####.", "#...#", "#...#", ".###."],
        '7' => &["#####", "....#", "...#.", "..#..", ".#...", ".#...", ".#..."],
        '8' => &[".###.", "#...#", "#...#", ".###.", "#...#", "#...#", ".###."],
        '9' => &[".###.", "#...#", "#...#", ".####", "....#", "...#.", ".##.."],
        'B' => &["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."],
        'G' => &[".###.", "#...#", "#....", "#.###", "#...#", "#...#", ".####"],
        'K' => &["#...#", "#..#.", "#.#..", "##...", "#.#..", "#..#.", "#...#"],
        'M' => &["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"],
        '%' => &["##...", "##..#", "...#.", "..#..", ".#...", "#..##", "...##"],
        '.' => &[".", ".", ".", ".", ".", ".", "#"],
        _ => &["..", "..", "..", "..", "..", "..", ".."],
    }
}

/// Glyph rows for `text`, one dark column between glyphs.
fn bits(text: &str) -> [Vec<bool>; 7] {
    let mut rows: [Vec<bool>; 7] = Default::default();
    for (i, c) in text.chars().enumerate() {
        let g = glyph(c.to_ascii_uppercase());
        for (y, line) in g.iter().enumerate() {
            if i > 0 {
                rows[y].push(false);
            }
            rows[y].extend(line.chars().map(|ch| ch == '#'));
        }
    }
    rows
}

/// The columns `cells` glyph cells take: five each, a gap between.
pub fn columns(cells: usize) -> usize {
    cells * 6 - 1
}

/// `text` right-aligned in `cells` cells, lit in `lit`; the rest of the
/// field unlit.
pub fn readout<'a, Message: 'a>(text: &str, cells: usize, lit: Color, unlit: Color) -> Element<'a, Message> {
    let grid = pixel::PANEL;
    let cols = columns(cells);
    let rows = bits(text);
    let width = rows[0].len().min(cols);
    let lead = cols - width;
    let (w, h) = (grid.span(cols), grid.span(7));
    let mut buf = vec![0u8; (w * h * 4) as usize];
    for (y, row) in rows.iter().enumerate() {
        for x in 0..cols {
            let on = x >= lead && row.get(x - lead).copied().unwrap_or(false);
            let (x0, y0) = (x as u32 * grid.pitch, y as u32 * grid.pitch);
            pixel::fill_cell(&mut buf, w, h, x0, y0, grid, if on { lit } else { unlit });
        }
    }
    pixel::image(w, h, buf)
}

/// A rate in bytes/second, in four cells: `980B`, `9.8K`, `84K`, `1.2M`.
pub fn rate(bps: f32) -> String {
    let unit = |v: f32, u: char| {
        if v < 9.95 {
            format!("{v:.1}{u}")
        } else {
            format!("{v:.0}{u}")
        }
    };
    if bps < 999.5 {
        format!("{:.0}B", bps.max(0.0))
    } else if bps < 999_500.0 {
        unit(bps / 1e3, 'K')
    } else if bps < 999_500_000.0 {
        unit(bps / 1e6, 'M')
    } else {
        unit(bps / 1e9, 'G')
    }
}

/// A percentage in four cells: `7%`, `83%`, `100%`.
pub fn percent(p: f32) -> String {
    format!("{:.0}%", p.clamp(0.0, 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_are_seven_rows_of_their_width() {
        for c in "0123456789BGKM%. ".chars() {
            let g = glyph(c);
            let w = g[0].len();
            assert!(g.iter().all(|r| r.len() == w), "{c}");
        }
    }

    #[test]
    fn the_widest_value_fits_four_cells() {
        // 100%: three 5-wide glyphs and a 5-wide %, three gaps.
        assert_eq!(bits("100%")[0].len(), columns(4));
        assert!(bits("1.2M")[0].len() <= columns(4));
        assert!(bits("980B")[0].len() <= columns(4));
    }

    #[test]
    fn rates_stay_in_four_cells() {
        assert_eq!(rate(0.0), "0B");
        assert_eq!(rate(999.0), "999B");
        assert_eq!(rate(9_800.0), "9.8K");
        assert_eq!(rate(84_000.0), "84K");
        assert_eq!(rate(999_400.0), "999K");
        assert_eq!(rate(999_600.0), "1.0M");
        assert_eq!(rate(1_234_567.0), "1.2M");
        assert_eq!(rate(120_000_000.0), "120M");
        for b in [0.0, 512.0, 9_949.0, 99_999.0, 5e6, 50e6, 999e6, 2e9] {
            assert!(bits(&rate(b))[0].len() <= columns(4), "{b} -> {}", rate(b));
        }
    }
}
