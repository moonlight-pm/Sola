//! Drawing a lock surface: the background picture under a dark veil and,
//! on the main output, the time, the date and the password field (its dots,
//! and what happened to the last try).

use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Weight,
};
use image::RgbaImage;
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

/// The kit's UI faces, the first one installed; else the system's sans.
const FAMILIES: [&str; 2] = ["SF Pro Text", "Inter"];
const GROUND: [u8; 3] = [16, 16, 20];

/// What a surface shows.
pub struct Scene<'a> {
    pub time: &'a str,
    pub date: &'a str,
    /// Characters typed (shown as dots).
    pub typed: usize,
    /// Under the field: "Checking…", "Wrong password", …
    pub message: &'a str,
    pub error: bool,
    /// The output that has the field; the others show the picture only.
    pub main: bool,
}

pub struct Painter {
    fonts: FontSystem,
    cache: SwashCache,
    family: Option<&'static str>,
}

impl Painter {
    pub fn new() -> Self {
        let fonts = FontSystem::new();
        if fonts.db().is_empty() {
            // Locks all the same: the field, without words.
            tracing::warn!("no fonts found (FONTCONFIG_FILE?): drawing without text");
        }
        let family = FAMILIES.into_iter().find(|name| {
            fonts
                .db()
                .faces()
                .any(|face| face.families.iter().any(|(f, _)| f == name))
        });
        Self {
            fonts,
            cache: SwashCache::new(),
            family,
        }
    }

    /// `scene` into `canvas` (ARGB8888, `w`×`h` buffer pixels at `scale`).
    /// `backdrop` is the picture already cut to `w`×`h`.
    pub fn paint(
        &mut self,
        canvas: &mut [u8],
        w: u32,
        h: u32,
        scale: f32,
        backdrop: Option<&RgbaImage>,
        scene: &Scene,
    ) {
        let Some(mut px) = Pixmap::new(w, h) else {
            return;
        };
        match backdrop {
            Some(img) if img.dimensions() == (w, h) => {
                for (d, s) in px.data_mut().chunks_exact_mut(4).zip(img.as_raw().chunks_exact(4)) {
                    d.copy_from_slice(&[s[0], s[1], s[2], 255]);
                }
            }
            _ => px.fill(tiny_skia::Color::from_rgba8(GROUND[0], GROUND[1], GROUND[2], 255)),
        }
        fill(&mut px, Rect::from_xywh(0.0, 0.0, w as f32, h as f32), [0, 0, 0, 150]);

        if scene.main {
            let cx = w as f32 / 2.0;
            let top = h as f32 * 0.26;
            let s = scale;
            self.text(&mut px, scene.time, 96.0 * s, Weight::NORMAL, [255, 255, 255, 240], cx, top);
            self.text(&mut px, scene.date, 22.0 * s, Weight::NORMAL, [255, 255, 255, 200], cx, top + 132.0 * s);

            let (fw, fh) = (300.0 * s, 46.0 * s);
            let fy = h as f32 * 0.52;
            let field = rounded(cx - fw / 2.0, fy, fw, fh, 12.0 * s);
            fill_path(&mut px, &field, [255, 255, 255, 34]);
            stroke_path(&mut px, &field, [255, 255, 255, if scene.error { 60 } else { 90 }], 1.0 * s);
            if scene.typed == 0 {
                self.text(&mut px, "Password", 16.0 * s, Weight::NORMAL, [255, 255, 255, 110], cx, fy + 12.0 * s);
            } else {
                let step = 16.0 * s;
                let fits = ((fw - 24.0 * s) / step) as usize;
                let n = scene.typed.min(fits.max(1));
                let x0 = cx - (n as f32 - 1.0) * step / 2.0;
                for i in 0..n {
                    dot(&mut px, x0 + i as f32 * step, fy + fh / 2.0, 4.5 * s, [255, 255, 255, 235]);
                }
            }
            let color = if scene.error {
                [255, 150, 140, 240]
            } else {
                [255, 255, 255, 200]
            };
            self.text(&mut px, scene.message, 15.0 * s, Weight::NORMAL, color, cx, fy + fh + 16.0 * s);
        }

        for (d, s) in canvas.chunks_exact_mut(4).zip(px.data().chunks_exact(4)) {
            d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
        }
    }

    /// One line of `text`, centered on `cx`, its top at `top`.
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, px: &mut Pixmap, text: &str, size: f32, weight: Weight, rgba: [u8; 4], cx: f32, top: f32) {
        if text.is_empty() || self.fonts.db().is_empty() {
            return;
        }
        let mut buf = Buffer::new(&mut self.fonts, Metrics::new(size, size * 1.25));
        let family = self.family.map_or(Family::SansSerif, Family::Name);
        let attrs = Attrs::new().family(family).weight(weight);
        buf.set_size(&mut self.fonts, Some(px.width() as f32), None);
        buf.set_text(&mut self.fonts, text, &attrs, Shaping::Advanced, Some(Align::Left));
        buf.shape_until_scroll(&mut self.fonts, false);
        let width = buf.layout_runs().map(|r| r.line_w).fold(0.0, f32::max);
        let (x0, y0) = ((cx - width / 2.0).round() as i32, top.round() as i32);
        let color = Color::rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
        let (pw, ph) = (px.width() as i32, px.height() as i32);
        let data = px.data_mut();
        buf.draw(&mut self.fonts, &mut self.cache, color, |x, y, w, h, c| {
            for yy in y0 + y..y0 + y + h as i32 {
                for xx in x0 + x..x0 + x + w as i32 {
                    if (0..pw).contains(&xx) && (0..ph).contains(&yy) {
                        let i = ((yy * pw + xx) * 4) as usize;
                        blend(&mut data[i..i + 4], [c.r(), c.g(), c.b(), c.a()]);
                    }
                }
            }
        });
    }
}

/// `src` (straight alpha) over a premultiplied pixel.
fn blend(dst: &mut [u8], src: [u8; 4]) {
    let a = src[3] as u32;
    for k in 0..3 {
        dst[k] = ((src[k] as u32 * a + dst[k] as u32 * (255 - a)) / 255) as u8;
    }
    dst[3] = (a + dst[3] as u32 * (255 - a) / 255) as u8;
}

fn paint(rgba: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    p.anti_alias = true;
    p
}

fn fill(px: &mut Pixmap, rect: Option<Rect>, rgba: [u8; 4]) {
    if let Some(r) = rect {
        px.fill_rect(r, &paint(rgba), Transform::identity(), None);
    }
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

fn fill_path(px: &mut Pixmap, path: &Option<tiny_skia::Path>, rgba: [u8; 4]) {
    if let Some(p) = path {
        px.fill_path(p, &paint(rgba), FillRule::Winding, Transform::identity(), None);
    }
}

fn stroke_path(px: &mut Pixmap, path: &Option<tiny_skia::Path>, rgba: [u8; 4], width: f32) {
    if let Some(p) = path {
        let stroke = Stroke {
            width,
            ..Stroke::default()
        };
        px.stroke_path(p, &paint(rgba), &stroke, Transform::identity(), None);
    }
}

fn dot(px: &mut Pixmap, cx: f32, cy: f32, r: f32, rgba: [u8; 4]) {
    if let Some(c) = PathBuilder::from_circle(cx, cy, r) {
        px.fill_path(&c, &paint(rgba), FillRule::Winding, Transform::identity(), None);
    }
}
