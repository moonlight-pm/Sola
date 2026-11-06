//! A squarified treemap: rectangles whose areas follow their values and
//! whose sides stay near square (Bruls, Huizing and van Wijk).

use iced::Rectangle;

/// Lay `values` (largest first reads best) into `area`; one rectangle per
/// value, in the same order.
pub fn squarify(values: &[f64], area: Rectangle) -> Vec<Rectangle> {
    let total: f64 = values.iter().sum();
    let mut out = vec![Rectangle::default(); values.len()];
    if total <= 0.0 || area.width <= 0.0 || area.height <= 0.0 {
        return out;
    }
    let scale = f64::from(area.width) * f64::from(area.height) / total;
    let areas: Vec<f64> = values.iter().map(|v| v * scale).collect();
    let (mut x, mut y, mut w, mut h) = (f64::from(area.x), f64::from(area.y), f64::from(area.width), f64::from(area.height));
    let mut i = 0;
    while i < areas.len() {
        let side = w.min(h).max(1e-9);
        let worst = |row: &[f64]| {
            let s: f64 = row.iter().sum();
            let (mx, mn) = row.iter().fold((0.0f64, f64::MAX), |(a, b), v| (a.max(*v), b.min(*v)));
            (side * side * mx / (s * s)).max(s * s / (side * side * mn.max(1e-12)))
        };
        let mut end = i + 1;
        while end < areas.len() && worst(&areas[i..=end]) <= worst(&areas[i..end]) {
            end += 1;
        }
        let row = &areas[i..end];
        let s: f64 = row.iter().sum();
        let thick = s / side;
        let mut o = 0.0;
        for (k, a) in row.iter().enumerate() {
            let len = a / thick;
            out[i + k] = if w >= h {
                Rectangle { x: x as f32, y: (y + o) as f32, width: thick as f32, height: len as f32 }
            } else {
                Rectangle { x: (x + o) as f32, y: y as f32, width: len as f32, height: thick as f32 }
            };
            o += len;
        }
        if w >= h {
            x += thick;
            w -= thick;
        } else {
            y += thick;
            h -= thick;
        }
        i = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{Point, Size};

    #[test]
    fn areas_follow_values_and_fill_the_box() {
        let area = Rectangle::new(Point::ORIGIN, Size::new(600.0, 400.0));
        let rs = squarify(&[6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0], area);
        let total: f32 = rs.iter().map(|r| r.width * r.height).sum();
        assert!((total - 240_000.0).abs() < 1.0);
        let a0 = rs[0].width * rs[0].height;
        assert!((a0 - 240_000.0 * 6.0 / 24.0).abs() < 1.0);
        for r in &rs {
            assert!(r.x >= -0.01 && r.y >= -0.01 && r.x + r.width <= 600.01 && r.y + r.height <= 400.01);
        }
    }

    #[test]
    fn nothing_to_lay_out() {
        let area = Rectangle::new(Point::ORIGIN, Size::new(10.0, 10.0));
        assert!(squarify(&[0.0, 0.0], area).iter().all(|r| r.width == 0.0));
    }
}
