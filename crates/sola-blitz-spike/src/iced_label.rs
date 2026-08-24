//! Sidebar labels laid out and painted with cosmic-text (same stack as iced).
//! In-memory PNG as a data: URL for <img> — no disk.

use base64::Engine;
use cosmic_text::{
    Attrs, Buffer, Color as CosmicColor, Family, FontSystem, Metrics, Shaping, SwashCache, Weight,
    Wrap,
};
use std::cell::RefCell;
use std::collections::HashMap;

pub const SIZE_PX: f32 = 12.0;
pub const LINE_HEIGHT: f32 = SIZE_PX * 1.3;
pub const LABEL_W: u32 = 190;
pub const LABEL_H: u32 = 16;

struct FontPack {
    system: FontSystem,
    swash: SwashCache,
}

thread_local! {
    static FONTS: RefCell<FontPack> = RefCell::new(FontPack {
        system: FontSystem::new(),
        swash: SwashCache::new(),
    });
    static URLS: RefCell<HashMap<(String, [u8; 4], u16), String>> = RefCell::new(HashMap::new());
}

pub fn label_data_url(text: &str, rgba: [u8; 4], weight: u16) -> String {
    URLS.with(|urls| {
        let mut urls = urls.borrow_mut();
        let key = (text.to_string(), rgba, weight);
        if let Some(url) = urls.get(&key) {
            return url.clone();
        }
        let png = raster_png(text, rgba, weight, LABEL_W, LABEL_H);
        let url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        );
        urls.insert(key, url.clone());
        url
    })
}

fn raster_png(text: &str, rgba: [u8; 4], weight: u16, w: u32, h: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    let [cr, cg, cb, _] = rgba;
    let color = CosmicColor::rgba(cr, cg, cb, 0xff);

    FONTS.with(|cell| {
        let mut pack = cell.borrow_mut();
        let families = ["SF Pro Text", "Inter"];
        for family in families {
            let metrics = Metrics::new(SIZE_PX, LINE_HEIGHT);
            let mut buffer = Buffer::new(&mut pack.system, metrics);
            buffer.set_wrap(&mut pack.system, Wrap::None);
            buffer.set_size(&mut pack.system, Some(w as f32), Some(h as f32));
            let attrs = Attrs::new()
                .family(Family::Name(family))
                .weight(Weight(weight))
                .stretch(cosmic_text::Stretch::Normal)
                .style(cosmic_text::Style::Normal);
            buffer.set_text(&mut pack.system, text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut pack.system, false);
            let has_glyphs = buffer.layout_runs().any(|run| !run.glyphs.is_empty());
            if !has_glyphs {
                continue;
            }
            let FontPack { system, swash } = &mut *pack;
            buffer.draw(system, swash, color, |x, y, _, _, pix| {
                if x < 0 || y < 0 {
                    return;
                }
                let ux = x as u32;
                let uy = y as u32;
                if ux >= w || uy >= h {
                    return;
                }
                let (r, g, b, a) = pix.as_rgba_tuple();
                let i = ((uy * w + ux) * 4) as usize;
                // Straight alpha: Blitz decodes PNG as unpremultiplied.
                pixels[i] = r;
                pixels[i + 1] = g;
                pixels[i + 2] = b;
                pixels[i + 3] = a;
            });
            break;
        }
        let painted = pixels.chunks(4).filter(|p| p[3] > 0).count();
        tracing::info!(text, painted, w, h, "cosmic-text label raster");
    });

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer.write_image_data(&pixels).expect("png data");
    }
    out
}
