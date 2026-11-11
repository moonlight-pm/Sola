//! The host's pointer, drawn by the viewer: the system cursor's image, read
//! 30 times a second while a viewer is connected and sent when it changes.
//! macOS says nothing when another app changes the cursor, so it is read,
//! not waited on; nothing is ordered by it.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use objc2::rc::autoreleasepool;
use objc2_app_kit::NSCursor;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, kCGColorSpaceSRGB};
use tracing::warn;

use super::server::Msg;
use crate::view_wire::{Cursor, MAX_CURSOR};

const EVERY: Duration = Duration::from_millis(33);

/// Reads the pointer until dropped.
pub struct Watch {
    _stop: Sender<()>,
}

impl Watch {
    /// `None` when this Mac will not say what its pointer looks like; the
    /// picture keeps it then. `scale` is the picture's pixels per point.
    pub fn start(tx: Sender<Msg>, scale: f64) -> Option<Self> {
        let first = read(scale)?;
        let (stop, stopped) = mpsc::channel();
        let spawned = thread::Builder::new().name("kvm-view-cursor".into()).spawn(move || {
            let mut last = first;
            if tx.send(Msg::Cursor(last.clone())).is_err() {
                return;
            }
            while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(EVERY) {
                match read(scale) {
                    Some(cursor) if cursor != last => {
                        last = cursor;
                        if tx.send(Msg::Cursor(last.clone())).is_err() {
                            return;
                        }
                    }
                    _ => {}
                }
            }
        });
        match spawned {
            Ok(_) => Some(Self { _stop: stop }),
            Err(e) => {
                warn!(%e, "remote view: could not start the pointer's thread");
                None
            }
        }
    }
}

/// The system cursor at `scale` pixels per point, no bigger than
/// [`MAX_CURSOR`]; `None` when macOS will not say.
#[allow(deprecated)]
fn read(scale: f64) -> Option<Cursor> {
    autoreleasepool(|_| {
        let cursor = NSCursor::currentSystemCursor()?;
        let image = cursor.image();
        let (size, hot) = (image.size(), cursor.hotSpot());
        let scale = scale.min(f64::from(MAX_CURSOR) / size.width.max(size.height).max(1.0));
        let px = |points: f64, most: u16| ((points * scale).round().max(0.0) as u16).min(most);
        let (width, height) = (px(size.width, MAX_CURSOR).max(1), px(size.height, MAX_CURSOR).max(1));
        let mut rect = CGRect::new(CGPoint::ZERO, CGSize::new(f64::from(width), f64::from(height)));
        let picture = unsafe { image.CGImageForProposedRect_context_hints(&mut rect, None, None) }?;
        let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
        let mut rgba = vec![0u8; usize::from(width) * usize::from(height) * 4];
        let context = unsafe {
            CGBitmapContextCreate(
                rgba.as_mut_ptr().cast(),
                usize::from(width),
                usize::from(height),
                8,
                usize::from(width) * 4,
                Some(&space),
                CGImageAlphaInfo::PremultipliedLast.0,
            )
        }?;
        CGContext::draw_image(Some(&context), rect, Some(&picture));
        drop(context);
        Some(Cursor {
            width,
            height,
            hot_x: px(hot.x, width - 1),
            hot_y: px(hot.y, height - 1),
            rgba,
        })
    })
}
