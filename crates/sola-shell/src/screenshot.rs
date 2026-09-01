//! Shell-initiated capture via `compositor.screenshot` (not the bus).
//!
//! Super+Shift+3/4/5 advertise `image/png` immediately, then Fastest-encode
//! in the background (shell owns the offer). Paste waits on the pipe.
//! No file, no Preview. `solactl compositor screenshot` still writes a PNG.

use std::fs;
use std::time::Duration;

use iced::widget::image;
use sola_call::methods::OWNER_COMPOSITOR;

use crate::app::{FreezeImage, Msg};

const CAPTURE_TIMEOUT: Duration = Duration::from_secs(8);

pub fn full() -> iced::Task<Msg> {
    copy_capture(serde_json::json!({ "format": "rgba" }))
}

pub fn window(app_id: String, title: Option<String>) -> iced::Task<Msg> {
    let mut params = serde_json::json!({ "app": app_id, "format": "rgba" });
    if let Some(t) = title {
        params["window"] = serde_json::Value::String(t);
    }
    copy_capture(params)
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

/// Crop a freeze handle to `region` and copy a Fast PNG. No second screencopy.
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

fn copy_capture(params: serde_json::Value) -> iced::Task<Msg> {
    iced::Task::perform(
        async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                let r = copy_capture_sync(params);
                let _ = tx.send(r);
            });
            match rx.await {
                Ok(r) => r,
                Err(_) => Err("screenshot: worker dropped".into()),
            }
        },
        Msg::ScreenshotDone,
    )
}

/// Advertise `image/png` first so Slack ⌘V can open a pipe, then capture.
/// Encode runs in the background; paste waits on the pipe until it lands.
fn copy_capture_sync(params: serde_json::Value) -> Result<(), String> {
    let offer = sola_kit::clipboard::offer_png().map_err(|e| e.to_string())?;
    let (w, h, rgba) = match capture_rgba(params) {
        Ok(v) => v,
        Err(e) => {
            offer.fail();
            return Err(e);
        }
    };
    fulfill_png(offer, w, h, rgba);
    Ok(())
}

fn fulfill_png(offer: sola_kit::clipboard::PngOffer, width: u32, height: u32, rgba: Vec<u8>) {
    let _ = std::thread::Builder::new()
        .name("sola-clip-encode".into())
        .spawn(move || {
            let t0 = std::time::Instant::now();
            match sola_kit::clipboard::encode_png_fast(width, height, &rgba) {
                Ok(png) => {
                    tracing::info!(
                        width,
                        height,
                        png_bytes = png.len(),
                        encode_ms = t0.elapsed().as_millis(),
                        "screenshot png ready"
                    );
                    offer.fulfill(png);
                }
                Err(e) => {
                    tracing::warn!(%e, "screenshot png encode failed");
                    offer.fail();
                }
            }
        });
}

fn capture_freeze() -> Result<FreezeImage, String> {
    let (width, height, pixels) = capture_rgba(serde_json::json!({ "format": "rgba" }))?;
    Ok(FreezeImage {
        handle: image::Handle::from_rgba(width, height, pixels),
        width,
        height,
    })
}

fn capture_rgba(params: serde_json::Value) -> Result<(u32, u32, Vec<u8>), String> {
    let v = sola_call::invoke(OWNER_COMPOSITOR, "screenshot", params, CAPTURE_TIMEOUT)
        .map_err(|e| e.to_string())?;
    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| "screenshot: no path in freeze reply".to_string())?;
    let width = v
        .get("width")
        .and_then(|w| w.as_u64())
        .ok_or_else(|| "screenshot: no width in freeze reply".to_string())? as u32;
    let height =
        v.get("height")
            .and_then(|h| h.as_u64())
            .ok_or_else(|| "screenshot: no height in freeze reply".to_string())? as u32;
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
    Ok((width, height, pixels))
}

fn crop_freeze_sync(
    handle: &image::Handle,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<(), String> {
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
    let offer = sola_kit::clipboard::offer_png().map_err(|e| e.to_string())?;
    fulfill_png(offer, w, h, rgba);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_rgba_extracts_center() {
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
