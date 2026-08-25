//! Open document: decoded pixels, iced handle, undo, save.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ::image::{DynamicImage, ImageFormat, RgbaImage};
use iced::Vector;
use iced::widget::image as iced_image;

const UNDO_CAP: usize = 8;

/// Pixels decoded off the UI thread. [`Doc::from_loaded`] builds the handle.
/// `Arc` so the iced `Message` clone does not copy the buffer.
#[derive(Clone)]
pub struct Loaded {
    pub id: u64,
    pub path: PathBuf,
    pub pixels: Arc<RgbaImage>,
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loaded")
            .field("id", &self.id)
            .field("path", &self.path)
            .field("width", &self.pixels.width())
            .field("height", &self.pixels.height())
            .finish()
    }
}

pub struct Doc {
    pub id: u64,
    pub path: Option<PathBuf>,
    pub pixels: RgbaImage,
    pub handle: iced_image::Handle,
    pub dirty: bool,
    /// 1.0 = contain-fit. Per-doc so tab switches remember the view.
    pub zoom: f32,
    pub pan: Vector,
    undo: Vec<RgbaImage>,
}

impl Doc {
    /// Decode on a worker. Safe to call from `spawn_blocking`.
    pub fn load_pixels(id: u64, path: PathBuf) -> Result<Loaded, String> {
        let dyn_img = ::image::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Loaded {
            id,
            path,
            pixels: Arc::new(dyn_img.to_rgba8()),
        })
    }

    pub fn from_loaded(loaded: Loaded) -> Self {
        let pixels = Arc::try_unwrap(loaded.pixels).unwrap_or_else(|a| (*a).clone());
        let handle = handle_from(&pixels);
        Self {
            id: loaded.id,
            path: Some(loaded.path),
            pixels,
            handle,
            dirty: false,
            zoom: 1.0,
            pan: Vector::ZERO,
            undo: Vec::new(),
        }
    }

    pub fn load(id: u64, path: PathBuf) -> Result<Self, String> {
        Ok(Self::from_loaded(Self::load_pixels(id, path)?))
    }

    pub fn reset_view(&mut self) {
        self.zoom = 1.0;
        self.pan = Vector::ZERO;
    }

    pub fn zoom_label(&self) -> String {
        format!("{}%", (self.zoom * 100.0).round() as i32)
    }

    pub fn label(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        if self.dirty {
            format!("{name} ·")
        } else {
            name
        }
    }

    pub fn dims_label(&self) -> String {
        format!("{} × {}", self.pixels.width(), self.pixels.height())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    fn push_undo(&mut self) {
        self.undo.push(self.pixels.clone());
        if self.undo.len() > UNDO_CAP {
            self.undo.remove(0);
        }
    }

    fn commit(&mut self, next: RgbaImage) {
        self.push_undo();
        self.pixels = next;
        self.handle = handle_from(&self.pixels);
        self.dirty = true;
        self.reset_view();
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.pixels = prev;
            self.handle = handle_from(&self.pixels);
            self.dirty = true;
            self.reset_view();
        }
    }

    pub fn crop(&mut self, x: u32, y: u32, w: u32, h: u32) -> Result<(), String> {
        if w == 0 || h == 0 {
            return Err("Crop is empty".into());
        }
        if x.saturating_add(w) > self.pixels.width() || y.saturating_add(h) > self.pixels.height() {
            return Err("Crop is outside the image".into());
        }
        let next = ::image::imageops::crop_imm(&self.pixels, x, y, w, h).to_image();
        self.commit(next);
        Ok(())
    }

    pub fn rotate_cw(&mut self) {
        self.commit(::image::imageops::rotate90(&self.pixels));
    }

    pub fn rotate_ccw(&mut self) {
        self.commit(::image::imageops::rotate270(&self.pixels));
    }

    pub fn flip_h(&mut self) {
        self.commit(::image::imageops::flip_horizontal(&self.pixels));
    }

    pub fn flip_v(&mut self) {
        self.commit(::image::imageops::flip_vertical(&self.pixels));
    }

    pub fn save(&mut self) -> Result<(), String> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| "No path — use Save as".to_string())?;
        self.save_to(&path)
    }

    pub fn save_to(&mut self, path: &Path) -> Result<(), String> {
        write_image(&self.pixels, path)?;
        self.path = Some(path.to_path_buf());
        self.dirty = false;
        Ok(())
    }
}

fn handle_from(pixels: &RgbaImage) -> iced_image::Handle {
    iced_image::Handle::from_rgba(pixels.width(), pixels.height(), pixels.as_raw().clone())
}

fn write_image(pixels: &RgbaImage, path: &Path) -> Result<(), String> {
    let format = ImageFormat::from_path(path).unwrap_or(ImageFormat::Png);
    match format {
        ImageFormat::Jpeg => DynamicImage::ImageRgba8(pixels.clone())
            .to_rgb8()
            .save_with_format(path, format)
            .map_err(|e| format!("{}: {e}", path.display())),
        _ => pixels
            .save_with_format(path, format)
            .map_err(|e| format!("{}: {e}", path.display())),
    }
}
