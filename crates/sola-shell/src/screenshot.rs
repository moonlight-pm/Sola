//! Shell-initiated capture via `compositor.screenshot` (not the bus).

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iced::widget::image;
use sola_call::methods::OWNER_COMPOSITOR;

use crate::app::{FreezeImage, Msg};

pub fn full() -> iced::Task<Msg> {
    invoke(serde_json::json!({}))
}

pub fn window(app_id: String, title: Option<String>) -> iced::Task<Msg> {
    let mut params = serde_json::json!({ "app": app_id });
    if let Some(t) = title {
        params["window"] = serde_json::Value::String(t);
    }
    invoke(params)
}

/// Full-output RGBA freeze (no PNG). Overlay stays hidden until this returns.
pub fn freeze(generation: u64) -> iced::Task<Msg> {
    iced::Task::perform(
        async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                let r = capture_freeze();
                let _ = tx.send(r);
            });
            match rx.await {
                Ok(r) => (generation, r),
                Err(_) => (generation, Err("screenshot: freeze worker dropped".into())),
            }
        },
        |(generation, result)| Msg::SelectionFreeze { generation, result },
    )
}

/// Crop a freeze handle to `region` and write a PNG. No second screencopy.
pub fn crop_freeze(
    handle: image::Handle,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> iced::Task<Msg> {
    iced::Task::perform(
        async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                let r = crop_freeze_sync(&handle, x, y, width, height);
                let _ = tx.send(r);
            });
            match rx.await {
                Ok(r) => r,
                Err(_) => Err("screenshot: crop worker dropped".into()),
            }
        },
        Msg::ScreenshotDone,
    )
}

fn capture_freeze() -> Result<FreezeImage, String> {
    let v = sola_call::invoke(
        OWNER_COMPOSITOR,
        "screenshot",
        serde_json::json!({ "format": "rgba" }),
        Duration::from_secs(8),
    )
    .map_err(|e| e.to_string())?;
    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| "screenshot: no path in freeze reply".to_string())?;
    let width = v
        .get("width")
        .and_then(|w| w.as_u64())
        .ok_or_else(|| "screenshot: no width in freeze reply".to_string())?
        as u32;
    let height = v
        .get("height")
        .and_then(|h| h.as_u64())
        .ok_or_else(|| "screenshot: no height in freeze reply".to_string())?
        as u32;
    let pixels = fs::read(path).map_err(|e| format!("read freeze {path}: {e}"))?;
    let _ = fs::remove_file(path);
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| "screenshot: freeze size overflow".to_string())?;
    if pixels.len() != expected {
        return Err(format!(
            "screenshot: freeze size mismatch: got {} bytes, expected {expected} ({width}×{height})",
            pixels.len()
        ));
    }
    Ok(FreezeImage {
        handle: image::Handle::from_rgba(width, height, pixels),
        width,
        height,
    })
}

fn crop_freeze_sync(
    handle: &image::Handle,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<PathBuf, String> {
    let image::Handle::Rgba {
        width: src_w,
        height: src_h,
        pixels,
        ..
    } = handle
    else {
        return Err("screenshot: freeze is not RGBA".into());
    };
    let (w, h, rgba) = crop_rgba(pixels.as_ref(), *src_w, *src_h, x, y, width, height)?;
    let path = default_png_path()?;
    write_png(&path, w, h, &rgba)?;
    Ok(path)
}

fn default_png_path() -> Result<PathBuf, String> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = PathBuf::from(format!("/tmp/sola/screenshots/{ms}.png"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create_dir_all {}: {e}", parent.display()))?;
    }
    Ok(path)
}

fn write_png(path: &PathBuf, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("png header: {e}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| format!("png write: {e}"))?;
    Ok(())
}

/// Tightly-packed RGBA8 crop, clamped to the source.
pub fn crop_rgba(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<(u32, u32, Vec<u8>), String> {
    if src_w == 0 || src_h == 0 {
        return Err("empty freeze".into());
    }
    let src_w_i = src_w as i32;
    let src_h_i = src_h as i32;
    let x2 = x.saturating_add(width);
    let y2 = y.saturating_add(height);
    let left = x.min(x2).clamp(0, src_w_i);
    let right = x.max(x2).clamp(0, src_w_i);
    let top = y.min(y2).clamp(0, src_h_i);
    let bottom = y.max(y2).clamp(0, src_h_i);
    let w = (right - left).max(0) as u32;
    let h = (bottom - top).max(0) as u32;
    if w == 0 || h == 0 {
        return Err("crop empty after clamp".into());
    }
    let sw = src_w as usize;
    let dw = w as usize;
    let expected = sw
        .checked_mul(src_h as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| "freeze size overflow".to_string())?;
    if src.len() < expected {
        return Err(format!(
            "freeze buffer too small: have {}, need {expected}",
            src.len()
        ));
    }
    let mut out = vec![0u8; dw * h as usize * 4];
    for row in 0..h as usize {
        let src_off = ((top as usize + row) * sw + left as usize) * 4;
        let dst_off = row * dw * 4;
        out[dst_off..dst_off + dw * 4].copy_from_slice(&src[src_off..src_off + dw * 4]);
    }
    Ok((w, h, out))
}

fn invoke(params: serde_json::Value) -> iced::Task<Msg> {
    iced::Task::perform(
        async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                let r = sola_call::invoke(
                    OWNER_COMPOSITOR,
                    "screenshot",
                    params,
                    Duration::from_secs(20),
                );
                let _ = tx.send(r);
            });
            match rx.await {
                Ok(Ok(v)) => v
                    .get("path")
                    .and_then(|p| p.as_str())
                    .map(PathBuf::from)
                    .ok_or_else(|| "screenshot: no path in reply".into()),
                Ok(Err(e)) => Err(e.to_string()),
                Err(_) => Err("screenshot: worker dropped".into()),
            }
        },
        Msg::ScreenshotDone,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_rgba_extracts_center() {
        // 3×2, each pixel unique RGB
        let mut src = vec![0u8; 3 * 2 * 4];
        for i in 0..6 {
            let o = i * 4;
            src[o] = i as u8;
            src[o + 1] = 10;
            src[o + 2] = 20;
            src[o + 3] = 255;
        }
        let (w, h, out) = crop_rgba(&src, 3, 2, 1, 0, 2, 1).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(&out[0..4], &[1, 10, 20, 255]);
        assert_eq!(&out[4..8], &[2, 10, 20, 255]);
    }

    #[test]
    fn crop_rgba_clamps() {
        let src = vec![9u8; 4 * 4 * 4];
        let (w, h, out) = crop_rgba(&src, 4, 4, -2, -2, 3, 3).unwrap();
        assert_eq!((w, h), (1, 1));
        assert_eq!(out.len(), 4);
    }
}
