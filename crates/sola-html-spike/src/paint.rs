//! Software paint: filled boxes + cosmic-text glyph runs (no bitmaps).

use cosmic_text::{
    Attrs, Buffer, Color as CosmicColor, Family, FontSystem, Metrics, Shaping, SwashCache, Weight,
    Wrap,
};

use crate::css::Rgba;
use crate::layout::PaintItem;

pub struct Fonts {
    pub system: FontSystem,
    pub swash: SwashCache,
}

impl Fonts {
    pub fn new() -> Self {
        Self {
            system: FontSystem::new(),
            swash: SwashCache::new(),
        }
    }

    pub fn measure_width(&mut self, text: &str, size: f32, weight: u16, family: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let metrics = Metrics::new(size, size * 1.3);
        let mut buffer = Buffer::new(&mut self.system, metrics);
        buffer.set_wrap(&mut self.system, Wrap::None);
        buffer.set_size(&mut self.system, Some(2000.0), Some(size * 2.0));
        let attrs = Attrs::new()
            .family(Family::Name(family))
            .weight(Weight(weight));
        buffer.set_text(&mut self.system, text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.system, false);
        buffer
            .layout_runs()
            .map(|r| r.line_w)
            .fold(0.0_f32, f32::max)
    }
}

pub fn draw_label(
    buf: &mut [u32],
    w: u32,
    h: u32,
    fonts: &mut Fonts,
    text: &str,
    x: f32,
    y: f32,
    box_w: f32,
    box_h: f32,
    color: Rgba,
    size: f32,
    weight: u16,
    family: &str,
    clip: Option<(f32, f32, f32, f32)>,
) {
    draw_text(
        buf, w, h, fonts, text, x, y, box_w, box_h, color, size, weight, family, clip, None,
    );
}

pub fn paint(items: &[PaintItem], fonts: &mut Fonts, css_w: f32, css_h: f32, scale: f32) -> Vec<u32> {
    let s = scale.max(0.01);
    let w = (css_w * s).round().max(1.0) as u32;
    let h = (css_h * s).round().max(1.0) as u32;
    let mut buf = vec![0x000c0e12u32; (w as usize) * (h as usize)];
    for item in items {
        if item.hidden {
            continue;
        }
        let clip = item.clip.map(|(x, y, cw, ch)| (x * s, y * s, cw * s, ch * s));
        if let Some(bg) = item.bg {
            fill_round(
                &mut buf,
                w,
                h,
                item.x * s,
                item.y * s,
                item.w * s,
                item.h * s,
                item.radius * s,
                bg,
                bg.a,
                clip,
            );
        }
        if let Some((bw, col)) = item.border {
            stroke_rect(
                &mut buf,
                w,
                h,
                item.x * s,
                item.y * s,
                item.w * s,
                item.h * s,
                bw * s,
                col,
                clip,
            );
        }
        if let Some(run) = &item.text {
            draw_text(
                &mut buf,
                w,
                h,
                fonts,
                run.text.as_str(),
                (item.x + item.pad[3]) * s,
                (item.y + item.pad[0]) * s,
                (item.w - item.pad[1] - item.pad[3]).max(1.0) * s,
                (item.h - item.pad[0] - item.pad[2]).max(1.0) * s,
                run.color,
                run.size * s,
                run.weight,
                run.family.as_str(),
                clip,
                None,
            );
        }
    }
    buf
}

/// Glyphs only (alpha in the high byte). CSS boxes are GPU quads on the live path.
pub fn paint_glyphs(
    items: &[PaintItem],
    fonts: &mut Fonts,
    css_w: f32,
    css_h: f32,
    scale: f32,
) -> Vec<u32> {
    let s = scale.max(0.01);
    let w = (css_w * s).round().max(1.0) as u32;
    let h = (css_h * s).round().max(1.0) as u32;
    let mut buf = vec![0u32; (w as usize) * (h as usize)];
    for (i, item) in items.iter().enumerate() {
        if item.hidden {
            continue;
        }
        let Some(run) = &item.text else {
            continue;
        };
        let clip = item.clip.map(|(x, y, cw, ch)| (x * s, y * s, cw * s, ch * s));
        let bg = covering_bg(items, i);
        draw_text(
            &mut buf,
            w,
            h,
            fonts,
            run.text.as_str(),
            (item.x + item.pad[3]) * s,
            (item.y + item.pad[0]) * s,
            (item.w - item.pad[1] - item.pad[3]).max(1.0) * s,
            (item.h - item.pad[0] - item.pad[2]).max(1.0) * s,
            run.color,
            run.size * s,
            run.weight,
            run.family.as_str(),
            clip,
            Some(bg),
        );
    }
    buf
}

/// Nearest ancestor (or self) with a fill — parents are collected first.
fn covering_bg(items: &[PaintItem], idx: usize) -> Rgba {
    let it = &items[idx];
    if let Some(bg) = it.bg {
        return bg;
    }
    let cx = it.x + it.w * 0.5;
    let cy = it.y + it.h * 0.5;
    for p in items[..idx].iter().rev() {
        if p.hidden {
            continue;
        }
        let Some(bg) = p.bg else {
            continue;
        };
        if cx >= p.x && cy >= p.y && cx < p.x + p.w && cy < p.y + p.h {
            return bg;
        }
    }
    Rgba::rgb(0x0c, 0x0e, 0x12)
}

fn clamp_i(v: f32) -> i32 {
    v.round() as i32
}

fn put(
    buf: &mut [u32],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    col: Rgba,
    a: u8,
    clip: Option<(f32, f32, f32, f32)>,
    dest_bg: Option<Rgba>,
) {
    if x < 0 || y < 0 || a == 0 {
        return;
    }
    if let Some((cx, cy, cw, ch)) = clip {
        let px = x as f32 + 0.5;
        let py = y as f32 + 0.5;
        if px < cx || py < cy || px >= cx + cw || py >= cy + ch {
            return;
        }
    }
    let ux = x as u32;
    let uy = y as u32;
    if ux >= w || uy >= h {
        return;
    }
    let i = (uy * w + ux) as usize;
    if a == 255 {
        buf[i] = 0xFF000000 | col.to_u32();
        return;
    }
    let (dr, dg, db) = if let Some(bg) = dest_bg {
        (bg.r, bg.g, bg.b)
    } else {
        let dst = buf[i];
        (
            ((dst >> 16) & 0xff) as u8,
            ((dst >> 8) & 0xff) as u8,
            (dst & 0xff) as u8,
        )
    };
    let cov = a as f32 / 255.0;
    let inv = 1.0 - cov;
    let r = blend_srgb(col.r, dr, cov, inv);
    let g = blend_srgb(col.g, dg, cov, inv);
    let b = blend_srgb(col.b, db, cov, inv);
    // Opaque: already composited in linear space onto the CSS box colour so
    // the GPU overlay can replace, not gamma-blend over black.
    buf[i] = 0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn blend_srgb(src: u8, dst: u8, cov: f32, inv: f32) -> u8 {
    let out = cov * srgb_to_linear(src as f32 / 255.0) + inv * srgb_to_linear(dst as f32 / 255.0);
    (linear_to_srgb(out.clamp(0.0, 1.0)) * 255.0).round() as u8
}

fn fill_round(
    buf: &mut [u32],
    w: u32,
    h: u32,
    x: f32,
    y: f32,
    bw: f32,
    bh: f32,
    radius: f32,
    col: Rgba,
    a: u8,
    clip: Option<(f32, f32, f32, f32)>,
) {
    let x0 = clamp_i(x);
    let y0 = clamp_i(y);
    let x1 = clamp_i(x + bw);
    let y1 = clamp_i(y + bh);
    let r = radius.max(0.0);
    for py in y0..y1 {
        for px in x0..x1 {
            if r > 0.5 && !inside_round(px as f32 + 0.5, py as f32 + 0.5, x, y, bw, bh, r) {
                continue;
            }
            put(buf, w, h, px, py, col, a, clip, None);
        }
    }
}

fn inside_round(px: f32, py: f32, x: f32, y: f32, bw: f32, bh: f32, r: f32) -> bool {
    let cx = if px < x + r {
        x + r
    } else if px > x + bw - r {
        x + bw - r
    } else {
        return py >= y && py <= y + bh;
    };
    let cy = if py < y + r {
        y + r
    } else if py > y + bh - r {
        y + bh - r
    } else {
        return true;
    };
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy <= r * r
}

fn stroke_rect(
    buf: &mut [u32],
    w: u32,
    h: u32,
    x: f32,
    y: f32,
    bw: f32,
    bh: f32,
    thickness: f32,
    col: Rgba,
    clip: Option<(f32, f32, f32, f32)>,
) {
    let t = thickness.max(1.0);
    fill_round(buf, w, h, x, y, bw, t, 0.0, col, 255, clip);
    fill_round(buf, w, h, x, y + bh - t, bw, t, 0.0, col, 255, clip);
    fill_round(buf, w, h, x, y, t, bh, 0.0, col, 255, clip);
    fill_round(buf, w, h, x + bw - t, y, t, bh, 0.0, col, 255, clip);
}

fn draw_text(
    buf: &mut [u32],
    w: u32,
    h: u32,
    fonts: &mut Fonts,
    text: &str,
    x: f32,
    y: f32,
    box_w: f32,
    box_h: f32,
    color: Rgba,
    size: f32,
    weight: u16,
    family: &str,
    clip: Option<(f32, f32, f32, f32)>,
    dest_bg: Option<Rgba>,
) {
    let metrics = Metrics::new(size, size * 1.3);
    let mut buffer = Buffer::new(&mut fonts.system, metrics);
    buffer.set_wrap(&mut fonts.system, Wrap::None);
    buffer.set_size(
        &mut fonts.system,
        Some(box_w.max(1.0)),
        Some(box_h.max(1.0)),
    );
    let attrs = Attrs::new()
        .family(Family::Name(family))
        .weight(Weight(weight));
    buffer.set_text(&mut fonts.system, text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut fonts.system, false);
    let y0 = ((box_h - size * 1.3) / 2.0).max(0.0);
    let cosmic = CosmicColor::rgba(color.r, color.g, color.b, 255);
    let ox = x.round() as i32;
    let oy = (y + y0).round() as i32;
    buffer.draw(
        &mut fonts.system,
        &mut fonts.swash,
        cosmic,
        |gx, gy, _, _, pix| {
            let (r, g, b, a) = pix.as_rgba_tuple();
            put(
                buf,
                w,
                h,
                ox + gx,
                oy + gy,
                Rgba { r, g, b, a: 255 },
                a,
                clip,
                dest_bg,
            );
        },
    );
}
