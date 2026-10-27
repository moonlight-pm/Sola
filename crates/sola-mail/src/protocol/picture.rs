//! Letter pictures, decoded off the UI thread with bounds: a picture too
//! large to fetch or decode is left out, a wide one is scaled down.

use std::io::Cursor;

use image::{ImageReader, Limits};

use super::types::LetterImage;

/// Bytes past this are not a letter's picture.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;
/// Pixels past this (either side) are refused before decoding.
const MAX_DECODE_SIDE: u32 = 8000;
/// Pictures wider than this are scaled down to it (the reading column
/// is far narrower; this keeps memory per letter small).
const MAX_SIDE: u32 = 1600;

pub fn decode(key: String, bytes: &[u8]) -> Option<LetterImage> {
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let mut img = reader.decode().ok()?;
    if img.width() > MAX_SIDE {
        img = img.resize(MAX_SIDE, u32::MAX, image::imageops::FilterType::Triangle);
    }
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let plate = wants_plate(rgba.as_raw());
    Some(LetterImage {
        key,
        width,
        height,
        rgba: rgba.into_raw(),
        plate,
    })
}

/// A picture made for white mail: at least a tenth transparent, and dark
/// (on average) where it is not — a logo in ink that a dark letter would
/// swallow. Every third pixel is enough to tell.
fn wants_plate(rgba: &[u8]) -> bool {
    let (mut seen, mut clear) = (0u32, 0u32);
    let (mut light, mut weight) = (0.0f32, 0.0f32);
    for px in rgba.chunks_exact(4).step_by(3) {
        seen += 1;
        let a = px[3] as f32 / 255.0;
        if px[3] < 250 {
            clear += 1;
        }
        if a > 0.1 {
            let l = (0.2126 * px[0] as f32 + 0.7152 * px[1] as f32 + 0.0722 * px[2] as f32) / 255.0;
            light += l * a;
            weight += a;
        }
    }
    clear * 10 >= seen && weight > 0.0 && light / weight < 0.4
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn decodes_and_scales_wide_pictures_down() {
        let small = decode("a".into(), &png(40, 20)).unwrap();
        assert_eq!((small.width, small.height), (40, 20));
        assert_eq!(small.rgba.len(), 40 * 20 * 4);
        let wide = decode("b".into(), &png(3200, 400)).unwrap();
        assert_eq!((wide.width, wide.height), (1600, 200));
    }

    #[test]
    fn dark_ink_on_clear_wants_a_plate() {
        let mut ink = image::RgbaImage::from_pixel(20, 20, image::Rgba([0, 0, 0, 0]));
        for x in 0..20 {
            ink.put_pixel(x, 10, image::Rgba([30, 40, 60, 255]));
        }
        assert!(wants_plate(ink.as_raw()));
        let mut white = ink.clone();
        for x in 0..20 {
            white.put_pixel(x, 10, image::Rgba([250, 250, 250, 255]));
        }
        assert!(!wants_plate(white.as_raw()));
        let opaque = image::RgbaImage::from_pixel(20, 20, image::Rgba([0, 0, 0, 255]));
        assert!(!wants_plate(opaque.as_raw()));
    }

    #[test]
    fn junk_is_no_picture() {
        assert!(decode("c".into(), b"not a picture").is_none());
    }
}
