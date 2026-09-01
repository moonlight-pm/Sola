//! Compositor clipboard via `wlr-data-control` / `ext-data-control`.
//!
//! Iced's clipboard is text-only (`smithay-clipboard`). Image copy/paste
//! has to talk to the compositor itself. This module offers and reads
//! `image/png` (and sibling image MIMEs) without going through iced, so
//! a smithay receive cannot drop the offer.
//!
//! Serving uses a **thread** (not `fork`). Screenshot hotkeys advertise
//! `image/png` immediately and fill the pipe when paste arrives. See
//! `docs/specs/2026-09-01-image-clipboard-design.md`.

mod pending;

use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

pub use pending::{PngOffer, offer_png};

use wl_clipboard_rs::copy::{self, MimeType as CopyMime, Source};
use wl_clipboard_rs::paste::{
    self, ClipboardType, Error as PasteError, MimeType as PasteMime, Seat,
};

/// Compressed payload cap (screenshots, not raw RGBA dumps).
pub const MAX_BYTES: usize = 32 * 1024 * 1024;

/// Image MIMEs we will offer or prefer on paste, in priority order.
pub const IMAGE_MIMES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/jpg",
    "image/gif",
    "image/webp",
    "image/bmp",
];

/// What is currently on the compositor clipboard.
#[derive(Debug, Clone)]
pub enum Offer {
    Empty,
    Text(String),
    Image {
        mime: String,
        bytes: Arc<[u8]>,
        filename: String,
    },
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Wayland(String),
    Encode(String),
    NotImage,
    TooLarge { bytes: usize },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Wayland(s) => write!(f, "{s}"),
            Error::Encode(s) => write!(f, "{s}"),
            Error::NotImage => write!(f, "not an image"),
            Error::TooLarge { bytes } => write!(f, "image too large ({bytes} bytes)"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Encode tightly-packed RGBA8 as PNG with [`png::Compression::Fastest`].
///
/// `Fast` still picks Adaptive filters — ~2s on a 5K still in debug.
/// `Fastest` is fdeflate + Up filter (tens–hundreds of ms).
pub fn encode_png_fast(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, Error> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| Error::Encode("png size overflow".into()))?;
    if rgba.len() != expected {
        return Err(Error::Encode(format!(
            "rgba size mismatch: got {}, expected {expected} ({width}×{height})",
            rgba.len()
        )));
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fastest);
        let mut writer = encoder
            .write_header()
            .map_err(|e| Error::Encode(format!("png header: {e}")))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| Error::Encode(format!("png write: {e}")))?;
        writer
            .finish()
            .map_err(|e| Error::Encode(format!("png finish: {e}")))?;
    }
    if out.len() > MAX_BYTES {
        return Err(Error::TooLarge { bytes: out.len() });
    }
    tracing::debug!(width, height, png_bytes = out.len(), "png fastest encoded");
    Ok(out)
}

/// Fast-encode RGBA and offer it as `image/png`.
pub fn write_png(width: u32, height: u32, rgba: Vec<u8>) -> Result<(), Error> {
    let t0 = Instant::now();
    let bytes = encode_png_fast(width, height, &rgba)?;
    tracing::info!(
        width,
        height,
        png_bytes = bytes.len(),
        encode_ms = t0.elapsed().as_millis(),
        "clipboard png encoded"
    );
    write_image("image/png", bytes)
}

/// Offer `bytes` as `mime` on the regular clipboard. Replaces the current
/// selection. Returns once the source is advertised; a helper thread
/// serves paste requests until something else copies.
pub fn write_image(mime: &str, bytes: Vec<u8>) -> Result<(), Error> {
    if bytes.len() > MAX_BYTES {
        return Err(Error::TooLarge { bytes: bytes.len() });
    }
    if !is_image_mime(mime) {
        return Err(Error::NotImage);
    }
    let opts = copy::Options::new();
    // Default `foreground(false)` serves on a thread (not fork).
    opts.copy(
        Source::Bytes(bytes.into_boxed_slice()),
        CopyMime::Specific(mime.to_string()),
    )
    .map_err(|e| Error::Wayland(e.to_string()))
}

/// Read a file and offer it as an image. Sniffs magic, then extension.
pub fn write_image_path(path: &Path) -> Result<(), Error> {
    let bytes = std::fs::read(path)?;
    if bytes.len() > MAX_BYTES {
        return Err(Error::TooLarge { bytes: bytes.len() });
    }
    let mime = sniff_image_mime(&bytes)
        .or_else(|| mime_from_ext(path))
        .ok_or(Error::NotImage)?;
    write_image(mime, bytes)
}

/// Read the current clipboard. Prefers an image MIME when both text and
/// image are offered (screenshot / Preview Copy).
pub fn read_offer() -> Offer {
    let types = match paste::get_mime_types(ClipboardType::Regular, Seat::Unspecified) {
        Ok(t) => t,
        Err(_) => return Offer::Empty,
    };
    if let Some(mime) = pick_image_mime(&types) {
        if let Some(offer) = read_image(mime) {
            return offer;
        }
    }
    read_text().unwrap_or(Offer::Empty)
}

fn read_image(mime: &str) -> Option<Offer> {
    match paste::get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        PasteMime::Specific(mime),
    ) {
        Ok((mut pipe, actual)) => {
            let mut buf = Vec::new();
            if pipe.read_to_end(&mut buf).is_err() {
                return None;
            }
            if buf.is_empty() || buf.len() > MAX_BYTES {
                return None;
            }
            let mime = if actual.is_empty() {
                mime.to_string()
            } else {
                actual
            };
            let filename = filename_for_mime(&mime);
            Some(Offer::Image {
                mime,
                bytes: Arc::from(buf),
                filename,
            })
        }
        Err(PasteError::ClipboardEmpty | PasteError::NoMimeType) => None,
        Err(e) => {
            tracing::debug!(%e, mime, "clipboard image read failed");
            None
        }
    }
}

fn read_text() -> Option<Offer> {
    match paste::get_contents(ClipboardType::Regular, Seat::Unspecified, PasteMime::Text) {
        Ok((mut pipe, _)) => {
            let mut buf = Vec::new();
            if pipe.read_to_end(&mut buf).is_err() {
                return None;
            }
            let s = String::from_utf8_lossy(&buf).into_owned();
            if s.is_empty() {
                None
            } else {
                Some(Offer::Text(s))
            }
        }
        Err(_) => None,
    }
}

pub fn is_image_mime(mime: &str) -> bool {
    IMAGE_MIMES.iter().any(|m| mime.eq_ignore_ascii_case(m))
}

pub fn pick_image_mime<'a>(types: impl IntoIterator<Item = &'a String>) -> Option<&'static str> {
    let offered: Vec<String> = types.into_iter().map(|s| s.to_ascii_lowercase()).collect();
    IMAGE_MIMES
        .iter()
        .copied()
        .find(|m| offered.iter().any(|o| o == m))
}

pub fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 8 && bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    None
}

pub fn mime_from_ext(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => Some("image/png"),
        Some("jpg") | Some("jpeg") => Some("image/jpeg"),
        Some("gif") => Some("image/gif"),
        Some("webp") => Some("image/webp"),
        Some("bmp") => Some("image/bmp"),
        _ => None,
    }
}

pub fn filename_for_mime(mime: &str) -> String {
    match mime.to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "clipboard.jpg".into(),
        "image/gif" => "clipboard.gif".into(),
        "image/webp" => "clipboard.webp".into(),
        "image/bmp" => "clipboard.bmp".into(),
        _ => "clipboard.png".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn sniffs_png_magic() {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        b.extend_from_slice(&[0u8; 8]);
        assert_eq!(sniff_image_mime(&b), Some("image/png"));
    }

    #[test]
    fn sniffs_jpeg_magic() {
        assert_eq!(
            sniff_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some("image/jpeg")
        );
    }

    #[test]
    fn sniffs_webp_magic() {
        let mut b = b"RIFF".to_vec();
        b.extend_from_slice(&[0, 0, 0, 0]);
        b.extend_from_slice(b"WEBP");
        assert_eq!(sniff_image_mime(&b), Some("image/webp"));
    }

    #[test]
    fn ext_fallback() {
        assert_eq!(mime_from_ext(Path::new("shot.PNG")), Some("image/png"));
        assert_eq!(mime_from_ext(Path::new("x.jpeg")), Some("image/jpeg"));
        assert_eq!(mime_from_ext(Path::new("notes.txt")), None);
    }

    #[test]
    fn pick_prefers_png() {
        let types = vec!["text/plain".into(), "image/jpeg".into(), "image/png".into()];
        assert_eq!(pick_image_mime(&types), Some("image/png"));
    }

    #[test]
    fn filename_matches_mime() {
        assert_eq!(filename_for_mime("image/png"), "clipboard.png");
        assert_eq!(filename_for_mime("image/jpeg"), "clipboard.jpg");
    }

    #[test]
    fn encode_png_fast_writes_magic() {
        let px = [0x11u8, 0x22, 0x33, 0xFF];
        let png = encode_png_fast(1, 1, &px).unwrap();
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]));
        assert!(png.len() > 8);
        assert!(png.len() < MAX_BYTES);
    }

    #[test]
    fn encode_png_fast_rejects_bad_len() {
        let err = encode_png_fast(2, 2, &[1, 2, 3]).unwrap_err();
        assert!(matches!(err, Error::Encode(_)));
    }

    #[test]
    fn write_rejects_non_image_mime() {
        let err = write_image("text/plain", b"hi".to_vec()).unwrap_err();
        assert!(matches!(err, Error::NotImage));
    }

    #[test]
    fn write_rejects_oversize() {
        let err = write_image("image/png", vec![0; MAX_BYTES + 1]).unwrap_err();
        assert!(matches!(err, Error::TooLarge { .. }));
    }

    #[test]
    fn write_path_missing_file() {
        let err = write_image_path(&PathBuf::from("/no/such/preview-clip.png")).unwrap_err();
        assert!(matches!(err, Error::Io(_)));
    }
}
