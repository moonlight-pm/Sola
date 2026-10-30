//! The desk's background picture, behind the lock: the path the bus keeps in
//! `state.yaml` (`Topic::Background`), decoded once on a thread and cut to
//! cover each output.

use std::path::PathBuf;

use image::RgbaImage;
use image::imageops::FilterType;

/// The picture's path, if one is set.
pub fn path() -> Option<PathBuf> {
    let state = sola_core::config::sola_config_dir().join("state.yaml");
    let text = std::fs::read_to_string(state).ok()?;
    let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).ok()?;
    let path = doc.get("Background")?.get("path")?.as_str()?;
    Some(PathBuf::from(path))
}

/// The most a picture keeps on its long side once decoded: enough for a
/// 4K output, and each output's cut stays quick.
const KEEP: u32 = 3840;

/// Decode the picture (slow for a large photo: run it off the event loop).
pub fn load() -> Option<RgbaImage> {
    let path = path()?;
    match image::open(&path) {
        Ok(img) if img.width().max(img.height()) > KEEP => {
            Some(img.resize(KEEP, KEEP, FilterType::Triangle).to_rgba8())
        }
        Ok(img) => Some(img.to_rgba8()),
        Err(e) => {
            tracing::warn!(path = %path.display(), "background picture: {e}");
            None
        }
    }
}

/// `img` scaled to cover `w`×`h` and cut to it, centered.
pub fn cover(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let (iw, ih) = img.dimensions();
    let scale = (w as f64 / iw as f64).max(h as f64 / ih as f64);
    let sw = ((iw as f64 * scale).ceil() as u32).max(w);
    let sh = ((ih as f64 * scale).ceil() as u32).max(h);
    let scaled = image::imageops::resize(img, sw, sh, FilterType::Triangle);
    image::imageops::crop_imm(&scaled, (sw - w) / 2, (sh - h) / 2, w, h).to_image()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_picture_covers_a_tall_screen() {
        let img = RgbaImage::from_pixel(400, 100, image::Rgba([10, 20, 30, 255]));
        let out = cover(&img, 100, 200);
        assert_eq!(out.dimensions(), (100, 200));
        assert_eq!(out.get_pixel(50, 100).0, [10, 20, 30, 255]);
    }
}
