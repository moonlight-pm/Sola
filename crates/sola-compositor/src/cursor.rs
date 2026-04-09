/// Cursor loading and management.
///
/// Loads the default cursor from the system's xcursor theme and provides
/// it as a `MemoryRenderBuffer` that can be rendered at the pointer position.
///
/// See: https://docs.rs/xcursor/0.3
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::utils::Transform;
use xcursor::parser::Image;

/// The target cursor size in pixels. We pick the xcursor image closest
/// to this size from the theme.
const TARGET_SIZE: u32 = 24;

/// Load the default cursor from the system xcursor theme.
///
/// Tries the "Adwaita" theme first, then falls back to "default".
/// Returns the render buffer and the hotspot offset (x, y).
pub fn load_default() -> Option<(MemoryRenderBuffer, (i32, i32))> {
    let theme = xcursor::CursorTheme::load("Adwaita");
    let cursor_path = theme.load_icon("default").or_else(|| {
        let fallback = xcursor::CursorTheme::load("default");
        fallback.load_icon("default")
    })?;

    let cursor_data = std::fs::read(&cursor_path).ok()?;
    let images = xcursor::parser::parse_xcursor(&cursor_data)?;

    let image = pick_best_size(&images, TARGET_SIZE)?;

    let width = image.width as i32;
    let height = image.height as i32;
    let hotspot = (image.xhot as i32, image.yhot as i32);

    // xcursor's `pixels_rgba` is byte-order [R, G, B, A].
    // DRM fourcc `Abgr8888` means the 32-bit word is 0xAABBGGRR,
    // which in little-endian memory is [R, G, B, A] — matching exactly.
    let buffer = MemoryRenderBuffer::from_slice(
        &image.pixels_rgba,
        drm_fourcc::DrmFourcc::Abgr8888,
        (width, height),
        1,
        Transform::Normal,
        None,
    );

    tracing::info!(
        %width, %height,
        hotspot_x = hotspot.0, hotspot_y = hotspot.1,
        "loaded cursor from xcursor theme"
    );

    Some((buffer, hotspot))
}

/// Pick the image closest to the target size from a list of xcursor images.
fn pick_best_size(images: &[Image], target: u32) -> Option<&Image> {
    images
        .iter()
        .min_by_key(|img| (img.size as i32 - target as i32).unsigned_abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_image(size: u32) -> Image {
        let pixel_count = (size * size) as usize;
        Image {
            size,
            width: size,
            height: size,
            xhot: 0,
            yhot: 0,
            delay: 0,
            pixels_rgba: vec![0u8; pixel_count * 4],
            pixels_argb: vec![0u8; pixel_count * 4],
        }
    }

    #[test]
    fn picks_exact_match() {
        let images = vec![make_image(16), make_image(24), make_image(32)];
        let best = pick_best_size(&images, 24).unwrap();
        assert_eq!(best.size, 24);
    }

    #[test]
    fn picks_closest_when_no_exact() {
        let images = vec![make_image(16), make_image(32), make_image(48)];
        let best = pick_best_size(&images, 24).unwrap();
        // 16 is 8 away, 32 is 8 away — either is valid, but min_by_key
        // returns the first match, which is 16.
        assert!(best.size == 16 || best.size == 32);
    }

    #[test]
    fn picks_only_available() {
        let images = vec![make_image(48)];
        let best = pick_best_size(&images, 24).unwrap();
        assert_eq!(best.size, 48);
    }

    #[test]
    fn empty_returns_none() {
        let images: Vec<Image> = vec![];
        assert!(pick_best_size(&images, 24).is_none());
    }
}
