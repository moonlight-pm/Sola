//! Use as Desktop Background (the page menu on a picture): find the
//! largest version of the picture, choose the part that fills the screen
//! when its shape differs, save it under `~/Pictures/Backgrounds/`, and
//! make it the desktop's background (`sola_core::background` keeps Sola's
//! own copy; the caller sends `Topic::Background`). The window side is
//! `app/wallpaper.rs`. Spec: `docs/specs/2026-10-24-background-design.md`
//! § From the browser.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine as _;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader};

/// A picture larger than this is not fetched.
const MAX_BYTES: u64 = 150 * 1024 * 1024;
/// Decoding may take this much memory (a 14k × 8k telescope picture fits).
const MAX_ALLOC: u64 = 1 << 30;
/// The picture shown while choosing, at most this on its longer side.
pub const PREVIEW_MAX: u32 = 2400;
/// A crop is saved at this JPEG quality.
const QUALITY: u8 = 92;
/// Shapes this close (as a ratio of aspect ratios) need no choosing.
const SAME_SHAPE: f32 = 0.01;
/// How small the chosen part may get: this much of the largest.
const MIN_ZOOM: f32 = 0.2;
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0 Safari/537.36";

/// The lightbox is up (process-wide, for the key subscription).
static OPEN: AtomicBool = AtomicBool::new(false);

pub fn is_open() -> bool {
    OPEN.load(Ordering::Relaxed)
}

pub fn set_open(open: bool) {
    OPEN.store(open, Ordering::Relaxed);
}

/// Where the picture may be found whole, best first: the link it sits in
/// (often the full-size file), Wikimedia's original behind a thumbnail,
/// then the picture itself. Each is fetched; the most pixels win.
pub fn candidates(src: &str, link: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |u: String| {
        if !u.is_empty() && !out.contains(&u) {
            out.push(u);
        }
    };
    if let Some(link) = link.filter(|l| l.starts_with("http")) {
        if let Some(original) = wikimedia_original(link) {
            push(original);
        }
        push(link.to_string());
    }
    if let Some(original) = wikimedia_original(src) {
        push(original);
    }
    push(src.to_string());
    out
}

/// Wikimedia serves a thumbnail at
/// `https://{upload,thumb}.wikimedia.org/wikipedia/commons/thumb/a/ab/Name.jpg/640px-Name.jpg?…`;
/// its original is `https://upload.wikimedia.org/wikipedia/commons/a/ab/Name.jpg`.
pub fn wikimedia_original(url: &str) -> Option<String> {
    let url = url.split(['?', '#']).next()?;
    let rest = url
        .strip_prefix("https://upload.wikimedia.org/")
        .or_else(|| url.strip_prefix("https://thumb.wikimedia.org/"))?;
    let i = rest.find("/thumb/")?;
    let after = &rest[i + "/thumb/".len()..];
    let original = &after[..after.rfind('/')?];
    // a/ab/Name.jpg: the hash folders and the file.
    (original.split('/').count() >= 3)
        .then(|| format!("https://upload.wikimedia.org/{}/{original}", &rest[..i]))
}

/// A picture as fetched: its bytes and how big it is.
pub struct Found {
    pub url: String,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: Option<ImageFormat>,
}

impl Found {
    fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
}

/// Fetch each candidate (the page as referrer) and keep the largest that
/// is a picture.
pub async fn find(candidates: Vec<String>, referer: String) -> Result<Found, String> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| format!("Could not start a download: {e}"))?;
    let mut best: Option<Found> = None;
    let mut last_err = None;
    for url in candidates {
        match fetch(&client, &url, &referer).await {
            Ok(found) => {
                tracing::info!(%url, w = found.width, h = found.height, "desktop background candidate");
                if best.as_ref().is_none_or(|b| found.pixels() > b.pixels()) {
                    best = Some(found);
                }
            }
            Err(e) => {
                tracing::info!(%url, error = %e, "desktop background candidate skipped");
                last_err = Some(e);
            }
        }
    }
    best.ok_or_else(|| last_err.unwrap_or_else(|| "There is no picture here.".into()))
}

async fn fetch(client: &reqwest::Client, url: &str, referer: &str) -> Result<Found, String> {
    let bytes = if let Some(data) = url.strip_prefix("data:") {
        data_url_bytes(data)?
    } else if url.starts_with("http://") || url.starts_with("https://") {
        let mut req = client.get(url);
        if !referer.is_empty() {
            req = req.header(reqwest::header::REFERER, referer);
        }
        let resp = req
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?;
        let kind = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if kind.starts_with("text/") {
            return Err(format!("not a picture ({kind})"));
        }
        if resp.content_length().is_some_and(|n| n > MAX_BYTES) {
            return Err("too large".into());
        }
        let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("too large".into());
        }
        bytes.to_vec()
    } else {
        return Err("This picture can't be downloaded.".into());
    };
    let reader = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let format = reader.format();
    let (width, height) = reader.into_dimensions().map_err(|e| e.to_string())?;
    Ok(Found {
        url: url.to_string(),
        bytes,
        width,
        height,
        format,
    })
}

/// `image/png;base64,…` (after `data:`).
fn data_url_bytes(data: &str) -> Result<Vec<u8>, String> {
    let (head, body) = data.split_once(',').ok_or("not a data URL")?;
    if !head.ends_with(";base64") {
        return Err("not a base64 picture".into());
    }
    base64::engine::general_purpose::STANDARD
        .decode(body.trim())
        .map_err(|e| e.to_string())
}

/// A picture decoded for choosing and saving.
pub struct Picture {
    pub image: DynamicImage,
    pub width: u32,
    pub height: u32,
    /// The bytes as fetched (saved as they are when no crop is needed).
    pub bytes: Vec<u8>,
    pub format: Option<ImageFormat>,
    /// A file name for it, from its URL.
    pub name: String,
    /// The preview, RGBA, at most [`PREVIEW_MAX`] on a side.
    pub preview: (u32, u32, Vec<u8>),
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Picture")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Decode `found` (blocking: run it off the UI thread).
pub fn decode(found: Found) -> Result<Picture, String> {
    let mut reader = ImageReader::new(Cursor::new(&found.bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|e| format!("Could not read the picture: {e}"))?;
    let preview = image
        .resize(PREVIEW_MAX, PREVIEW_MAX, FilterType::Triangle)
        .to_rgba8();
    let preview = (preview.width(), preview.height(), preview.into_raw());
    Ok(Picture {
        width: image.width(),
        height: image.height(),
        image,
        name: file_name(&found.url, found.format),
        bytes: found.bytes,
        format: found.format,
        preview,
    })
}

/// A file name from the picture's URL, with an extension that matches.
pub fn file_name(url: &str, format: Option<ImageFormat>) -> String {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let last = if url.starts_with("data:") {
        ""
    } else {
        path.rsplit('/').next().unwrap_or("")
    };
    let last = percent_decode(last);
    let stem = Path::new(&last)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem: String = stem
        .chars()
        .map(|c| if c.is_alphanumeric() || "-_.".contains(c) { c } else { '-' })
        .collect();
    let stem = stem.trim_matches(['-', '.']);
    let stem = if stem.is_empty() { "picture" } else { stem };
    let ext = format
        .and_then(|f| f.extensions_str().first().copied())
        .unwrap_or("jpg");
    format!("{stem}.{ext}")
}

fn percent_decode(s: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The part of the picture kept, in its pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The screen's width over its height.
pub fn aspect(width: u32, height: u32) -> f32 {
    width.max(1) as f32 / height.max(1) as f32
}

/// The picture is already the screen's shape.
pub fn same_shape(w: u32, h: u32, screen: f32) -> bool {
    (aspect(w, h) / screen - 1.0).abs() < SAME_SHAPE
}

/// The largest part of a `w × h` picture in the `screen` shape, centered.
pub fn fit(w: u32, h: u32, screen: f32) -> Crop {
    let (w, h) = (w as f32, h as f32);
    let (cw, ch) = if w / h > screen {
        (h * screen, h)
    } else {
        (w, w / screen)
    };
    Crop {
        x: (w - cw) / 2.0,
        y: (h - ch) / 2.0,
        w: cw,
        h: ch,
    }
}

/// `c` moved by (`dx`, `dy`), kept inside the picture.
pub fn moved(c: Crop, dx: f32, dy: f32, w: u32, h: u32) -> Crop {
    Crop {
        x: (c.x + dx).clamp(0.0, (w as f32 - c.w).max(0.0)),
        y: (c.y + dy).clamp(0.0, (h as f32 - c.h).max(0.0)),
        ..c
    }
}

/// `c` scaled by `k` about its center, from [`MIN_ZOOM`] of the largest up
/// to the largest, kept inside the picture.
pub fn zoomed(c: Crop, k: f32, w: u32, h: u32, screen: f32) -> Crop {
    let largest = fit(w, h, screen);
    let cw = (c.w * k).clamp(largest.w * MIN_ZOOM, largest.w);
    let ch = cw / screen;
    let (cx, cy) = (c.x + c.w / 2.0, c.y + c.h / 2.0);
    let scaled = Crop {
        x: cx - cw / 2.0,
        y: cy - ch / 2.0,
        w: cw,
        h: ch,
    };
    moved(scaled, 0.0, 0.0, w, h)
}

/// Save the picture under `~/Pictures/Backgrounds/` — the chosen part as a
/// JPEG, or the file as fetched when nothing was cut — and adopt it as the
/// background. Returns Sola's copy for `Topic::Background`. Blocking.
pub fn save(pic: &Picture, crop: Option<Crop>) -> Result<PathBuf, String> {
    let dir = pictures_dir().join("Backgrounds");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not make {}: {e}", dir.display()))?;
    let (bytes, name) = match crop {
        None => (pic.bytes.clone(), pic.name.clone()),
        Some(c) => {
            let x = c.x.round().max(0.0) as u32;
            let y = c.y.round().max(0.0) as u32;
            let w = (c.w.round() as u32).min(pic.width - x.min(pic.width)).max(1);
            let h = (c.h.round() as u32).min(pic.height - y.min(pic.height)).max(1);
            let part = pic.image.crop_imm(x, y, w, h).to_rgb8();
            let mut out = Vec::new();
            JpegEncoder::new_with_quality(&mut out, QUALITY)
                .encode_image(&part)
                .map_err(|e| format!("Could not save the picture: {e}"))?;
            let stem = Path::new(&pic.name)
                .file_stem()
                .map_or_else(|| "picture".into(), |s| s.to_string_lossy().into_owned());
            (out, format!("{stem}-{w}x{h}.jpg"))
        }
    };
    let path = unused(&dir, &name);
    std::fs::write(&path, &bytes).map_err(|e| format!("Could not save {}: {e}", path.display()))?;
    tracing::info!(path = %path.display(), "desktop background saved");
    sola_core::background::adopt(&path).map_err(|e| format!("Could not set the background: {e}"))
}

/// `~/Pictures` (or `$XDG_PICTURES_DIR`).
fn pictures_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_PICTURES_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Pictures")
}

/// `dir/name`, or `name-2`, `name-3`… when that is taken.
fn unused(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let p = Path::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    (2..)
        .map(|n| dir.join(format!("{stem}-{n}.{ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// Decode and wrap for the window: shared, since it is cloned through
/// messages.
pub async fn find_and_decode(candidates: Vec<String>, referer: String) -> Result<Arc<Picture>, String> {
    let found = find(candidates, referer).await?;
    tokio::task::spawn_blocking(move || decode(found))
        .await
        .map_err(|e| e.to_string())?
        .map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikimedia_thumbnails_lead_to_the_original() {
        assert_eq!(
            wikimedia_original(
                "https://upload.wikimedia.org/wikipedia/commons/thumb/9/9b/Cosmic_Cliffs.jpg/640px-Cosmic_Cliffs.jpg"
            )
            .as_deref(),
            Some("https://upload.wikimedia.org/wikipedia/commons/9/9b/Cosmic_Cliffs.jpg")
        );
        // Wikipedia's own thumbnails now come from thumb.wikimedia.org, tagged.
        assert_eq!(
            wikimedia_original(
                "https://thumb.wikimedia.org/wikipedia/commons/thumb/4/49/NGC_3324.jpg/330px-NGC_3324.jpg?utm_source=en.wikipedia.org&utm_content=thumbnail"
            )
            .as_deref(),
            Some("https://upload.wikimedia.org/wikipedia/commons/4/49/NGC_3324.jpg")
        );
        assert_eq!(wikimedia_original("https://upload.wikimedia.org/wikipedia/commons/9/9b/A.jpg"), None);
        assert_eq!(wikimedia_original("https://example.com/thumb/a/ab/x.jpg/1px-x.jpg"), None);
    }

    #[test]
    fn the_link_and_the_original_come_before_the_picture() {
        let c = candidates(
            "https://upload.wikimedia.org/wikipedia/commons/thumb/9/9b/A.jpg/640px-A.jpg",
            Some("https://en.wikipedia.org/wiki/File:A.jpg"),
        );
        assert_eq!(
            c,
            vec![
                "https://en.wikipedia.org/wiki/File:A.jpg".to_string(),
                "https://upload.wikimedia.org/wikipedia/commons/9/9b/A.jpg".to_string(),
                "https://upload.wikimedia.org/wikipedia/commons/thumb/9/9b/A.jpg/640px-A.jpg".to_string(),
            ]
        );
        // No link, no duplicates; a javascript: link is not a candidate.
        assert_eq!(candidates("https://x/a.png", Some("javascript:void(0)")), vec!["https://x/a.png"]);
    }

    #[test]
    fn a_crop_is_the_largest_part_in_the_screens_shape() {
        let screen = aspect(1920, 1080);
        // A square picture: full width, a band in the middle.
        let c = fit(1000, 1000, screen);
        assert!((c.w - 1000.0).abs() < 0.01 && (c.h - 562.5).abs() < 0.01);
        assert!((c.y - 218.75).abs() < 0.01 && c.x == 0.0);
        // A panorama: full height, centered.
        let c = fit(4000, 1000, screen);
        assert!((c.h - 1000.0).abs() < 0.01 && (c.x - (4000.0 - c.w) / 2.0).abs() < 0.01);
        assert!(same_shape(3840, 2160, screen));
        assert!(!same_shape(1000, 1000, screen));
    }

    #[test]
    fn moving_and_zooming_stay_inside() {
        let screen = aspect(1920, 1080);
        let c = fit(1000, 1000, screen);
        let m = moved(c, 0.0, 10_000.0, 1000, 1000);
        assert!((m.y + m.h - 1000.0).abs() < 0.01);
        let z = zoomed(c, 0.5, 1000, 1000, screen);
        assert!((z.w - 500.0).abs() < 0.01 && (z.w / z.h - screen).abs() < 0.001);
        let tiny = zoomed(c, 0.01, 1000, 1000, screen);
        assert!((tiny.w - 1000.0 * MIN_ZOOM).abs() < 0.01);
        let big = zoomed(z, 10.0, 1000, 1000, screen);
        assert_eq!(big, fit(1000, 1000, screen));
    }

    #[test]
    fn file_names_come_from_the_url() {
        assert_eq!(file_name("https://x/a/Cosmic%20Cliffs.jpeg?w=640", Some(ImageFormat::Jpeg)), "Cosmic-Cliffs.jpg");
        assert_eq!(file_name("https://x/a/", Some(ImageFormat::Png)), "picture.png");
        assert_eq!(file_name("data:image/png;base64,AAAA", Some(ImageFormat::Png)), "picture.png");
    }

    #[test]
    fn data_urls_decode() {
        assert_eq!(data_url_bytes("image/png;base64,AAEC").unwrap(), vec![0, 1, 2]);
        assert!(data_url_bytes("image/svg+xml,<svg/>").is_err());
    }
}
