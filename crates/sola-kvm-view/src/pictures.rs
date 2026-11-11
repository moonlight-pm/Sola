//! The newest decoded picture, and the host's pointer, between the network's
//! thread and the GPU. Only the newest is kept: a frame the screen did not
//! have time for is one nobody needs.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sola_kvm::view_wire::{Cursor, Size};

use crate::stats::{Stamp, Stats};

/// One decoded picture, NV12, tightly packed.
#[derive(Default)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub y: Vec<u8>,
    pub uv: Vec<u8>,
    pub stamp: Option<Stamp>,
}

#[derive(Default)]
pub struct Pictures {
    pub stats: Stats,
    newest: Mutex<Option<Frame>>,
    spare: Mutex<Option<Frame>>,
    size: AtomicU64,
    /// The window has been told about a picture it has not drawn.
    told: AtomicBool,
    /// The host's pointer, when it sends one instead of drawing it in.
    cursor: Mutex<Option<Arc<Cursor>>>,
    /// The size the window would have the host's screen, said again on
    /// each connection.
    wanted: Mutex<Option<Size>>,
}

impl Pictures {
    pub fn with_stats(stats: Stats) -> Self {
        Self {
            stats,
            ..Self::default()
        }
    }

    /// A frame to decode into, with its old buffers when there is one.
    pub fn spare(&self) -> Frame {
        lock(&self.spare).take().unwrap_or_default()
    }

    /// Publish `frame`. True when the window needs telling.
    pub fn put(&self, frame: Frame) -> bool {
        self.size
            .store(u64::from(frame.width) << 32 | u64::from(frame.height), Ordering::Relaxed);
        if let Some(old) = lock(&self.newest).replace(frame) {
            *lock(&self.spare) = Some(old);
        }
        !self.told.swap(true, Ordering::AcqRel)
    }

    /// The picture to upload, if a new one came.
    pub fn take(&self) -> Option<Frame> {
        lock(&self.newest).take()
    }

    pub fn give_back(&self, frame: Frame) {
        *lock(&self.spare) = Some(frame);
    }

    /// The window is about to draw: pictures from here on need a new word.
    pub fn drawn(&self) {
        self.told.store(false, Ordering::Release);
    }

    pub fn set_cursor(&self, cursor: Option<Cursor>) {
        *lock(&self.cursor) = cursor.map(Arc::new);
    }

    /// The host's pointer; a new one is a new `Arc`.
    pub fn cursor(&self) -> Option<Arc<Cursor>> {
        lock(&self.cursor).clone()
    }

    /// Want `size`; false when it was wanted already.
    pub fn want(&self, size: Size) -> bool {
        lock(&self.wanted).replace(size) != Some(size)
    }

    pub fn wanted(&self) -> Option<Size> {
        *lock(&self.wanted)
    }

    pub fn size(&self) -> (u32, u32) {
        let size = self.size.load(Ordering::Relaxed);
        ((size >> 32) as u32, size as u32)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32) -> Frame {
        Frame {
            width,
            height: 2,
            ..Frame::default()
        }
    }

    #[test]
    fn only_the_newest_is_kept_and_the_old_buffers_come_back() {
        let p = Pictures::default();
        p.put(frame(1));
        p.put(frame(2));
        assert_eq!(p.take().unwrap().width, 2);
        assert!(p.take().is_none());
        assert_eq!(p.spare().width, 1);
    }

    #[test]
    fn the_window_is_told_once_until_it_draws() {
        let p = Pictures::default();
        assert!(p.put(frame(1)));
        assert!(!p.put(frame(2)));
        p.drawn();
        assert!(p.put(frame(3)));
    }

    #[test]
    fn size_follows_the_newest() {
        let p = Pictures::default();
        assert_eq!(p.size(), (0, 0));
        p.put(frame(3024));
        assert_eq!(p.size(), (3024, 2));
    }
}
