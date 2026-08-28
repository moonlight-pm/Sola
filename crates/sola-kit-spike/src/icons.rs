//! Rasterize lucide SVGs from `/opt/sola/share/icons` and tint them.

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

    /// RGBA8 pixels, `size * size`.
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
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    let ts = tree.size();
    let sx = size as f32 / ts.width().max(1.0);
    let sy = size as f32 / ts.height().max(1.0);
    let transform = tiny_skia::Transform::from_scale(sx, sy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut out = pixmap.data().to_vec();
    for px in out.chunks_exact_mut(4) {
        let a = px[3] as f32 / 255.0;
        if a <= 0.0 {
            continue;
        }
        px[0] = ((tint.r as f32) * a) as u8;
        px[1] = ((tint.g as f32) * a) as u8;
        px[2] = ((tint.b as f32) * a) as u8;
    }
    Some(out)
}
