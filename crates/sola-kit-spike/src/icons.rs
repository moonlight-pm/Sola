//! Rasterize lucide SVGs from `/opt/sola/share/icons` and tint them.
//!
//! Coverage stays in alpha; RGB is the tint (straight). Glyph overlay
//! composites onto the CSS box colour — do not premultiply here or AA
//! fringes go black (wispy).

use std::collections::HashMap;

use crate::css::Rgba;

pub struct Icons {
    cache: HashMap<(String, u32, u32), Vec<u8>>,
}

impl Icons {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// RGBA8 pixels, `size * size`. RGB is the tint; A is coverage.
    pub fn rgba(&mut self, name: &str, size: u32, tint: Rgba) -> Option<&[u8]> {
        let size = size.max(8).min(64);
        let key = (name.to_string(), size, pack_tint(tint));
        if !self.cache.contains_key(&key) {
            let pix = raster(name, size, tint)?;
            self.cache.insert(key.clone(), pix);
        }
        self.cache.get(&key).map(|v| v.as_slice())
    }
}

fn pack_tint(c: Rgba) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
}

fn raster(name: &str, size: u32, tint: Rgba) -> Option<Vec<u8>> {
    let path = format!("/opt/sola/share/icons/{name}.svg");
    let data = std::fs::read(&path).ok()?;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).ok()?;
    // 2× then box-filter: 16px lucide strokes stay solid like iced SVG.
    let hi = (size * 2).min(128);
    let mut pixmap = tiny_skia::Pixmap::new(hi, hi)?;
    let ts = tree.size();
    let sx = hi as f32 / ts.width().max(1.0);
    let sy = hi as f32 / ts.height().max(1.0);
    let transform = tiny_skia::Transform::from_scale(sx, sy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let src = pixmap.data();
    let mut out = vec![0u8; (size * size * 4) as usize];
    let scale = (hi / size).max(1);
    for y in 0..size {
        for x in 0..size {
            let mut a_sum = 0.0f32;
            let n = (scale * scale) as f32;
            for oy in 0..scale {
                for ox in 0..scale {
                    let sx = x * scale + ox;
                    let sy = y * scale + oy;
                    let i = ((sy * hi + sx) * 4) as usize;
                    if i + 3 < src.len() {
                        a_sum += src[i + 3] as f32;
                    }
                }
            }
            let a = (a_sum / n).round().clamp(0.0, 255.0) as u8;
            let o = ((y * size + x) * 4) as usize;
            out[o] = tint.r;
            out[o + 1] = tint.g;
            out[o + 2] = tint.b;
            out[o + 3] = a;
        }
    }
    Some(out)
}
