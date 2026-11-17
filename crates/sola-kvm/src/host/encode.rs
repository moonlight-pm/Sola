//! HEVC on NVIDIA's encoder (NVENC) for a Linux host's screen: one frame
//! in, one picture out, synchronously, with no reordering, no lookahead and
//! CBR over a one-frame VBV, as the Mac's encoder is tuned.
//!
//! Frames are copied into NVENC's own input buffer as packed RGB, and NVENC
//! converts them to 4:2:0 itself. Its conversion follows the VUI: the matrix
//! and range the stream says it has are the ones it converts with, and with
//! no VUI it uses BT.601. So the VUI here (BT.709, limited range) is what
//! makes the pictures match the viewer's shader; decoded solid colours come
//! back as BT.709 limited to the code value.

use std::ffi::{CStr, c_int};
use std::ptr;

use tracing::{info, warn};

use super::nvenc::*;

/// The fastest preset; with ultra-low-latency tuning it encodes 1440p in
/// about 5 ms. P2 and P3 took 2–2.5 ms more for under 1 dB.
const PRESET: Guid = PRESET_P1;
const PRESET_NAME: &str = "P1";
const TUNING: u32 = TUNING_ULTRA_LOW_LATENCY;

/// One captured frame, as the compositor wrote it into wl_shm memory.
pub struct Frame<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    /// Bytes from one row to the next.
    pub stride: u32,
    /// The wl_shm format code: 0 ARGB8888, 1 XRGB8888, otherwise a DRM fourcc
    /// (XBGR8888, ABGR8888, BGR888 'BG24', RGB888 'RG24').
    pub format: u32,
    /// The rows come bottom first (screencopy's y_invert flag).
    pub y_invert: bool,
}

pub struct Encoded {
    /// An IDR picture, with its VPS/SPS/PPS in front.
    pub key: bool,
    /// Annex B (start codes), one access unit.
    pub data: Vec<u8>,
}

const ARGB8888: u32 = 0;
const XRGB8888: u32 = 1;
const XBGR8888: u32 = fourcc(b"XB24");
const ABGR8888: u32 = fourcc(b"AB24");
const BGR888: u32 = fourcc(b"BG24");
const RGB888: u32 = fourcc(b"RG24");

const fn fourcc(code: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*code)
}

/// How a frame's pixels go into NVENC's input buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Input {
    /// NVENC's packed format: ARGB is B G R A in memory, ABGR is R G B A.
    buffer: u32,
    /// Bytes per pixel in the frame: 4 are copied as they are, 3 are
    /// widened with an opaque alpha.
    bytes: usize,
}

fn input_for(format: u32) -> Option<Input> {
    let (buffer, bytes) = match format {
        ARGB8888 | XRGB8888 => (BUFFER_FORMAT_ARGB, 4),
        XBGR8888 | ABGR8888 => (BUFFER_FORMAT_ABGR, 4),
        // BGR888 is R G B in memory, RGB888 B G R.
        BGR888 => (BUFFER_FORMAT_ABGR, 3),
        RGB888 => (BUFFER_FORMAT_ARGB, 3),
        _ => return None,
    };
    Some(Input { buffer, bytes })
}

fn buffer_name(buffer: u32) -> &'static str {
    if buffer == BUFFER_FORMAT_ABGR { "ABGR" } else { "ARGB" }
}

/// The even size inside `width`×`height`: 4:2:0 has a chroma sample for
/// each 2×2 block, so an odd last row or column is left out.
fn even(width: u32, height: u32) -> (u32, u32) {
    (width & !1, height & !1)
}

/// Whether `frame.data` holds every row the copy reads.
fn check_fits(frame: &Frame, bytes: usize) -> Result<(), String> {
    let row = frame.width as usize * bytes;
    let stride = frame.stride as usize;
    let needed = match frame.height as usize {
        0 => 0,
        rows => stride * (rows - 1) + row,
    };
    if stride < row || frame.data.len() < needed {
        return Err(format!(
            "a {}x{} frame with stride {} needs {needed} bytes and rows of at least {row}; it has {}",
            frame.width,
            frame.height,
            frame.stride,
            frame.data.len()
        ));
    }
    Ok(())
}

/// Copies the top-left `width`×`height` pixels of `frame` into `dst` as
/// 4-byte pixels, top row first, rows `pitch` bytes apart.
fn copy_rows(frame: &Frame, bytes: usize, dst: &mut [u8], pitch: usize, width: usize, height: usize) {
    let stride = frame.stride as usize;
    for (y, out) in dst.chunks_mut(pitch).take(height).enumerate() {
        let row = if frame.y_invert { frame.height as usize - 1 - y } else { y };
        let src = &frame.data[row * stride..][..width * bytes];
        let out = &mut out[..width * 4];
        if bytes == 4 {
            out.copy_from_slice(src);
        } else {
            for (o, i) in out.as_chunks_mut::<4>().0.iter_mut().zip(src.as_chunks::<3>().0) {
                *o = [i[0], i[1], i[2], 0xff];
            }
        }
    }
}

/// CBR at `kbps`, with a VBV of one frame's bits so a picture never waits
/// behind a burst.
fn set_rate(rc: &mut RcParams, fps: u32, kbps: u32) {
    let bits = kbps.saturating_mul(1000);
    rc.rate_control_mode = RC_CBR;
    rc.average_bit_rate = bits;
    rc.max_bit_rate = bits;
    rc.vbv_buffer_size = bits / fps;
    rc.vbv_initial_delay = bits / fps;
}

/// The preset's config, made low-latency HEVC Main with BT.709 limited VUI.
fn configure(config: &mut Config, fps: u32, kbps: u32) {
    config.profile = PROFILE_HEVC_MAIN;
    config.gop_length = INFINITE_GOP;
    config.frame_interval_p = 1;
    set_rate(&mut config.rc, fps, kbps);
    // A quarter-size first pass, as the preset has it: without it, scrolling
    // text at 2 Mbps came out at 5–7 times the one-frame budget; with it,
    // within 1.6 times, for about a millisecond more at 1440p.
    config.rc.multi_pass = MULTI_PASS_QUARTER_RESOLUTION;
    config.rc.flags = (config.rc.flags & !RC_ENABLE_LOOKAHEAD) | RC_ZERO_REORDER_DELAY;
    let hevc = &mut config.hevc;
    hevc.level = LEVEL_AUTOSELECT;
    hevc.idr_period = INFINITE_GOP;
    hevc.flags &= !(HEVC_CHROMA_FORMAT_MASK | HEVC_ENABLE_FILLER_DATA);
    hevc.flags |= HEVC_REPEAT_SPSPPS | (1 << HEVC_CHROMA_FORMAT_SHIFT);
    hevc.input_bit_depth = BIT_DEPTH_8;
    hevc.output_bit_depth = BIT_DEPTH_8;
    let vui = &mut hevc.vui;
    vui.video_signal_type_present = 1;
    vui.video_format = VUI_VIDEO_FORMAT_UNSPECIFIED;
    vui.video_full_range = 0;
    vui.colour_description_present = 1;
    vui.colour_primaries = VUI_PRIMARIES_BT709;
    vui.transfer_characteristics = VUI_TRANSFER_SRGB;
    vui.colour_matrix = VUI_MATRIX_BT709;
}

fn cuda(status: c_int, what: &str) -> Result<(), String> {
    if status == 0 { Ok(()) } else { Err(format!("{what} failed (CUDA error {status})")) }
}

/// The encoder's CUDA context, current on this thread until dropped.
struct Current {
    pop: unsafe extern "C" fn(*mut Handle) -> c_int,
}

impl Drop for Current {
    fn drop(&mut self) {
        let mut popped = ptr::null_mut();
        unsafe { (self.pop)(&mut popped) };
    }
}

pub struct Encoder {
    api: Api,
    context: Handle,
    session: Handle,
    output: Handle,
    /// Made for the first frame, and again if a frame's format needs the
    /// other packing.
    input: Handle,
    input_format: u32,
    input_pitch: u32,
    /// Frames come at this size; the pictures are the even size inside it.
    frame_size: (u32, u32),
    size: (u32, u32),
    fps: u32,
    /// Boxed: `params` points at it.
    config: Box<Config>,
    params: InitializeParams,
    frames: u64,
}

// The handles are used only through `&mut self`, with the CUDA context
// pushed onto the calling thread for each call; NVENC sessions may move
// between threads that way.
unsafe impl Send for Encoder {}

impl Encoder {
    /// HEVC Main (8-bit 4:2:0) at `width`×`height` pixels, `fps` frames a second, CBR at `kbps`.
    pub fn new(width: u32, height: u32, fps: u32, kbps: u32) -> Result<Self, String> {
        let size = even(width, height);
        if size.0 == 0 || size.1 == 0 || fps == 0 || kbps == 0 {
            return Err(format!("cannot encode {width}x{height} at {fps} fps and {kbps} kbps"));
        }
        let api = Api::load()?;
        let (mut device, mut context) = (0, ptr::null_mut());
        unsafe {
            cuda((api.cu_init)(0), "cuInit")?;
            cuda((api.cu_device_get)(&mut device, 0), "cuDeviceGet")?;
            cuda((api.cu_ctx_create)(&mut context, 0, device), "cuCtxCreate")?;
        }
        let mut this = Self {
            api,
            context,
            session: ptr::null_mut(),
            output: ptr::null_mut(),
            input: ptr::null_mut(),
            input_format: 0,
            input_pitch: 0,
            frame_size: (width, height),
            size,
            fps,
            // SAFETY: integers and pointers, all zero.
            config: Box::new(unsafe { std::mem::zeroed() }),
            params: unsafe { std::mem::zeroed() },
            frames: 0,
        };
        // cuCtxCreate left the context current here; `current` pushes it
        // around each call instead, on whichever thread has the encoder.
        let mut popped = ptr::null_mut();
        cuda(unsafe { (this.api.cu_ctx_pop)(&mut popped) }, "cuCtxPopCurrent")?;
        this.open(kbps)?;
        let max = this.api.max_version;
        info!(
            width = size.0,
            height = size.1,
            frame_width = width,
            frame_height = height,
            fps,
            kbps,
            api = %format!("{API_MAJOR}.{API_MINOR}"),
            driver_api = %format!("{}.{}", max >> 4, max & 0xf),
            preset = PRESET_NAME,
            input = "packed RGB (ARGB or ABGR by frame format), BT.709 limited by NVENC",
            "remote view: NVENC HEVC encoder open"
        );
        Ok(this)
    }

    /// Whether `format` (a Frame::format) can be encoded.
    pub fn supports(format: u32) -> bool {
        input_for(format).is_some()
    }

    /// Change the bitrate without a new key picture (nvEncReconfigureEncoder).
    pub fn set_kbps(&mut self, kbps: u32) -> Result<(), String> {
        if kbps == 0 {
            return Err("cannot encode at 0 kbps".into());
        }
        let _current = self.current()?;
        let before = self.config.rc;
        set_rate(&mut self.config.rc, self.fps, kbps);
        // SAFETY: integers and pointers, all zero.
        let mut params: ReconfigureParams = unsafe { std::mem::zeroed() };
        params.version = RECONFIGURE_PARAMS_VER;
        params.params = self.params;
        params.params.encode_config = &mut *self.config;
        let status = unsafe { (self.api.session.reconfigure_encoder)(self.session, &mut params) };
        if let Err(e) = self.check(status, &format!("nvEncReconfigureEncoder to {kbps} kbps")) {
            self.config.rc = before;
            return Err(e);
        }
        Ok(())
    }

    /// Encode one frame, synchronously; `key` forces an IDR with parameter sets.
    pub fn encode(&mut self, frame: &Frame, key: bool, pts_us: u64) -> Result<Encoded, String> {
        if (frame.width, frame.height) != self.frame_size {
            let (width, height) = self.frame_size;
            return Err(format!("a {}x{} frame came to the encoder for {width}x{height}", frame.width, frame.height));
        }
        let input =
            input_for(frame.format).ok_or_else(|| format!("frame format {:#010x} cannot be encoded", frame.format))?;
        check_fits(frame, input.bytes)?;
        let _current = self.current()?;
        if self.input.is_null() || self.input_format != input.buffer {
            self.make_input(input.buffer)?;
        }
        self.fill(frame, input)?;
        // The first picture is always a key one: nothing decodes without it.
        let key = key || self.frames == 0;
        // SAFETY: integers and pointers, all zero.
        let mut picture: PicParams = unsafe { std::mem::zeroed() };
        picture.version = PIC_PARAMS_VER;
        picture.input_width = self.size.0;
        picture.input_height = self.size.1;
        picture.input_pitch = self.input_pitch;
        picture.encode_pic_flags = if key { PIC_FLAG_FORCE_IDR | PIC_FLAG_OUTPUT_SPSPPS } else { 0 };
        picture.frame_idx = self.frames as u32;
        picture.input_time_stamp = pts_us;
        picture.input_buffer = self.input;
        picture.output_bitstream = self.output;
        picture.buffer_format = self.input_format;
        picture.picture_struct = PIC_STRUCT_FRAME;
        let status = unsafe { (self.api.session.encode_picture)(self.session, &mut picture) };
        self.check(status, "nvEncEncodePicture")?;
        self.frames += 1;
        let picture = self.take_picture()?;
        if key && !picture.key {
            warn!(frame = self.frames, "remote view: NVENC made no key picture where one was asked for");
        }
        Ok(picture)
    }

    fn open(&mut self, kbps: u32) -> Result<(), String> {
        let _current = self.current()?;
        let mut open = OpenSessionParams {
            version: OPEN_SESSION_VER,
            device_type: DEVICE_TYPE_CUDA,
            device: self.context,
            reserved: ptr::null_mut(),
            api_version: API_VERSION,
            reserved1: [0; 253],
            reserved2: [ptr::null_mut(); 64],
        };
        let status = unsafe { (self.api.session.open_encode_session_ex)(&mut open, &mut self.session) };
        self.check(status, "nvEncOpenEncodeSessionEx")?;

        // SAFETY: integers and pointers, all zero.
        let mut preset: Box<PresetConfig> = Box::new(unsafe { std::mem::zeroed() });
        preset.version = PRESET_CONFIG_VER;
        preset.config.version = CONFIG_VER;
        let status =
            unsafe { (self.api.session.preset_config_ex)(self.session, CODEC_HEVC, PRESET, TUNING, &mut *preset) };
        self.check(status, &format!("nvEncGetEncodePresetConfigEx (HEVC, {PRESET_NAME}, ultra low latency)"))?;
        *self.config = preset.config;
        configure(&mut self.config, self.fps, kbps);

        let params = &mut self.params;
        params.version = INITIALIZE_PARAMS_VER;
        params.encode_guid = CODEC_HEVC;
        params.preset_guid = PRESET;
        params.encode_width = self.size.0;
        params.encode_height = self.size.1;
        params.dar_width = self.size.0;
        params.dar_height = self.size.1;
        params.frame_rate_num = self.fps;
        params.frame_rate_den = 1;
        params.enable_ptd = 1;
        params.encode_config = &mut *self.config;
        params.tuning_info = TUNING;
        let status = unsafe { (self.api.session.initialize_encoder)(self.session, &mut self.params) };
        self.check(status, &format!("nvEncInitializeEncoder ({}x{} HEVC)", self.size.0, self.size.1))?;

        // SAFETY: integers and pointers, all zero.
        let mut output: CreateBitstreamBuffer = unsafe { std::mem::zeroed() };
        output.version = CREATE_BITSTREAM_BUFFER_VER;
        let status = unsafe { (self.api.session.create_bitstream_buffer)(self.session, &mut output) };
        self.check(status, "nvEncCreateBitstreamBuffer")?;
        self.output = output.bitstream_buffer;
        Ok(())
    }

    fn make_input(&mut self, buffer: u32) -> Result<(), String> {
        if !self.input.is_null() {
            let status = unsafe { (self.api.session.destroy_input_buffer)(self.session, self.input) };
            self.input = ptr::null_mut();
            self.check(status, "nvEncDestroyInputBuffer")?;
        }
        // SAFETY: integers and pointers, all zero.
        let mut input: CreateInputBuffer = unsafe { std::mem::zeroed() };
        input.version = CREATE_INPUT_BUFFER_VER;
        input.width = self.size.0;
        input.height = self.size.1;
        input.buffer_format = buffer;
        let status = unsafe { (self.api.session.create_input_buffer)(self.session, &mut input) };
        self.check(status, &format!("nvEncCreateInputBuffer ({})", buffer_name(buffer)))?;
        self.input = input.input_buffer;
        self.input_format = buffer;
        info!(format = buffer_name(buffer), "remote view: NVENC input buffer made");
        Ok(())
    }

    /// Copies `frame` into the input buffer.
    fn fill(&mut self, frame: &Frame, input: Input) -> Result<(), String> {
        // SAFETY: integers and pointers, all zero.
        let mut lock: LockInputBuffer = unsafe { std::mem::zeroed() };
        lock.version = LOCK_INPUT_BUFFER_VER;
        lock.input_buffer = self.input;
        let status = unsafe { (self.api.session.lock_input_buffer)(self.session, &mut lock) };
        self.check(status, "nvEncLockInputBuffer")?;
        let (width, height) = (self.size.0 as usize, self.size.1 as usize);
        let pitch = lock.pitch as usize;
        let copied = if lock.buffer_data.is_null() || pitch < width * 4 {
            Err(format!("NVENC's input buffer came back with pitch {pitch} for {width} pixels"))
        } else {
            // SAFETY: the locked buffer holds `height` rows of `pitch` bytes.
            let dst = unsafe { std::slice::from_raw_parts_mut(lock.buffer_data, pitch * height) };
            copy_rows(frame, input.bytes, dst, pitch, width, height);
            self.input_pitch = lock.pitch;
            Ok(())
        };
        let status = unsafe { (self.api.session.unlock_input_buffer)(self.session, self.input) };
        copied?;
        self.check(status, "nvEncUnlockInputBuffer")
    }

    fn take_picture(&mut self) -> Result<Encoded, String> {
        // SAFETY: integers and pointers, all zero.
        let mut lock: LockBitstream = unsafe { std::mem::zeroed() };
        lock.version = LOCK_BITSTREAM_VER;
        lock.output_bitstream = self.output;
        let status = unsafe { (self.api.session.lock_bitstream)(self.session, &mut lock) };
        self.check(status, "nvEncLockBitstream")?;
        let data = if lock.bitstream.is_null() {
            Vec::new()
        } else {
            // SAFETY: NVENC wrote `bitstream_size` bytes there, ours until unlocked.
            unsafe { std::slice::from_raw_parts(lock.bitstream, lock.bitstream_size as usize) }.to_vec()
        };
        let status = unsafe { (self.api.session.unlock_bitstream)(self.session, self.output) };
        self.check(status, "nvEncUnlockBitstream")?;
        if data.is_empty() {
            return Err("NVENC returned an empty picture".into());
        }
        Ok(Encoded { key: lock.picture_type == PIC_TYPE_IDR, data })
    }

    fn current(&self) -> Result<Current, String> {
        cuda(unsafe { (self.api.cu_ctx_push)(self.context) }, "cuCtxPushCurrent")?;
        Ok(Current { pop: self.api.cu_ctx_pop })
    }

    /// `what` failed with `status`, in NVENC's words when it has some.
    fn check(&self, status: Status, what: &str) -> Result<(), String> {
        if status == SUCCESS {
            return Ok(());
        }
        let mut error = format!("{what} failed: {}", status_name(status));
        if !self.session.is_null() {
            let text = unsafe { (self.api.session.last_error_string)(self.session) };
            if !text.is_null() {
                let text = unsafe { CStr::from_ptr(text) }.to_string_lossy();
                if !text.is_empty() {
                    error.push_str(": ");
                    error.push_str(&text);
                }
            }
        }
        Err(error)
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        match self.current() {
            Ok(_current) => {
                let session = &self.api.session;
                unsafe {
                    if !self.input.is_null() {
                        (session.destroy_input_buffer)(self.session, self.input);
                    }
                    if !self.output.is_null() {
                        (session.destroy_bitstream_buffer)(self.session, self.output);
                    }
                    if !self.session.is_null() {
                        (session.destroy_encoder)(self.session);
                    }
                }
            }
            Err(e) => warn!(error = %e, "remote view: could not close the NVENC session"),
        }
        unsafe { (self.api.cu_ctx_destroy)(self.context) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(data: &[u8], width: u32, height: u32, stride: u32, format: u32, y_invert: bool) -> Frame<'_> {
        Frame { data, width, height, stride, format, y_invert }
    }

    #[test]
    fn encoders_can_move_between_threads() {
        fn send<T: Send>() {}
        send::<Encoder>();
    }

    #[test]
    fn formats_map_to_nvenc_packings() {
        let argb = Input { buffer: BUFFER_FORMAT_ARGB, bytes: 4 };
        let abgr = Input { buffer: BUFFER_FORMAT_ABGR, bytes: 4 };
        assert_eq!(input_for(0), Some(argb));
        assert_eq!(input_for(1), Some(argb));
        assert_eq!(input_for(fourcc(b"XB24")), Some(abgr));
        assert_eq!(input_for(fourcc(b"AB24")), Some(abgr));
        assert_eq!(input_for(fourcc(b"BG24")), Some(Input { buffer: BUFFER_FORMAT_ABGR, bytes: 3 }));
        assert_eq!(input_for(fourcc(b"RG24")), Some(Input { buffer: BUFFER_FORMAT_ARGB, bytes: 3 }));
        assert_eq!(fourcc(b"XB24"), 0x3432_4258);
        assert!(Encoder::supports(1));
        assert!(!Encoder::supports(fourcc(b"NV12")));
        assert!(!Encoder::supports(fourcc(b"AR30")));
    }

    #[test]
    fn odd_sizes_lose_their_last_row_and_column() {
        assert_eq!(even(2560, 1440), (2560, 1440));
        assert_eq!(even(1921, 1081), (1920, 1080));
        assert_eq!(even(1, 1), (0, 0));
    }

    #[test]
    fn copy_follows_the_stride_and_leaves_the_odd_edge() {
        // 3×3 ARGB, each pixel's bytes its row and column, 4 bytes of
        // padding at each row's end.
        let mut data = vec![0xee; 16 * 3];
        for y in 0..3 {
            for x in 0..3 {
                data[y * 16 + x * 4..][..4].copy_from_slice(&[y as u8, x as u8, 0, 0xff]);
            }
        }
        let mut dst = vec![0; 12 * 2];
        copy_rows(&frame(&data, 3, 3, 16, 0, false), 4, &mut dst, 12, 2, 2);
        assert_eq!(&dst[..8], &[0, 0, 0, 0xff, 0, 1, 0, 0xff]);
        assert_eq!(&dst[8..12], &[0, 0, 0, 0]);
        assert_eq!(&dst[12..20], &[1, 0, 0, 0xff, 1, 1, 0, 0xff]);
    }

    #[test]
    fn inverted_frames_are_read_bottom_row_first() {
        // Rows in memory: 0, 1, 2; on screen: 2, 1, 0. The odd row left out
        // is the screen's last, memory row 0.
        let data: Vec<u8> = (0..3u8).flat_map(|y| [y; 8]).collect();
        let mut dst = vec![0; 8 * 2];
        copy_rows(&frame(&data, 2, 3, 8, 1, true), 4, &mut dst, 8, 2, 2);
        assert_eq!(dst, [[2u8; 8], [1u8; 8]].concat());
    }

    #[test]
    fn three_byte_pixels_are_widened_opaque() {
        let data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let mut dst = vec![0; 16];
        copy_rows(&frame(&data, 2, 2, 6, fourcc(b"BG24"), false), 3, &mut dst, 8, 2, 2);
        assert_eq!(dst, [1, 2, 3, 0xff, 4, 5, 6, 0xff, 7, 8, 9, 0xff, 10, 11, 12, 0xff]);
    }

    #[test]
    fn short_or_narrow_frames_are_refused() {
        let data = vec![0; 4 * 4 * 4];
        assert!(check_fits(&frame(&data, 4, 4, 16, 0, false), 4).is_ok());
        // The last row needs only its pixels, not a whole stride.
        assert!(check_fits(&frame(&data[..16 * 3 + 16], 4, 4, 16, 0, false), 4).is_ok());
        assert!(check_fits(&frame(&data[..16 * 3 + 15], 4, 4, 16, 0, false), 4).is_err());
        assert!(check_fits(&frame(&data, 4, 4, 12, 0, false), 4).is_err());
    }

    #[test]
    fn rate_is_one_frame_of_vbv() {
        let mut rc: RcParams = unsafe { std::mem::zeroed() };
        set_rate(&mut rc, 60, 20_000);
        assert_eq!((rc.rate_control_mode, rc.average_bit_rate, rc.vbv_buffer_size), (RC_CBR, 20_000_000, 333_333));
    }

    /// NAL unit types in an Annex B access unit.
    fn nal_types(data: &[u8]) -> Vec<u8> {
        let mut types = Vec::new();
        let mut i = 0;
        while i + 3 < data.len() {
            if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
                types.push((data[i + 3] >> 1) & 0x3f);
                i += 3;
            } else {
                i += 1;
            }
        }
        types
    }

    /// A scrolling field of 4×4 blocks of noise: `pattern(k)` is the frame
    /// `k` steps along, read through the stride from a wider buffer.
    struct Pattern {
        data: Vec<u8>,
        width: u32,
        height: u32,
        stride: u32,
    }

    impl Pattern {
        const STEP: u32 = 8;

        fn new(width: u32, height: u32, frames: u32) -> Self {
            let wide = width + Self::STEP * frames;
            let mut data = vec![0u8; wide as usize * 4 * height as usize];
            for (y, row) in data.chunks_exact_mut(wide as usize * 4).enumerate() {
                for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    let h = ((x / 4) as u32).wrapping_mul(0x9e37_79b1) ^ ((y / 4) as u32).wrapping_mul(0x85eb_ca6b);
                    let h = h ^ (h >> 15);
                    *px = [h as u8, (h >> 8) as u8, (h >> 16) as u8, 0xff];
                }
            }
            Self { data, width, height, stride: wide * 4 }
        }

        fn frame(&self, k: u32) -> Frame<'_> {
            let at = (k * Self::STEP * 4) as usize;
            frame(&self.data[at..], self.width, self.height, self.stride, 1, false)
        }
    }

    #[test]
    #[ignore = "needs an NVIDIA GPU"]
    fn encodes_a_moving_picture_at_1440p() {
        let pattern = Pattern::new(2560, 1440, 120);
        let mut encoder = Encoder::new(2560, 1440, 60, 20_000).expect("encoder");
        let mut sizes = Vec::new();
        for k in 0..120 {
            if k == 60 {
                encoder.set_kbps(2000).expect("set_kbps");
            }
            let key = k == 100;
            let picture = encoder.encode(&pattern.frame(k), key, u64::from(k) * 16_667).expect("encode");
            let types = nal_types(&picture.data);
            if k == 0 || k == 100 {
                assert!(picture.key, "picture {k} is a key one");
                assert!(picture.data.starts_with(&[0, 0, 0, 1]) || picture.data.starts_with(&[0, 0, 1]));
                for t in [32, 33, 34] {
                    assert!(types.contains(&t), "picture {k} has NAL type {t}: {types:?}");
                }
                assert!(types.iter().any(|&t| t == 19 || t == 20), "picture {k} has an IDR slice: {types:?}");
            } else {
                assert!(!picture.key, "picture {k} is not a key one");
                assert!(!types.contains(&33), "picture {k} has no SPS: {types:?}");
            }
            sizes.push(picture.data.len());
        }
        let mean = |range: std::ops::Range<usize>| sizes[range.clone()].iter().sum::<usize>() / range.len();
        let (high, low) = (mean(10..60), mean(70..100));
        eprintln!("mean picture at 20 Mbps {high} bytes, at 2 Mbps {low} bytes; first {}", sizes[0]);
        assert!(low * 3 < high, "2 Mbps pictures ({low}) are well under 20 Mbps ones ({high})");
    }

    #[test]
    #[ignore = "needs an NVIDIA GPU"]
    fn encode_time_per_frame() {
        for (width, height) in [(2560, 1440), (1920, 1080)] {
            let pattern = Pattern::new(width, height, 120);
            let mut encoder = Encoder::new(width, height, 60, 20_000).expect("encoder");
            let mut times = Vec::new();
            for k in 0..120 {
                let start = std::time::Instant::now();
                encoder.encode(&pattern.frame(k), false, 0).expect("encode");
                times.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            let first = times.remove(0);
            times.sort_by(f64::total_cmp);
            let mean = times.iter().sum::<f64>() / times.len() as f64;
            eprintln!(
                "{width}x{height}: first {first:.2} ms, then mean {mean:.2} ms, median {:.2}, max {:.2}",
                times[times.len() / 2],
                times[times.len() - 1]
            );
        }
    }
}
