//! The encoder: VideoToolbox's hardware HEVC, tuned for one frame in, one
//! frame out, no reordering.

use std::ffi::{c_char, c_void};
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, kCFBooleanTrue};
use objc2_core_media::{
    CMBlockBuffer, CMFormatDescription, CMSampleBuffer, CMTime, CMVideoFormatDescriptionGetHEVCParameterSetAtIndex,
    kCMSampleAttachmentKey_NotSync, kCMTimeInvalid, kCMVideoCodecType_HEVC,
};
use objc2_core_video::CVImageBuffer;
use objc2_video_toolbox::{
    VTCompressionSession, VTEncodeInfoFlags, VTSessionSetProperty, kVTCompressionPropertyKey_AllowFrameReordering,
    kVTCompressionPropertyKey_AverageBitRate, kVTCompressionPropertyKey_ExpectedFrameRate,
    kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration,
    kVTCompressionPropertyKey_ProfileLevel, kVTCompressionPropertyKey_RealTime, kVTEncodeFrameOptionKey_ForceKeyFrame,
    kVTProfileLevel_HEVC_Main_AutoLevel, kVTVideoEncoderSpecification_EnableLowLatencyRateControl,
    kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder,
};
use tracing::{info, warn};

use super::annexb;
use super::server::Msg;
use crate::view_wire::Down;

pub const FPS: i32 = 60;
const BITS_PER_SECOND: i32 = 50_000_000;
/// A key picture this often even when none is asked for.
const KEY_INTERVAL_SECONDS: i32 = 10;

struct Out {
    tx: Sender<Msg>,
    in_flight: Arc<AtomicUsize>,
}

pub struct Encoder {
    session: CFRetained<VTCompressionSession>,
    /// The callback's argument, alive as long as the session.
    out: *mut Out,
}

// VTCompressionSession may be fed from any thread; `out` is only read, by
// the session's callback, until `drop` has stopped it.
unsafe impl Send for Encoder {}
unsafe impl Sync for Encoder {}

impl Encoder {
    pub fn new(width: i32, height: i32, tx: Sender<Msg>, in_flight: Arc<AtomicUsize>) -> Result<Self, String> {
        let out = Box::into_raw(Box::new(Out { tx, in_flight }));
        let session = create(width, height, out, true)
            .or_else(|status| {
                info!(status, "remote view: no low-latency rate control for HEVC here; using the default");
                create(width, height, out, false)
            })
            .map_err(|status| {
                drop(unsafe { Box::from_raw(out) });
                format!("VideoToolbox would not make an HEVC encoder for {width}x{height} (status {status})")
            })?;
        configure(&session);
        Ok(Self { session, out })
    }

    pub fn encode(&self, image: &CVImageBuffer, pts: CMTime, force_key: bool) -> Result<(), i32> {
        let force: Option<CFRetained<CFDictionary<CFString, CFBoolean>>> = force_key.then(|| {
            let t = unsafe { kCFBooleanTrue }.expect("kCFBooleanTrue");
            CFDictionary::from_slices(&[unsafe { kVTEncodeFrameOptionKey_ForceKeyFrame }], &[t])
        });
        let props = force.as_ref().map(|d| d.as_opaque());
        let status = unsafe {
            self.session.encode_frame(
                image,
                pts,
                kCMTimeInvalid,
                props,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if status == 0 { Ok(()) } else { Err(status) }
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // Invalidating waits out the callbacks still running.
        unsafe { self.session.invalidate() };
        drop(unsafe { Box::from_raw(self.out) });
    }
}

fn create(width: i32, height: i32, out: *mut Out, low_latency: bool) -> Result<CFRetained<VTCompressionSession>, i32> {
    let t = unsafe { kCFBooleanTrue }.expect("kCFBooleanTrue");
    let mut keys = vec![unsafe { kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder }];
    if low_latency {
        keys.push(unsafe { kVTVideoEncoderSpecification_EnableLowLatencyRateControl });
    }
    let values = vec![t; keys.len()];
    let spec = CFDictionary::<CFString, CFBoolean>::from_slices(&keys, &values);
    let mut session: *mut VTCompressionSession = ptr::null_mut();
    let status = unsafe {
        VTCompressionSession::create(
            None,
            width,
            height,
            kCMVideoCodecType_HEVC,
            Some(spec.as_opaque()),
            None,
            None,
            Some(on_encoded),
            out.cast(),
            NonNull::from(&mut session),
        )
    };
    match NonNull::new(session) {
        Some(session) if status == 0 => Ok(unsafe { CFRetained::from_raw(session) }),
        _ => Err(status),
    }
}

fn configure(session: &VTCompressionSession) {
    let t: &CFType = unsafe { kCFBooleanTrue }.expect("kCFBooleanTrue");
    let f: &CFType = unsafe { objc2_core_foundation::kCFBooleanFalse }.expect("kCFBooleanFalse");
    let rate = CFNumber::new_i32(BITS_PER_SECOND);
    let fps = CFNumber::new_i32(FPS);
    let key_interval = CFNumber::new_i32(KEY_INTERVAL_SECONDS);
    let properties: [(&str, &CFString, &CFType); 6] = unsafe {
        [
            ("RealTime", kVTCompressionPropertyKey_RealTime, t),
            ("AllowFrameReordering", kVTCompressionPropertyKey_AllowFrameReordering, f),
            ("ProfileLevel", kVTCompressionPropertyKey_ProfileLevel, kVTProfileLevel_HEVC_Main_AutoLevel),
            ("AverageBitRate", kVTCompressionPropertyKey_AverageBitRate, &rate),
            ("ExpectedFrameRate", kVTCompressionPropertyKey_ExpectedFrameRate, &fps),
            ("MaxKeyFrameIntervalDuration", kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration, &key_interval),
        ]
    };
    for (name, key, value) in properties {
        let status = unsafe { VTSessionSetProperty(session, key, Some(value)) };
        if status != 0 {
            warn!(name, status, "remote view: the encoder refused a property");
        }
    }
    let status = unsafe { session.prepare_to_encode_frames() };
    if status != 0 {
        warn!(status, "remote view: the encoder would not prepare");
    }
}

unsafe extern "C-unwind" fn on_encoded(
    refcon: *mut c_void,
    _source: *mut c_void,
    status: i32,
    _flags: VTEncodeInfoFlags,
    sample: *mut CMSampleBuffer,
) {
    let out = unsafe { &*refcon.cast::<Out>() };
    let picture = match unsafe { sample.as_ref() } {
        Some(sample) if status == 0 => unsafe { picture(sample) },
        // A dropped frame comes here with no sample.
        _ => None,
    };
    match picture {
        Some(picture) => {
            let _ = out.tx.send(Msg::Picture(picture));
        }
        None => {
            if status != 0 {
                warn!(status, "remote view: the encoder failed a frame");
            }
            out.in_flight.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

unsafe fn picture(sample: &CMSampleBuffer) -> Option<Down> {
    let key = unsafe { is_key(sample) };
    let pts = unsafe { sample.presentation_time_stamp() };
    let pts_us = (pts.value as i128 * 1_000_000 / i128::from(pts.timescale.max(1))) as u64;
    let format = unsafe { sample.format_description() }?;
    let block = unsafe { sample.data_buffer() }?;
    let mut data = Vec::new();
    let header = unsafe { parameter_sets(&format, key.then_some(&mut data)) }?;
    annexb::push_sample(&mut data, unsafe { contiguous(&block) }?, header)?;
    Some(Down::Picture { key, pts_us, data })
}

unsafe fn is_key(sample: &CMSampleBuffer) -> bool {
    let Some(array) = (unsafe { sample.sample_attachments_array(false) }) else {
        return true;
    };
    if array.count() == 0 {
        return true;
    }
    let first = unsafe { array.value_at_index(0) }.cast::<CFDictionary>();
    let Some(first) = (unsafe { first.as_ref() }) else {
        return true;
    };
    let not_sync: *const CFString = unsafe { kCMSampleAttachmentKey_NotSync };
    !unsafe { first.contains_ptr_key(not_sync.cast()) }
}

/// The length of the sample's NAL headers; on a key picture also the
/// parameter sets, into `out`.
unsafe fn parameter_sets(format: &CMFormatDescription, mut out: Option<&mut Vec<u8>>) -> Option<usize> {
    let (mut count, mut header, mut index) = (0usize, 0i32, 0usize);
    loop {
        let (mut ptr, mut len) = (ptr::null::<u8>(), 0usize);
        let status = unsafe {
            CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(format, index, &mut ptr, &mut len, &mut count, &mut header)
        };
        if status != 0 {
            return None;
        }
        if let Some(out) = out.as_deref_mut() {
            annexb::push_nal(out, unsafe { std::slice::from_raw_parts(ptr, len) });
        }
        index += 1;
        if index >= count || out.is_none() {
            return usize::try_from(header).ok();
        }
    }
}

unsafe fn contiguous(block: &CMBlockBuffer) -> Option<&[u8]> {
    let (mut at, mut total, mut ptr) = (0usize, 0usize, ptr::null_mut::<c_char>());
    let status = unsafe { block.data_pointer(0, &mut at, &mut total, &mut ptr) };
    (status == 0 && at == total && !ptr.is_null()).then(|| unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), total) })
}
