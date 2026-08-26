//! Call-plane fetch of the pointer-centered pixel patch.

use std::time::Duration;

use base64::Engine;
use sola_call::methods::OWNER_COMPOSITOR;

use crate::grid::Patch;

const TIMEOUT: Duration = Duration::from_millis(800);

#[allow(dead_code)]
pub fn set_cursor_visible(visible: bool) -> Result<(), String> {
    sola_call::invoke(
        OWNER_COMPOSITOR,
        "cursor",
        serde_json::json!({ "visible": visible }),
        TIMEOUT,
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

pub fn fetch(size: u32) -> Result<Patch, String> {
    let data = sola_call::invoke(
        OWNER_COMPOSITOR,
        "sample",
        serde_json::json!({ "size": size }),
        TIMEOUT,
    )
    .map_err(|e| e.to_string())?;
    parse(data)
}

fn parse(data: serde_json::Value) -> Result<Patch, String> {
    let x = i32_field(&data, "x")?;
    let y = i32_field(&data, "y")?;
    let width = u32_field(&data, "width")?;
    let height = u32_field(&data, "height")?;
    let hot_x = u32_field(&data, "hot_x")?;
    let hot_y = u32_field(&data, "hot_y")?;
    let b64 = data
        .get("pixels")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing pixels".to_string())?;
    let pixels = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("pixels base64: {e}"))?;
    let expect = width as usize * height as usize * 4;
    if pixels.len() != expect {
        return Err(format!(
            "pixels length {} != {expect} ({width}×{height} RGBA)",
            pixels.len()
        ));
    }
    if hot_x >= width || hot_y >= height {
        return Err(format!(
            "hot pixel ({hot_x},{hot_y}) outside {width}×{height}"
        ));
    }
    Ok(Patch {
        x,
        y,
        width,
        height,
        hot_x,
        hot_y,
        pixels,
    })
}

fn i32_field(v: &serde_json::Value, name: &str) -> Result<i32, String> {
    v.get(name)
        .and_then(|n| n.as_i64())
        .map(|n| n as i32)
        .ok_or_else(|| format!("missing {name}"))
}

fn u32_field(v: &serde_json::Value, name: &str) -> Result<u32, String> {
    v.get(name)
        .and_then(|n| n.as_u64())
        .map(|n| n as u32)
        .ok_or_else(|| format!("missing {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_roundtrip() {
        let pixels = vec![1u8, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255];
        let b64 = base64::engine::general_purpose::STANDARD.encode(&pixels);
        let patch = parse(serde_json::json!({
            "x": 10,
            "y": 20,
            "left": 9,
            "top": 19,
            "width": 2,
            "height": 2,
            "hot_x": 1,
            "hot_y": 0,
            "pixels": b64,
        }))
        .unwrap();
        assert_eq!(patch.x, 10);
        assert_eq!(patch.hot_x, 1);
        assert_eq!(patch.pixels, pixels);
        assert_eq!(patch.hot_rgba(), Some([4, 5, 6, 255]));
    }
}
