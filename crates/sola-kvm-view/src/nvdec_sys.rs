//! The parts of NVDEC (`libnvcuvid.so.1`) and CUDA (`libcuda.so.1`) the
//! decoder calls, loaded at run time. Layouts are from NVIDIA's headers
//! (FFmpeg's `nv-codec-headers`); `tests` pin their sizes.

use std::ffi::{c_int, c_uint, c_ulong, c_void};

use libloading::Library;

pub const CODEC_HEVC: c_int = 8;
pub const SURFACE_NV12: c_int = 0;
pub const DEINTERLACE_WEAVE: c_int = 0;
pub const CREATE_PREFER_CUVID: c_ulong = 0x04;
pub const PKT_TIMESTAMP: c_ulong = 0x02;
pub const PKT_ENDOFPICTURE: c_ulong = 0x08;
pub const MEMORY_HOST: c_int = 1;
pub const MEMORY_DEVICE: c_int = 2;

pub type Handle = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect16 {
    pub left: i16,
    pub top: i16,
    pub right: i16,
    pub bottom: i16,
}

#[repr(C)]
pub struct DecodeCreateInfo {
    pub width: c_ulong,
    pub height: c_ulong,
    pub num_decode_surfaces: c_ulong,
    pub codec: c_int,
    pub chroma: c_int,
    pub creation_flags: c_ulong,
    pub bit_depth_minus8: c_ulong,
    pub intra_only: c_ulong,
    pub max_width: c_ulong,
    pub max_height: c_ulong,
    pub reserved1: c_ulong,
    pub display_area: Rect16,
    pub output_format: c_int,
    pub deinterlace: c_int,
    pub target_width: c_ulong,
    pub target_height: c_ulong,
    pub num_output_surfaces: c_ulong,
    pub lock: Handle,
    pub target_rect: Rect16,
    pub reserved2: [c_ulong; 5],
}

/// The start of `CUVIDEOFORMAT`, which NVDEC owns and this only reads.
#[repr(C)]
pub struct VideoFormat {
    pub codec: c_int,
    pub frame_rate: [c_uint; 2],
    pub progressive: u8,
    pub luma_minus8: u8,
    pub chroma_minus8: u8,
    pub min_decode_surfaces: u8,
    pub coded_width: c_uint,
    pub coded_height: c_uint,
    pub display: [c_int; 4],
    pub chroma: c_int,
}

#[repr(C)]
pub struct DisplayInfo {
    pub picture_index: c_int,
    pub progressive: c_int,
    pub top_field_first: c_int,
    pub repeat_first_field: c_int,
    pub timestamp: i64,
}

#[repr(C)]
pub struct SourcePacket {
    pub flags: c_ulong,
    pub size: c_ulong,
    pub payload: *const u8,
    pub timestamp: i64,
}

pub type SequenceCb = unsafe extern "C" fn(*mut c_void, *mut VideoFormat) -> c_int;
pub type DecodeCb = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
pub type DisplayCb = unsafe extern "C" fn(*mut c_void, *mut DisplayInfo) -> c_int;

#[repr(C)]
pub struct ParserParams {
    pub codec: c_int,
    pub max_decode_surfaces: c_uint,
    pub clock_rate: c_uint,
    pub error_threshold: c_uint,
    pub max_display_delay: c_uint,
    pub flags: c_uint,
    pub reserved1: [c_uint; 4],
    pub user: *mut c_void,
    pub on_sequence: Option<SequenceCb>,
    pub on_decode: Option<DecodeCb>,
    pub on_display: Option<DisplayCb>,
    pub on_operating_point: *mut c_void,
    pub on_sei: *mut c_void,
    pub reserved2: [*mut c_void; 5],
    pub ext: *mut c_void,
}

#[repr(C)]
pub struct ProcParams {
    pub progressive: c_int,
    pub second_field: c_int,
    pub top_field_first: c_int,
    pub unpaired_field: c_int,
    pub reserved: [c_uint; 2],
    pub raw_input: u64,
    pub raw_input_pitch: c_uint,
    pub raw_input_format: c_uint,
    pub raw_output: u64,
    pub raw_output_pitch: c_uint,
    pub reserved1: c_uint,
    pub output_stream: *mut c_void,
    pub reserved2: [c_uint; 46],
    pub histogram: *mut u64,
    pub ext: *mut c_void,
}

#[repr(C)]
pub struct Copy2D {
    pub src_x: usize,
    pub src_y: usize,
    pub src_type: c_int,
    pub src_host: *const c_void,
    pub src_device: u64,
    pub src_array: *mut c_void,
    pub src_pitch: usize,
    pub dst_x: usize,
    pub dst_y: usize,
    pub dst_type: c_int,
    pub dst_host: *mut c_void,
    pub dst_device: u64,
    pub dst_array: *mut c_void,
    pub dst_pitch: usize,
    pub width: usize,
    pub height: usize,
}

macro_rules! functions {
    ($($field:ident = $symbol:literal : fn($($arg:ty),*);)*) => {
        /// The loaded entry points; the libraries stay open beside them.
        pub struct Api {
            $(pub $field: unsafe extern "C" fn($($arg),*) -> c_int,)*
            _libs: [Library; 2],
        }

        impl Api {
            pub fn load() -> Result<Self, String> {
                let open = |name: &str| unsafe { Library::new(name) }
                    .map_err(|e| format!("{name} would not load ({e}); this needs an NVIDIA GPU"));
                let (cuda, cuvid) = (open("libcuda.so.1")?, open("libnvcuvid.so.1")?);
                $(
                    let $field = unsafe {
                        let lib = if $symbol.starts_with("cuvid") { &cuvid } else { &cuda };
                        *lib.get::<unsafe extern "C" fn($($arg),*) -> c_int>($symbol.as_bytes())
                            .map_err(|e| format!("{}: {e}", $symbol))?
                    };
                )*
                Ok(Self { $($field,)* _libs: [cuda, cuvid] })
            }
        }
    };
}

functions! {
    cu_init = "cuInit": fn(c_uint);
    cu_device_get = "cuDeviceGet": fn(*mut c_int, c_int);
    cu_ctx_create = "cuCtxCreate_v2": fn(*mut Handle, c_uint, c_int);
    cu_ctx_destroy = "cuCtxDestroy_v2": fn(Handle);
    cu_memcpy2d = "cuMemcpy2D_v2": fn(*const Copy2D);
    lock_create = "cuvidCtxLockCreate": fn(*mut Handle, Handle);
    lock_destroy = "cuvidCtxLockDestroy": fn(Handle);
    parser_create = "cuvidCreateVideoParser": fn(*mut Handle, *mut ParserParams);
    parser_destroy = "cuvidDestroyVideoParser": fn(Handle);
    parse = "cuvidParseVideoData": fn(Handle, *mut SourcePacket);
    decoder_create = "cuvidCreateDecoder": fn(*mut Handle, *mut DecodeCreateInfo);
    decoder_destroy = "cuvidDestroyDecoder": fn(Handle);
    decode_picture = "cuvidDecodePicture": fn(Handle, *mut c_void);
    map_frame = "cuvidMapVideoFrame64": fn(Handle, c_int, *mut u64, *mut c_uint, *mut ProcParams);
    unmap_frame = "cuvidUnmapVideoFrame64": fn(Handle, u64);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn layouts_match_the_headers() {
        assert_eq!(size_of::<SourcePacket>(), 32);
        assert_eq!(size_of::<DisplayInfo>(), 24);
        assert_eq!(size_of::<Copy2D>(), 128);
        assert_eq!(size_of::<ProcParams>(), 264);
        assert_eq!(offset_of!(ParserParams, user), 40);
        assert_eq!(size_of::<ParserParams>(), 136);
        assert_eq!(offset_of!(DecodeCreateInfo, display_area), 80);
        assert_eq!(offset_of!(DecodeCreateInfo, lock), 120);
        assert_eq!(size_of::<DecodeCreateInfo>(), 176);
        assert_eq!(offset_of!(VideoFormat, coded_width), 16);
        assert_eq!(offset_of!(VideoFormat, display), 24);
    }
}
