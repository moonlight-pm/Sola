/// Desktop wallpaper loading.
///
/// Decodes an embedded JPEG wallpaper into a `MemoryRenderBuffer` that can
/// be composited behind all windows.
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::utils::Transform;

/// The wallpaper image, embedded at compile time.
const WALLPAPER_BYTES: &[u8] = include_bytes!("../../../assets/wallpaper.jpg");

/// Load the embedded wallpaper and return it as a render buffer.
pub fn load() -> Option<MemoryRenderBuffer> {
    let img = image::load_from_memory(WALLPAPER_BYTES)
        .map_err(|e| tracing::error!("failed to decode wallpaper: {e}"))
        .ok()?;

    let rgba = img.to_rgba8();
    let (width, height) = (rgba.width() as i32, rgba.height() as i32);

    // RGBA8 in memory is [R, G, B, A] per pixel.
    // DRM fourcc Abgr8888 = 0xAABBGGRR in a 32-bit word = [R, G, B, A] in
    // little-endian memory — matches exactly.
    let buffer = MemoryRenderBuffer::from_slice(
        rgba.as_raw(),
        drm_fourcc::DrmFourcc::Abgr8888,
        (width, height),
        1,
        Transform::Normal,
        None,
    );

    tracing::info!(width, height, "loaded wallpaper");
    Some(buffer)
}
