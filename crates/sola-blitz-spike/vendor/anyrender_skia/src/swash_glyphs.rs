//! Glyph raster via cosmic-text's SwashCache (same hinter iced uses).
//! Layout/positions still come from Parley through AnyRender.

use std::collections::HashMap;
use std::sync::Arc;

use cosmic_text::fontdb::{self, Source};
use cosmic_text::{CacheKey, CacheKeyFlags, FontSystem, SwashCache, SwashContent};
use kurbo::Affine;
use skia_safe::{AlphaType, ColorType, Data, ImageInfo, Paint, SamplingOptions, images};

use crate::scene::SkiaScenePainter;

pub(crate) struct SwashGlyphs {
    font_system: FontSystem,
    swash: SwashCache,
    ids: HashMap<(u64, u32), fontdb::ID>,
}

impl SwashGlyphs {
    pub(crate) fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash: SwashCache::new(),
            ids: HashMap::new(),
        }
    }

    fn font_id(&mut self, font: &peniko::FontData) -> Option<fontdb::ID> {
        let key = (font.data.id(), font.index);
        if let Some(id) = self.ids.get(&key).copied() {
            return Some(id);
        }
        let bytes: Arc<Vec<u8>> = Arc::new(font.data.data().to_vec());
        let loaded = self
            .font_system
            .db_mut()
            .load_font_source(Source::Binary(bytes));
        let id = loaded.into_iter().find(|id| {
            self.font_system
                .db()
                .face(*id)
                .is_some_and(|face| face.index == font.index)
        })?;
        self.ids.insert(key, id);
        Some(id)
    }
}

pub(crate) fn solid_rgba(brush: &anyrender::PaintRef<'_>, brush_alpha: f32) -> [u8; 4] {
    match brush {
        anyrender::Paint::Solid(color) => {
            let rgba8 = color.to_rgba8();
            let a = ((rgba8.a as f32) * brush_alpha.clamp(0.0, 1.0)).round() as u8;
            [rgba8.r, rgba8.g, rgba8.b, a]
        }
        _ => [233, 236, 242, 255],
    }
}

impl SkiaScenePainter<'_> {
    pub(crate) fn draw_glyphs_swash(
        &mut self,
        font: &peniko::FontData,
        font_size: f32,
        hint: bool,
        rgba: [u8; 4],
        transform: Affine,
        glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = anyrender::Glyph>,
    ) -> bool {
        let Some(font_id) = self.cache.swash.font_id(font) else {
            return false;
        };

        // Raster in *device* pixels and blit with an identity matrix so
        // hinted 12px bitmaps are not bilinear-scaled by a CSS/DPI transform.
        let t = match glyph_transform {
            Some(g) => transform * g,
            None => transform,
        };
        let [a, b, c, d, e, f] = t.as_coeffs();
        let scale = (a * a + b * b).sqrt() as f32;
        let scale = if scale.is_finite() && scale > 0.01 {
            scale
        } else {
            1.0
        };
        let raster_size = font_size * scale;

        let mut flags = CacheKeyFlags::empty();
        if !hint {
            flags |= CacheKeyFlags::DISABLE_HINTING;
        }

        let mut paint = Paint::default();
        paint.set_anti_alias(false);

        self.inner.reset_matrix();

        let SwashGlyphs {
            font_system, swash, ..
        } = &mut self.cache.swash;

        for glyph in glyphs {
            let dx = a * glyph.x as f64 + c * glyph.y as f64 + e;
            let dy = b * glyph.x as f64 + d * glyph.y as f64 + f;
            let (key, px, py) = CacheKey::new(
                font_id,
                glyph.id as u16,
                raster_size,
                (dx as f32, dy as f32),
                fontdb::Weight::NORMAL,
                flags,
            );
            let Some(image) = swash.get_image(font_system, key).clone() else {
                continue;
            };
            if image.placement.width == 0 || image.placement.height == 0 {
                continue;
            }
            blit_glyph(self.inner, &mut paint, &image, px, py, rgba);
        }
        true
    }
}

fn blit_glyph(
    canvas: &skia_safe::Canvas,
    paint: &mut Paint,
    image: &cosmic_text::SwashImage,
    origin_x: i32,
    origin_y: i32,
    rgba: [u8; 4],
) {
    let w = image.placement.width as usize;
    let h = image.placement.height as usize;
    let mut pixels = vec![0u8; w * h * 4];
    match image.content {
        SwashContent::Mask => {
            for (i, cov) in image.data.iter().enumerate() {
                let a = ((*cov as u16 * rgba[3] as u16) / 255) as u8;
                let o = i * 4;
                pixels[o] = ((rgba[0] as u16 * a as u16) / 255) as u8;
                pixels[o + 1] = ((rgba[1] as u16 * a as u16) / 255) as u8;
                pixels[o + 2] = ((rgba[2] as u16 * a as u16) / 255) as u8;
                pixels[o + 3] = a;
            }
        }
        SwashContent::Color => {
            if image.data.len() >= w * h * 4 {
                pixels.copy_from_slice(&image.data[..w * h * 4]);
            }
        }
        SwashContent::SubpixelMask => {
            // cosmic-text 0.15 SwashCache emits Mask (Format::Alpha), not this.
            for (i, cov) in image.data.iter().step_by(3).enumerate() {
                if i >= w * h {
                    break;
                }
                let a = ((*cov as u16 * rgba[3] as u16) / 255) as u8;
                let o = i * 4;
                pixels[o] = rgba[0];
                pixels[o + 1] = rgba[1];
                pixels[o + 2] = rgba[2];
                pixels[o + 3] = a;
            }
        }
    }

    let info = ImageInfo::new(
        (w as i32, h as i32),
        ColorType::RGBA8888,
        AlphaType::Premul,
        None,
    );
    let Some(sk_image) = images::raster_from_data(&info, Data::new_copy(&pixels), w * 4) else {
        return;
    };
    let x = origin_x + image.placement.left;
    let y = origin_y - image.placement.top;
    canvas.draw_image_with_sampling_options(
        sk_image,
        (x as f32, y as f32),
        SamplingOptions::default(),
        Some(paint),
    );
}
