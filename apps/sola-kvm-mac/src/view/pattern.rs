//! A test card in place of the screen, through the real encoder and wire:
//! four known colors and a bar that moves, so a screenshot of the viewer
//! can say that chroma, plane order and motion all arrived. A grey band of
//! text-like detail scrolls across the middle third, so the card presses the
//! bitrate the way a scrolled page does. It is drawn at the size the viewer
//! asks for, as a display made for it would be.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use objc2_core_foundation::{CFDictionary, CFRetained, CFString, CFType};
use objc2_core_media::CMTime;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelBufferIOSurfacePropertiesKey, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
};

use super::capture::Frames;

const BAR: usize = 48;
const PERIOD: Duration = Duration::from_micros(16_667);
/// How many rows the band scrolls a frame.
const SCROLL: usize = 6;
/// The page that scrolls through the band, in rows.
const PAGE_ROWS: usize = 2048;
/// A glyph's cell, and the glyph inside it.
const CELL: (usize, usize) = (12, 20);
const GLYPH: (usize, usize) = (7, 11);
const INK: u8 = 16;
const PAPER: u8 = 235;

/// Quadrants (Y, U, V), BT.709 limited range: red, green, blue, grey.
const QUADRANTS: [[u8; 3]; 4] = [[63, 102, 240], [173, 42, 26], [32, 240, 118], [126, 128, 128]];

/// `width` by `height` pixels, both even.
pub fn run(frames: Arc<Frames>, stop: Arc<AtomicBool>, width: usize, height: usize) {
    let page = page(width);
    let start = Instant::now();
    let mut next = start;
    let mut n = 0usize;
    while !stop.load(Ordering::Relaxed) {
        if let Some(image) = paint(n, &page, width, height) {
            let us = start.elapsed().as_micros() as i64;
            frames.offer(&image, unsafe { CMTime::new(us, 1_000_000) });
        }
        n += 1;
        next += PERIOD;
        thread::sleep(next.saturating_duration_since(Instant::now()));
    }
}

/// Luma of a page of made-up text: random strokes in a grid of cells, with a
/// space now and then.
fn page(width: usize) -> Vec<u8> {
    let mut page = vec![PAPER; width * PAGE_ROWS];
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    for cell_y in 0..PAGE_ROWS / CELL.1 {
        for cell_x in 0..width / CELL.0 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            if seed & 0xf == 0 {
                continue;
            }
            for y in 0..GLYPH.1 {
                for x in 0..GLYPH.0 {
                    if seed >> ((y * GLYPH.0 + x) % 60 + 4) & 1 == 1 {
                        page[(cell_y * CELL.1 + 4 + y) * width + cell_x * CELL.0 + 2 + x] = INK;
                    }
                }
            }
        }
    }
    page
}

fn paint(frame: usize, page: &[u8], width: usize, height: usize) -> Option<CFRetained<CVPixelBuffer>> {
    // The band of text, across the middle third.
    let band = height / 3..height * 2 / 3;
    // On an IOSurface, as ScreenCaptureKit's frames are, so the encoder
    // takes the card as it takes the screen.
    let surface = CFDictionary::<CFString, CFType>::from_slices(&[], &[]);
    let attributes = CFDictionary::<CFString, CFType>::from_slices(
        &[unsafe { kCVPixelBufferIOSurfacePropertiesKey }],
        &[surface.as_ref()],
    );
    let mut raw = std::ptr::null_mut();
    let status = unsafe {
        CVPixelBufferCreate(
            None,
            width,
            height,
            kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            Some(attributes.as_opaque()),
            std::ptr::NonNull::from(&mut raw),
        )
    };
    if status != 0 {
        return None;
    }
    let image = unsafe { CFRetained::from_raw(std::ptr::NonNull::new(raw)?) };
    let flags = CVPixelBufferLockFlags(0);
    unsafe { CVPixelBufferLockBaseAddress(&image, flags) };
    let bar = (frame * 8) % width;
    let quadrant = |x: usize, y: usize| QUADRANTS[usize::from(x >= width / 2) + 2 * usize::from(y >= height / 2)];
    unsafe {
        let (y_plane, y_row) = (CVPixelBufferGetBaseAddressOfPlane(&image, 0).cast::<u8>(), CVPixelBufferGetBytesPerRowOfPlane(&image, 0));
        let (uv_plane, uv_row) = (CVPixelBufferGetBaseAddressOfPlane(&image, 1).cast::<u8>(), CVPixelBufferGetBytesPerRowOfPlane(&image, 1));
        for y in 0..height {
            let row = std::slice::from_raw_parts_mut(y_plane.add(y * y_row), width);
            if band.contains(&y) {
                let at = (y - band.start + frame * SCROLL) % PAGE_ROWS * width;
                row.copy_from_slice(&page[at..at + width]);
                continue;
            }
            for (x, px) in row.iter_mut().enumerate() {
                *px = if x.wrapping_sub(bar) < BAR { 235 } else { quadrant(x, y)[0] };
            }
        }
        for y in 0..height / 2 {
            let row = std::slice::from_raw_parts_mut(uv_plane.add(y * uv_row), width);
            for x in 0..width / 2 {
                let [_, u, v] = if band.contains(&(y * 2)) { [0, 128, 128] } else { quadrant(x * 2, y * 2) };
                row[x * 2] = u;
                row[x * 2 + 1] = v;
            }
        }
        CVPixelBufferUnlockBaseAddress(&image, flags);
    }
    Some(image)
}
