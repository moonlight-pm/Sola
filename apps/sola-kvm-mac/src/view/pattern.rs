//! A test card in place of the screen, through the real encoder and wire:
//! four known colors and a bar that moves, so a screenshot of the viewer
//! can say that chroma, plane order and motion all arrived.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use objc2_core_foundation::CFRetained;
use objc2_core_media::CMTime;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
};

use super::capture::Frames;

pub const WIDTH: usize = 3024;
pub const HEIGHT: usize = 1964;
const BAR: usize = 48;
const PERIOD: Duration = Duration::from_micros(16_667);

/// Quadrants (Y, U, V), BT.709 limited range: red, green, blue, grey.
const QUADRANTS: [[u8; 3]; 4] = [[63, 102, 240], [173, 42, 26], [32, 240, 118], [126, 128, 128]];

pub fn run(frames: Arc<Frames>, stop: Arc<AtomicBool>) {
    let start = Instant::now();
    let mut next = start;
    let mut n = 0usize;
    while !stop.load(Ordering::Relaxed) {
        if let Some(image) = paint(n) {
            let us = start.elapsed().as_micros() as i64;
            frames.offer(&image, unsafe { CMTime::new(us, 1_000_000) });
        }
        n += 1;
        next += PERIOD;
        thread::sleep(next.saturating_duration_since(Instant::now()));
    }
}

fn paint(frame: usize) -> Option<CFRetained<CVPixelBuffer>> {
    let mut raw = std::ptr::null_mut();
    let status = unsafe {
        CVPixelBufferCreate(
            None,
            WIDTH,
            HEIGHT,
            kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            None,
            std::ptr::NonNull::from(&mut raw),
        )
    };
    if status != 0 {
        return None;
    }
    let image = unsafe { CFRetained::from_raw(std::ptr::NonNull::new(raw)?) };
    let flags = CVPixelBufferLockFlags(0);
    unsafe { CVPixelBufferLockBaseAddress(&image, flags) };
    let bar = (frame * 8) % WIDTH;
    let quadrant = |x: usize, y: usize| QUADRANTS[usize::from(x >= WIDTH / 2) + 2 * usize::from(y >= HEIGHT / 2)];
    unsafe {
        let (y_plane, y_row) = (CVPixelBufferGetBaseAddressOfPlane(&image, 0).cast::<u8>(), CVPixelBufferGetBytesPerRowOfPlane(&image, 0));
        let (uv_plane, uv_row) = (CVPixelBufferGetBaseAddressOfPlane(&image, 1).cast::<u8>(), CVPixelBufferGetBytesPerRowOfPlane(&image, 1));
        for y in 0..HEIGHT {
            let row = std::slice::from_raw_parts_mut(y_plane.add(y * y_row), WIDTH);
            for (x, px) in row.iter_mut().enumerate() {
                *px = if x.wrapping_sub(bar) < BAR { 235 } else { quadrant(x, y)[0] };
            }
        }
        for y in 0..HEIGHT / 2 {
            let row = std::slice::from_raw_parts_mut(uv_plane.add(y * uv_row), WIDTH);
            for x in 0..WIDTH / 2 {
                let [_, u, v] = quadrant(x * 2, y * 2);
                row[x * 2] = u;
                row[x * 2 + 1] = v;
            }
        }
        CVPixelBufferUnlockBaseAddress(&image, flags);
    }
    Some(image)
}
