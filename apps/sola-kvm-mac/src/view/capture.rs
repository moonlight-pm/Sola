//! The picture source: ScreenCaptureKit on a display made the viewer's size
//! ([`display`](super::display)) or the main display, or the test card,
//! into the encoder. The pointer is left out of the picture when the viewer
//! can draw it ([`cursor`](super::cursor)).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_core_foundation::{CGPoint, CGRect};
use objc2_core_graphics::{
    CGDisplayCopyDisplayMode, CGDisplayMode, CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess,
    kCGColorSpaceSRGB, kCGDisplayStreamYCbCrMatrix_ITU_R_709_2,
};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{CVImageBuffer, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate, SCStreamOutput,
    SCStreamOutputType,
};
use tracing::{info, warn};

use super::cursor::Watch;
use super::display::{self, Virtual};
use super::encode::{Encoder, FPS};
use super::flow::{Gate, now_us};
use super::rate::CEILING_KBPS;
use super::pattern;
use super::server::{Feed, Msg, Source};
use super::shape::Shape;
use crate::view_wire::{Down, Size};

const FRAME_US: u32 = 1_000_000 / FPS as u32;
/// The test card when the viewer says no size: the laptop's screen.
const CARD: Shape = Shape {
    points: (1512, 982),
    scale: 2,
};
/// How long a made display may take to reach ScreenCaptureKit's list. It
/// tells no one, so this looks every [`LOOK`]; nothing is ordered by it.
const LISTED: Duration = Duration::from_secs(2);
const LOOK: Duration = Duration::from_millis(50);
/// A guard against an Apple callback that never comes, not a wait on order.
const CALLBACK: Duration = Duration::from_secs(10);
const QUEUE_DEPTH: isize = 5;

/// Frames on their way to the encoder.
pub struct Frames {
    encoder: Encoder,
    gate: Arc<Gate>,
    skipped: Arc<AtomicU32>,
    force_key: Arc<AtomicBool>,
    /// The bitrate the encoder has.
    kbps: AtomicU32,
}

impl Frames {
    pub fn offer(&self, image: &CVImageBuffer, pts: CMTime) {
        let captured_us = now_us();
        if !self.gate.admit(captured_us, FRAME_US) {
            self.skipped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let kbps = self.gate.kbps();
        if self.kbps.swap(kbps, Ordering::Relaxed) != kbps {
            self.encoder.set_kbps(kbps);
        }
        let key = self.force_key.swap(false, Ordering::Relaxed);
        if let Err(status) = self.encoder.encode(image, pts, key, captured_us) {
            warn!(status, "remote view: the encoder would not take a frame");
            self.gate.dropped();
        }
    }
}

/// Stops the source, and the pointer's watch, when dropped.
pub struct Running {
    _capture: Capture,
    _pointer: Option<Watch>,
}

enum Capture {
    Screen {
        stream: Retained<SCStream>,
        _output: Retained<Output>,
    },
    Pattern(Arc<AtomicBool>),
}

// Held by the session's thread alone; SCStream may be stopped from any thread.
unsafe impl Send for Capture {}

impl Drop for Capture {
    fn drop(&mut self) {
        match self {
            Self::Screen { stream, .. } => unsafe { stream.stopCaptureWithCompletionHandler(None) },
            Self::Pattern(stop) => stop.store(true, Ordering::Relaxed),
        }
    }
}

/// A session's display: one made the viewer's size when it says a size and
/// this Mac can, else the main one. Dropped with the session.
#[derive(Default)]
pub struct Screen {
    made: Option<Virtual>,
    /// This Mac would not make one: the main display from then on.
    refused: bool,
}

impl Screen {
    /// The made display at `shape`, and main; `None` for the main display.
    /// `displays`: where each is, as ScreenCaptureKit lists them.
    fn fit(&mut self, shape: Shape, displays: &[(u32, CGRect)]) -> Option<(u32, Shape)> {
        if self.refused {
            return None;
        }
        let made = match self.made.take() {
            Some(made) => Ok(made),
            None => Virtual::new(shape),
        };
        match made.and_then(|mut made| made.fit(shape, displays).map(|()| made)) {
            Ok(made) => {
                let id = made.id();
                self.made = Some(made);
                info!(id, ?shape, "remote view: a display the window's size, made main");
                Some((id, shape))
            }
            Err(why) => {
                warn!(%why, "remote view: no display of the window's size; the main display");
                self.give_up();
                None
            }
        }
    }

    /// The main display from now on: the made one goes.
    fn give_up(&mut self) {
        self.made = None;
        self.refused = true;
    }
}

/// Start sending: the screen at the viewer's `size` when it said one.
pub fn start(source: Source, size: Option<Size>, screen: &mut Screen, feed: Feed) -> Result<(Down, Running), String> {
    let shape = size.map(Shape::of);
    match source {
        Source::Screen => capture_screen(feed, screen, shape),
        Source::TestPattern => test_pattern(feed, shape.map_or(CARD, |shape| Shape { scale: 1, ..shape })),
    }
}

fn frames(width: usize, height: usize, feed: Feed) -> Result<(Arc<Frames>, Sender<Msg>), String> {
    let Feed {
        tx,
        gate,
        skipped,
        force_key,
        generation,
    } = feed;
    let encoder = Encoder::new(width as i32, height as i32, tx.clone(), Arc::clone(&gate), generation)?;
    Ok((
        Arc::new(Frames {
            encoder,
            gate,
            skipped,
            force_key,
            kbps: AtomicU32::new(CEILING_KBPS),
        }),
        tx,
    ))
}

/// The pointer's watch, for a picture `width` pixels across that input
/// reaches as `points_w` points.
fn pointer(tx: Sender<Msg>, width: usize, points_w: u32) -> Option<Watch> {
    let watch = Watch::start(tx, width as f64 / f64::from(points_w.max(1)));
    info!(drawn_by_the_viewer = watch.is_some(), "remote view: the pointer");
    watch
}

fn test_pattern(feed: Feed, shape: Shape) -> Result<(Down, Running), String> {
    let (width, height) = shape.pixels();
    let (frames, tx) = frames(width, height, feed)?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    thread::Builder::new()
        .name("kvm-view-pattern".into())
        .spawn(move || pattern::run(frames, flag, width, height))
        .map_err(|e| format!("could not start the test pattern: {e}"))?;
    let (points_w, points_h) = shape.points;
    // The card has no pointer of its own: this Mac's is drawn over it, so
    // the viewer's half can be tried without moving anyone's.
    let running = Running {
        _capture: Capture::Pattern(stop),
        _pointer: pointer(tx, width, points_w),
    };
    let display = Down::Display {
        width: width as u32,
        height: height as u32,
        points_w,
        points_h,
    };
    Ok((display, running))
}

/// The main display, at (0, 0), and not one this process made and let go.
fn main_display(content: &SCShareableContent) -> Result<Retained<SCDisplay>, String> {
    let kept: Vec<Retained<SCDisplay>> =
        unsafe { content.displays() }.iter().filter(|d| !display::gone(unsafe { d.displayID() })).collect();
    let main = kept.iter().position(|d| unsafe { d.frame() }.origin == CGPoint::ZERO).unwrap_or(0);
    kept.into_iter().nth(main).ok_or_else(|| "ScreenCaptureKit does not list the main display".to_string())
}

/// The display made with `id`, once ScreenCaptureKit lists it at `shape`.
fn made_display(id: u32, shape: Shape) -> Result<Retained<SCDisplay>, String> {
    let started = Instant::now();
    loop {
        let content = shareable_content()?;
        let listed = unsafe { content.displays() }.iter().find(|d| {
            unsafe { d.displayID() == id && (d.width(), d.height()) == (shape.points.0 as isize, shape.points.1 as isize) }
        });
        if let Some(display) = listed {
            return Ok(display);
        }
        if started.elapsed() > LISTED {
            return Err(format!("ScreenCaptureKit does not list the display made for the window at {shape:?}"));
        }
        thread::sleep(LOOK);
    }
}

/// The display made at `shape` when there is one and ScreenCaptureKit lists
/// it; else the main display.
fn capture_screen(feed: Feed, screen: &mut Screen, shape: Option<Shape>) -> Result<(Down, Running), String> {
    if !CGPreflightScreenCaptureAccess() {
        // Puts this app in the list in System Settings; the answer takes
        // effect the next time the agent starts.
        CGRequestScreenCaptureAccess();
        return Err(
            "This Mac has not allowed Screen Recording for SolaKvmMac. Turn it on in System Settings, \
             Privacy & Security, Screen & System Audio Recording, then restart the agent."
                .into(),
        );
    }
    let content = shareable_content()?;
    let displays: Vec<_> = unsafe { content.displays() }.iter().map(|d| unsafe { (d.displayID(), d.frame()) }).collect();
    let made = shape.and_then(|shape| screen.fit(shape, &displays)).and_then(|(id, shape)| match made_display(id, shape) {
        Ok(display) => Some((display, shape)),
        Err(why) => {
            warn!(%why, "remote view: the main display");
            screen.give_up();
            None
        }
    });
    let (display, (width, height), (points_w, points_h)) = match made {
        Some((display, shape)) => (display, shape.pixels(), shape.points),
        None => {
            let display = main_display(&content)?;
            let id = unsafe { display.displayID() };
            let mode = CGDisplayCopyDisplayMode(id).ok_or_else(|| "the main display has no mode".to_string())?;
            let pixels = (CGDisplayMode::pixel_width(Some(&mode)), CGDisplayMode::pixel_height(Some(&mode)));
            let points = unsafe { (display.width() as u32, display.height() as u32) };
            (display, pixels, points)
        }
    };
    let generation = feed.generation;
    let (frames, tx) = frames(width, height, feed)?;
    let pointer = pointer(tx.clone(), width, points_w);

    let config = unsafe { SCStreamConfiguration::new() };
    unsafe {
        config.setWidth(width);
        config.setHeight(height);
        config.setMinimumFrameInterval(CMTime::new(1, FPS));
        config.setPixelFormat(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
        config.setColorMatrix(kCGDisplayStreamYCbCrMatrix_ITU_R_709_2);
        config.setColorSpaceName(kCGColorSpaceSRGB);
        config.setShowsCursor(pointer.is_none());
        config.setQueueDepth(QUEUE_DEPTH);
    }
    let filter = unsafe {
        SCContentFilter::initWithDisplay_excludingWindows(SCContentFilter::alloc(), &display, &NSArray::new())
    };

    let output = Output::new(frames, tx, generation);
    let delegate = ProtocolObject::from_ref(&*output);
    let stream =
        unsafe { SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &config, Some(delegate)) };
    let queue = DispatchQueue::new("sola.kvm.view", DispatchQueueAttr::SERIAL);
    let added = unsafe {
        stream.addStreamOutput_type_sampleHandlerQueue_error(
            ProtocolObject::from_ref(&*output),
            SCStreamOutputType::Screen,
            Some(&queue),
        )
    };
    added.map_err(|e| format!("ScreenCaptureKit would not take the output: {}", describe(&e)))?;
    start_capture(&stream)?;
    let display = Down::Display {
        width: width as u32,
        height: height as u32,
        points_w,
        points_h,
    };
    let running = Running {
        _capture: Capture::Screen { stream, _output: output },
        _pointer: pointer,
    };
    Ok((display, running))
}

struct Sendable<T>(T);
// Handed once from Apple's callback queue to the thread that waits for it.
unsafe impl<T> Send for Sendable<T> {}

fn shareable_content() -> Result<Retained<SCShareableContent>, String> {
    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(move |content: *mut SCShareableContent, error: *mut NSError| {
        let result = unsafe { Retained::retain(content) }.ok_or_else(|| describe_ptr(error));
        let _ = tx.send(Sendable(result));
    });
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };
    match rx.recv_timeout(CALLBACK) {
        Ok(Sendable(result)) => result.map_err(|e| format!("ScreenCaptureKit lists no screens: {e}")),
        Err(_) => Err("ScreenCaptureKit did not answer".into()),
    }
}

fn start_capture(stream: &SCStream) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(move |error: *mut NSError| {
        let _ = tx.send((!error.is_null()).then(|| describe_ptr(error)));
    });
    unsafe { stream.startCaptureWithCompletionHandler(Some(&block)) };
    match rx.recv_timeout(CALLBACK) {
        Ok(None) => Ok(()),
        Ok(Some(e)) => Err(format!("ScreenCaptureKit would not start: {e}")),
        Err(_) => Err("ScreenCaptureKit did not answer".into()),
    }
}

fn describe(error: &NSError) -> String {
    error.localizedDescription().to_string()
}

fn describe_ptr(error: *mut NSError) -> String {
    unsafe { error.as_ref() }.map_or_else(|| "no reason given".into(), describe)
}

pub struct OutputIvars {
    frames: Arc<Frames>,
    tx: Sender<Msg>,
    generation: u32,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SolaKvmViewOutput"]
    #[ivars = OutputIvars]
    pub struct Output;

    unsafe impl NSObjectProtocol for Output {}

    unsafe impl SCStreamOutput for Output {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        unsafe fn stream_did_output(&self, _stream: &SCStream, sample: &CMSampleBuffer, kind: SCStreamOutputType) {
            if kind != SCStreamOutputType::Screen {
                return;
            }
            // A frame that says only "nothing changed" has no image.
            if let Some(image) = unsafe { sample.image_buffer() } {
                let pts = unsafe { sample.presentation_time_stamp() };
                self.ivars().frames.offer(&image, pts);
            }
        }
    }

    unsafe impl SCStreamDelegate for Output {
        #[unsafe(method(stream:didStopWithError:))]
        unsafe fn stream_did_stop(&self, _stream: &SCStream, error: &NSError) {
            warn!(error = %describe(error), "remote view: capture stopped");
            let _ = self.ivars().tx.send(Msg::Stopped(self.ivars().generation));
        }
    }
);

impl Output {
    fn new(frames: Arc<Frames>, tx: Sender<Msg>, generation: u32) -> Retained<Self> {
        let this = Self::alloc().set_ivars(OutputIvars { frames, tx, generation });
        unsafe { msg_send![super(this), init] }
    }
}
