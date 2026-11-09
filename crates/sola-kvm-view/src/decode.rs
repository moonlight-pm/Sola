//! HEVC to NV12 on the GPU's decoder. NVDEC's parser finds the pictures and
//! calls back on this thread: a sequence to set the decoder up, a picture to
//! decode, a picture to show. With no reordering and no display delay, the
//! picture pushed comes out before `push` returns.

use std::ffi::{c_int, c_uint, c_void};
use std::ptr;
use std::sync::Arc;

use tracing::info;

use crate::nvdec_sys::*;
use crate::pictures::{Frame, Pictures};

const DECODE_SURFACES: u8 = 8;
const OUTPUT_SURFACES: u64 = 2;

struct State {
    api: Api,
    lock: Handle,
    decoder: Handle,
    size: (u32, u32),
    pictures: Arc<Pictures>,
    /// The window needs telling about a picture.
    tell: bool,
    error: Option<String>,
}

/// Must stay on the thread that made it: the CUDA context is current there.
pub struct Decoder {
    state: Box<State>,
    context: Handle,
    parser: Handle,
}

fn check(status: c_int, what: &str) -> Result<(), String> {
    if status == 0 { Ok(()) } else { Err(format!("{what} failed (status {status})")) }
}

impl Decoder {
    pub fn new(pictures: Arc<Pictures>) -> Result<Self, String> {
        let api = Api::load()?;
        let (mut device, mut context, mut lock, mut parser) = (0, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
        unsafe {
            check((api.cu_init)(0), "cuInit")?;
            check((api.cu_device_get)(&mut device, 0), "cuDeviceGet")?;
            check((api.cu_ctx_create)(&mut context, 0, device), "cuCtxCreate")?;
            check((api.lock_create)(&mut lock, context), "cuvidCtxLockCreate")?;
        }
        let mut state = Box::new(State {
            api,
            lock,
            decoder: ptr::null_mut(),
            size: (0, 0),
            pictures,
            tell: false,
            error: None,
        });
        let mut params = ParserParams {
            codec: CODEC_HEVC,
            max_decode_surfaces: u32::from(DECODE_SURFACES),
            clock_rate: 1_000_000,
            error_threshold: 100,
            max_display_delay: 0,
            flags: 0,
            reserved1: [0; 4],
            user: ptr::from_mut(state.as_mut()).cast(),
            on_sequence: Some(on_sequence),
            on_decode: Some(on_decode),
            on_display: Some(on_display),
            on_operating_point: ptr::null_mut(),
            on_sei: ptr::null_mut(),
            reserved2: [ptr::null_mut(); 5],
            ext: ptr::null_mut(),
        };
        let created = unsafe { (state.api.parser_create)(&mut parser, &mut params) };
        let this = Self { state, context, parser };
        check(created, "cuvidCreateVideoParser")?;
        Ok(this)
    }

    /// Decode one picture. True when the window needs telling.
    pub fn push(&mut self, data: &[u8], pts_us: u64) -> Result<bool, String> {
        let mut packet = SourcePacket {
            flags: PKT_TIMESTAMP | PKT_ENDOFPICTURE,
            size: data.len() as _,
            payload: data.as_ptr(),
            timestamp: pts_us as i64,
        };
        self.state.tell = false;
        let status = unsafe { (self.state.api.parse)(self.parser, &mut packet) };
        if let Some(error) = self.state.error.take() {
            return Err(error);
        }
        check(status, "cuvidParseVideoData")?;
        Ok(self.state.tell)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        let api = &self.state.api;
        unsafe {
            if !self.parser.is_null() {
                (api.parser_destroy)(self.parser);
            }
            if !self.state.decoder.is_null() {
                (api.decoder_destroy)(self.state.decoder);
            }
            (api.lock_destroy)(self.state.lock);
            (api.cu_ctx_destroy)(self.context);
        }
    }
}

impl State {
    fn sequence(&mut self, format: &VideoFormat) -> Result<c_int, String> {
        let [left, top, right, bottom] = format.display;
        let size = ((right - left) as u32, (bottom - top) as u32);
        let surfaces = format.min_decode_surfaces.max(DECODE_SURFACES);
        if size == self.size && !self.decoder.is_null() {
            return Ok(c_int::from(surfaces));
        }
        if !self.decoder.is_null() {
            unsafe { (self.api.decoder_destroy)(self.decoder) };
            self.decoder = ptr::null_mut();
        }
        let area = Rect16 {
            left: left as i16,
            top: top as i16,
            right: right as i16,
            bottom: bottom as i16,
        };
        let mut info = DecodeCreateInfo {
            width: format.coded_width as _,
            height: format.coded_height as _,
            num_decode_surfaces: u64::from(surfaces) as _,
            codec: format.codec,
            chroma: format.chroma,
            creation_flags: CREATE_PREFER_CUVID,
            bit_depth_minus8: format.luma_minus8 as _,
            intra_only: 0,
            max_width: format.coded_width as _,
            max_height: format.coded_height as _,
            reserved1: 0,
            display_area: area,
            output_format: SURFACE_NV12,
            deinterlace: DEINTERLACE_WEAVE,
            target_width: size.0 as _,
            target_height: size.1 as _,
            num_output_surfaces: OUTPUT_SURFACES as _,
            lock: self.lock,
            target_rect: Rect16::default(),
            reserved2: [0; 5],
        };
        check(unsafe { (self.api.decoder_create)(&mut self.decoder, &mut info) }, "cuvidCreateDecoder")?;
        self.size = size;
        info!(width = size.0, height = size.1, surfaces, "remote view: decoder ready");
        Ok(c_int::from(surfaces))
    }

    fn display(&mut self, info: &DisplayInfo) -> Result<(), String> {
        let (mut device, mut pitch) = (0u64, 0 as c_uint);
        let mut params: ProcParams = unsafe { std::mem::zeroed() };
        params.progressive = info.progressive;
        params.top_field_first = info.top_field_first;
        check(
            unsafe { (self.api.map_frame)(self.decoder, info.picture_index, &mut device, &mut pitch, &mut params) },
            "cuvidMapVideoFrame",
        )?;
        let mut frame = self.pictures.spare();
        let copied = self.copy(device, pitch as usize, &mut frame);
        let unmapped = unsafe { (self.api.unmap_frame)(self.decoder, device) };
        copied?;
        check(unmapped, "cuvidUnmapVideoFrame")?;
        self.tell |= self.pictures.put(frame);
        Ok(())
    }

    fn copy(&self, device: u64, pitch: usize, frame: &mut Frame) -> Result<(), String> {
        let (width, height) = (self.size.0 as usize, self.size.1 as usize);
        frame.width = self.size.0;
        frame.height = self.size.1;
        frame.y.resize(width * height, 0);
        frame.uv.resize(width * height.div_ceil(2), 0);
        let plane = |src_y: usize, rows: usize, dst: &mut [u8]| {
            let copy = Copy2D {
                src_x: 0,
                src_y,
                src_type: MEMORY_DEVICE,
                src_host: ptr::null(),
                src_device: device,
                src_array: ptr::null_mut(),
                src_pitch: pitch,
                dst_x: 0,
                dst_y: 0,
                dst_type: MEMORY_HOST,
                dst_host: dst.as_mut_ptr().cast::<c_void>(),
                dst_device: 0,
                dst_array: ptr::null_mut(),
                dst_pitch: width,
                width,
                height: rows,
            };
            check(unsafe { (self.api.cu_memcpy2d)(&copy) }, "cuMemcpy2D")
        };
        plane(0, height, &mut frame.y)?;
        // The chroma plane follows the luma plane's `height` rows.
        plane(height, height.div_ceil(2), &mut frame.uv)
    }
}

/// Runs a parser callback, keeping an error for `push` to return. `ok` is
/// the callback's success value; 0 tells the parser it failed.
fn guarded(user: *mut c_void, body: impl FnOnce(&mut State) -> Result<c_int, String>) -> c_int {
    let state = unsafe { &mut *user.cast::<State>() };
    match body(state) {
        Ok(ok) => ok,
        Err(error) => {
            state.error = Some(error);
            0
        }
    }
}

unsafe extern "C" fn on_sequence(user: *mut c_void, format: *mut VideoFormat) -> c_int {
    guarded(user, |s| s.sequence(unsafe { &*format }))
}

unsafe extern "C" fn on_decode(user: *mut c_void, params: *mut c_void) -> c_int {
    guarded(user, |s| {
        check(unsafe { (s.api.decode_picture)(s.decoder, params) }, "cuvidDecodePicture")?;
        Ok(1)
    })
}

unsafe extern "C" fn on_display(user: *mut c_void, info: *mut DisplayInfo) -> c_int {
    guarded(user, |s| {
        // The parser ends a stream with no picture.
        if let Some(info) = unsafe { info.as_ref() } {
            s.display(info)?;
        }
        Ok(1)
    })
}
