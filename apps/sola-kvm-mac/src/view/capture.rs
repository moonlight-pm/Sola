//! The picture source: ScreenCaptureKit on the main display (or the test
//! card), into the encoder.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchQueueAttr};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_core_graphics::{
    CGDisplayCopyDisplayMode, CGDisplayMode, CGMainDisplayID, CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess,
    kCGColorSpaceSRGB, kCGDisplayStreamYCbCrMatrix_ITU_R_709_2,
};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{CVImageBuffer, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate, SCStreamOutput,
    SCStreamOutputType,
};
use tracing::warn;

use super::encode::{Encoder, FPS};
use super::pattern;
use super::server::{Feed, Msg, Source};
use crate::view_wire::Down;

/// Pictures allowed out at once; past this, frames are skipped.
const MAX_IN_FLIGHT: usize = 3;
/// A guard against an Apple callback that never comes, not a wait on order.
const CALLBACK: Duration = Duration::from_secs(10);
const QUEUE_DEPTH: isize = 5;

/// Frames on their way to the encoder.
pub struct Frames {
    encoder: Encoder,
    in_flight: Arc<AtomicUsize>,
    force_key: Arc<AtomicBool>,
}

impl Frames {
    pub fn offer(&self, image: &CVImageBuffer, pts: CMTime) {
        if self.in_flight.load(Ordering::Relaxed) >= MAX_IN_FLIGHT {
            return;
        }
        self.in_flight.fetch_add(1, Ordering::Relaxed);
        let key = self.force_key.swap(false, Ordering::Relaxed);
        if let Err(status) = self.encoder.encode(image, pts, key) {
            warn!(status, "remote view: the encoder would not take a frame");
            self.in_flight.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

/// Stops the source when dropped.
pub enum Running {
    Screen {
        stream: Retained<SCStream>,
        _output: Retained<Output>,
    },
    Pattern(Arc<AtomicBool>),
}

// Held by the session's thread alone; SCStream may be stopped from any thread.
unsafe impl Send for Running {}

impl Drop for Running {
    fn drop(&mut self) {
        match self {
            Self::Screen { stream, .. } => unsafe { stream.stopCaptureWithCompletionHandler(None) },
            Self::Pattern(stop) => stop.store(true, Ordering::Relaxed),
        }
    }
}

pub fn start(source: Source, feed: Feed) -> Result<(Down, Running), String> {
    match source {
        Source::Screen => screen(feed),
        Source::TestPattern => test_pattern(feed),
    }
}

fn frames(width: usize, height: usize, feed: Feed) -> Result<(Arc<Frames>, Sender<Msg>), String> {
    let Feed { tx, in_flight, force_key } = feed;
    let encoder = Encoder::new(width as i32, height as i32, tx.clone(), Arc::clone(&in_flight))?;
    Ok((Arc::new(Frames { encoder, in_flight, force_key }), tx))
}

fn test_pattern(feed: Feed) -> Result<(Down, Running), String> {
    let (width, height) = (pattern::WIDTH, pattern::HEIGHT);
    let (frames, _) = frames(width, height, feed)?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    thread::Builder::new()
        .name("kvm-view-pattern".into())
        .spawn(move || pattern::run(frames, flag))
        .map_err(|e| format!("could not start the test pattern: {e}"))?;
    let display = Down::Display {
        width: width as u32,
        height: height as u32,
        points_w: width as u32 / 2,
        points_h: height as u32 / 2,
    };
    Ok((display, Running::Pattern(stop)))
}

fn screen(feed: Feed) -> Result<(Down, Running), String> {
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
    let id = CGMainDisplayID();
    let content = shareable_content()?;
    let displays = unsafe { content.displays() };
    let display: Retained<SCDisplay> = displays
        .iter()
        .find(|d| unsafe { d.displayID() } == id)
        .ok_or_else(|| "ScreenCaptureKit does not list the main display".to_string())?;
    let mode = CGDisplayCopyDisplayMode(id).ok_or_else(|| "the main display has no mode".to_string())?;
    let (width, height) = (CGDisplayMode::pixel_width(Some(&mode)), CGDisplayMode::pixel_height(Some(&mode)));
    let (points_w, points_h) = unsafe { (display.width() as u32, display.height() as u32) };

    let config = unsafe { SCStreamConfiguration::new() };
    unsafe {
        config.setWidth(width);
        config.setHeight(height);
        config.setMinimumFrameInterval(CMTime::new(1, FPS));
        config.setPixelFormat(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
        config.setColorMatrix(kCGDisplayStreamYCbCrMatrix_ITU_R_709_2);
        config.setColorSpaceName(kCGColorSpaceSRGB);
        config.setShowsCursor(true);
        config.setQueueDepth(QUEUE_DEPTH);
    }
    let filter = unsafe {
        SCContentFilter::initWithDisplay_excludingWindows(SCContentFilter::alloc(), &display, &NSArray::new())
    };

    let (frames, tx) = frames(width, height, feed)?;
    let output = Output::new(frames, tx);
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
    Ok((display, Running::Screen { stream, _output: output }))
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
            let _ = self.ivars().tx.send(Msg::Closed);
        }
    }
);

impl Output {
    fn new(frames: Arc<Frames>, tx: Sender<Msg>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(OutputIvars { frames, tx });
        unsafe { msg_send![super(this), init] }
    }
}
