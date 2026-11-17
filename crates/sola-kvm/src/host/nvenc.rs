//! The parts of NVENC (`libnvidia-encode.so.1`) and CUDA (`libcuda.so.1`)
//! the encoder calls, loaded at run time. Layouts are from NVIDIA's
//! `nvEncodeAPI.h` (FFmpeg's `nv-codec-headers` n13.0.19.0, API 13.0); the
//! asserts at the end pin every size and offset used. Runs of fields the
//! encoder leaves alone are kept as plain arrays of the same size.

use std::ffi::{c_char, c_int, c_uint, c_void};

use libloading::Library;

pub type Status = c_int;
pub type Handle = *mut c_void;

pub const SUCCESS: Status = 0;
const STATUS_NAMES: [&str; 27] = [
    "NV_ENC_SUCCESS",
    "NV_ENC_ERR_NO_ENCODE_DEVICE",
    "NV_ENC_ERR_UNSUPPORTED_DEVICE",
    "NV_ENC_ERR_INVALID_ENCODERDEVICE",
    "NV_ENC_ERR_INVALID_DEVICE",
    "NV_ENC_ERR_DEVICE_NOT_EXIST",
    "NV_ENC_ERR_INVALID_PTR",
    "NV_ENC_ERR_INVALID_EVENT",
    "NV_ENC_ERR_INVALID_PARAM",
    "NV_ENC_ERR_INVALID_CALL",
    "NV_ENC_ERR_OUT_OF_MEMORY",
    "NV_ENC_ERR_ENCODER_NOT_INITIALIZED",
    "NV_ENC_ERR_UNSUPPORTED_PARAM",
    "NV_ENC_ERR_LOCK_BUSY",
    "NV_ENC_ERR_NOT_ENOUGH_BUFFER",
    "NV_ENC_ERR_INVALID_VERSION",
    "NV_ENC_ERR_MAP_FAILED",
    "NV_ENC_ERR_NEED_MORE_INPUT",
    "NV_ENC_ERR_ENCODER_BUSY",
    "NV_ENC_ERR_EVENT_NOT_REGISTERD",
    "NV_ENC_ERR_GENERIC",
    "NV_ENC_ERR_INCOMPATIBLE_CLIENT_KEY",
    "NV_ENC_ERR_UNIMPLEMENTED",
    "NV_ENC_ERR_RESOURCE_REGISTER_FAILED",
    "NV_ENC_ERR_RESOURCE_NOT_REGISTERED",
    "NV_ENC_ERR_RESOURCE_NOT_MAPPED",
    "NV_ENC_ERR_NEED_MORE_OUTPUT",
];

pub const API_MAJOR: u32 = 13;
pub const API_MINOR: u32 = 0;
pub const API_VERSION: u32 = API_MAJOR | (API_MINOR << 24);

const fn struct_version(version: u32) -> u32 {
    API_VERSION | (version << 16) | (0x7 << 28)
}

pub const OPEN_SESSION_VER: u32 = struct_version(1);
pub const CONFIG_VER: u32 = struct_version(9) | (1 << 31);
pub const PRESET_CONFIG_VER: u32 = struct_version(5) | (1 << 31);
pub const INITIALIZE_PARAMS_VER: u32 = struct_version(7) | (1 << 31);
pub const RECONFIGURE_PARAMS_VER: u32 = struct_version(2) | (1 << 31);
pub const CREATE_INPUT_BUFFER_VER: u32 = struct_version(2);
pub const CREATE_BITSTREAM_BUFFER_VER: u32 = struct_version(1);
pub const LOCK_INPUT_BUFFER_VER: u32 = struct_version(1);
pub const PIC_PARAMS_VER: u32 = struct_version(7) | (1 << 31);
pub const LOCK_BITSTREAM_VER: u32 = struct_version(2) | (1 << 31);
pub const FUNCTION_LIST_VER: u32 = struct_version(2);

pub const DEVICE_TYPE_CUDA: u32 = 1;
pub const INFINITE_GOP: u32 = 0xffff_ffff;
pub const RC_CBR: u32 = 2;
pub const MULTI_PASS_QUARTER_RESOLUTION: u32 = 1;
pub const TUNING_ULTRA_LOW_LATENCY: u32 = 3;
pub const BIT_DEPTH_8: u32 = 8;
pub const LEVEL_AUTOSELECT: u32 = 0;

/// Packed 8-bit RGB, B in the lowest byte: B G R A in memory.
pub const BUFFER_FORMAT_ARGB: u32 = 0x0100_0000;
/// Packed 8-bit RGB, R in the lowest byte: R G B A in memory.
pub const BUFFER_FORMAT_ABGR: u32 = 0x1000_0000;

pub const PIC_STRUCT_FRAME: u32 = 1;
pub const PIC_TYPE_IDR: u32 = 3;
pub const PIC_FLAG_FORCE_IDR: u32 = 0x2;
pub const PIC_FLAG_OUTPUT_SPSPPS: u32 = 0x4;

// `RcParams::flags`, the header's bitfields.
pub const RC_ENABLE_LOOKAHEAD: u32 = 1 << 5;
pub const RC_ZERO_REORDER_DELAY: u32 = 1 << 9;

// `HevcConfig::flags`, the header's bitfields.
pub const HEVC_REPEAT_SPSPPS: u32 = 1 << 7;
pub const HEVC_ENABLE_FILLER_DATA: u32 = 1 << 14;
pub const HEVC_CHROMA_FORMAT_SHIFT: u32 = 9;
pub const HEVC_CHROMA_FORMAT_MASK: u32 = 0b11 << HEVC_CHROMA_FORMAT_SHIFT;

// VUI codes (H.265 Annex E).
pub const VUI_VIDEO_FORMAT_UNSPECIFIED: u32 = 5;
pub const VUI_PRIMARIES_BT709: u32 = 1;
pub const VUI_TRANSFER_SRGB: u32 = 13;
pub const VUI_MATRIX_BT709: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

const fn guid(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Guid {
    Guid { data1, data2, data3, data4 }
}

pub const CODEC_HEVC: Guid = guid(0x790cdc88, 0x4522, 0x4d7b, [0x94, 0x25, 0xbd, 0xa9, 0x97, 0x5f, 0x76, 0x03]);
pub const PROFILE_HEVC_MAIN: Guid = guid(0xb514c39a, 0xb55b, 0x40fa, [0x87, 0x8f, 0xf1, 0x25, 0x3b, 0x4d, 0xfd, 0xec]);
pub const PRESET_P1: Guid = guid(0xfc0a8d3e, 0x45f8, 0x4cf8, [0x80, 0xc7, 0x29, 0x88, 0x71, 0x59, 0x0e, 0xbf]);

#[repr(C)]
pub struct OpenSessionParams {
    pub version: u32,
    pub device_type: u32,
    pub device: Handle,
    pub reserved: Handle,
    pub api_version: u32,
    pub reserved1: [u32; 253],
    pub reserved2: [Handle; 64],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RcParams {
    pub version: u32,
    pub rate_control_mode: u32,
    pub const_qp: [u32; 3],
    pub average_bit_rate: u32,
    pub max_bit_rate: u32,
    pub vbv_buffer_size: u32,
    pub vbv_initial_delay: u32,
    /// enableMinQP … enableExtLookahead: the `RC_*` bits.
    pub flags: u32,
    /// minQP, maxQP, initialRCQP, temporallayerIdxMask, temporalLayerQP,
    /// targetQuality, lookaheadDepth, lowDelayKeyFrameScale, the DC offsets
    /// and qpMapMode.
    pub qps_and_quality: [u32; 15],
    pub multi_pass: u32,
    pub rest: [u32; 6],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Vui {
    pub overscan_info_present: u32,
    pub overscan_info: u32,
    pub video_signal_type_present: u32,
    pub video_format: u32,
    pub video_full_range: u32,
    pub colour_description_present: u32,
    pub colour_primaries: u32,
    pub transfer_characteristics: u32,
    pub colour_matrix: u32,
    /// chromaSampleLocationFlag, Top, Bot; bitstreamRestrictionFlag;
    /// timingInfoPresentFlag, numUnitInTicks, timeScale; reserved.
    pub rest: [u32; 19],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct HevcConfig {
    pub level: u32,
    pub tier: u32,
    pub min_cu_size: u32,
    pub max_cu_size: u32,
    /// useConstrainedIntraPred … outputMasteringDisplay: the `HEVC_*` bits.
    pub flags: u32,
    pub idr_period: u32,
    /// intraRefreshPeriod … maxTemporalLayersMinus1.
    pub refresh_refs_slices: [u32; 10],
    pub vui: Vui,
    /// ltrTrustMode … disableDeblockingFilterIDC.
    pub refs_and_filters: [u32; 6],
    pub output_bit_depth: u32,
    pub input_bit_depth: u32,
    /// numTemporalLayers, numViews, reserved1.
    pub reserved1: [u32; 210],
    pub reserved2: [Handle; 64],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Config {
    pub version: u32,
    pub profile: Guid,
    pub gop_length: u32,
    pub frame_interval_p: i32,
    pub mono_chrome_encoding: u32,
    pub frame_field_mode: u32,
    pub mv_precision: u32,
    pub rc: RcParams,
    /// The codec union, NV_ENC_CODEC_CONFIG: HEVC's member, then the rest of
    /// the union, as large as its H.264 member.
    pub hevc: HevcConfig,
    pub codec_rest: [u8; 232],
    pub reserved: [u32; 278],
    pub reserved2: [Handle; 64],
}

#[repr(C)]
pub struct PresetConfig {
    pub version: u32,
    pub reserved: u32,
    pub config: Config,
    pub reserved1: [u32; 256],
    pub reserved2: [Handle; 64],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct InitializeParams {
    pub version: u32,
    pub encode_guid: Guid,
    pub preset_guid: Guid,
    pub encode_width: u32,
    pub encode_height: u32,
    pub dar_width: u32,
    pub dar_height: u32,
    pub frame_rate_num: u32,
    pub frame_rate_den: u32,
    pub enable_encode_async: u32,
    pub enable_ptd: u32,
    /// reportSliceOffsets … enableUniDirectionalB.
    pub flags: u32,
    pub priv_data_size: u32,
    pub reserved: u32,
    pub priv_data: Handle,
    pub encode_config: *mut Config,
    pub max_encode_width: u32,
    pub max_encode_height: u32,
    pub max_me_hint_counts_per_block: [[u32; 4]; 2],
    pub tuning_info: u32,
    /// bufferFormat (D3D12 only), numStateBuffers, outputStatsLevel, reserved1.
    pub reserved1: [u32; 287],
    pub reserved2: [Handle; 64],
}

#[repr(C)]
pub struct ReconfigureParams {
    pub version: u32,
    pub reserved: u32,
    pub params: InitializeParams,
    /// resetEncoder, forceIDR.
    pub flags: u32,
    pub reserved2: u32,
}

#[repr(C)]
pub struct CreateInputBuffer {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub memory_heap: u32,
    pub buffer_format: u32,
    pub reserved: u32,
    pub input_buffer: Handle,
    pub sys_mem_buffer: Handle,
    pub reserved1: [u32; 58],
    pub reserved2: [Handle; 63],
}

#[repr(C)]
pub struct CreateBitstreamBuffer {
    pub version: u32,
    pub size: u32,
    pub memory_heap: u32,
    pub reserved: u32,
    pub bitstream_buffer: Handle,
    pub bitstream_buffer_ptr: Handle,
    pub reserved1: [u32; 58],
    pub reserved2: [Handle; 64],
}

#[repr(C)]
pub struct LockInputBuffer {
    pub version: u32,
    /// doNotWait.
    pub flags: u32,
    pub input_buffer: Handle,
    pub buffer_data: *mut u8,
    pub pitch: u32,
    pub reserved1: [u32; 251],
    pub reserved2: [Handle; 64],
}

#[repr(C)]
pub struct PicParams {
    pub version: u32,
    pub input_width: u32,
    pub input_height: u32,
    pub input_pitch: u32,
    pub encode_pic_flags: u32,
    pub frame_idx: u32,
    pub input_time_stamp: u64,
    pub input_duration: u64,
    pub input_buffer: Handle,
    pub output_bitstream: Handle,
    pub completion_event: Handle,
    pub buffer_format: u32,
    pub picture_struct: u32,
    pub picture_type: u32,
    /// NV_ENC_CODEC_PIC_PARAMS: HEVC's zeroes ask for nothing special.
    pub codec: [u64; 193],
    /// ME hints, the QP map, the alpha buffer, state and recon buffers, reserved.
    pub rest: [u64; 217],
}

#[repr(C)]
pub struct LockBitstream {
    pub version: u32,
    /// doNotWait, ltrFrame, getRCStats.
    pub flags: u32,
    pub output_bitstream: Handle,
    pub slice_offsets: *mut u32,
    pub frame_idx: u32,
    pub hw_encode_status: u32,
    pub num_slices: u32,
    pub bitstream_size: u32,
    pub output_time_stamp: u64,
    pub output_duration: u64,
    pub bitstream: *const u8,
    pub picture_type: u32,
    pub picture_struct: u32,
    pub frame_avg_qp: u32,
    pub frame_satd: u32,
    /// LTR and RC stats, the output stats buffer, reserved.
    pub rest: [u64; 183],
}

/// A function pointer from the list, before its signature is known.
pub type Entry = unsafe extern "C" fn();

/// NV_ENCODE_API_FUNCTION_LIST: 43 entry points in the header's order.
#[repr(C)]
pub struct FunctionList {
    pub version: u32,
    pub reserved: u32,
    pub entries: [Option<Entry>; 43],
    pub reserved2: [Handle; 275],
}

macro_rules! entries {
    ($($field:ident @ $offset:literal : fn($($arg:ty),*) -> $ret:ty;)*) => {
        /// The session calls, typed, from NvEncodeAPICreateInstance's list.
        /// `@` is each one's byte offset in the list, from the header.
        pub struct Session {
            $(pub $field: unsafe extern "C" fn($($arg),*) -> $ret,)*
        }

        impl Session {
            fn from_list(list: &FunctionList) -> Result<Self, String> {
                $(
                    const _: () = assert!($offset >= 8 && ($offset - 8) % 8 == 0 && ($offset - 8) / 8 < 43);
                    let entry = list.entries[($offset - 8) / 8]
                        .ok_or(concat!("NVENC's function list has no ", stringify!($field)))?;
                    // SAFETY: the entry at this offset has this signature in the header.
                    let $field = unsafe {
                        std::mem::transmute::<Entry, unsafe extern "C" fn($($arg),*) -> $ret>(entry)
                    };
                )*
                Ok(Self { $($field,)* })
            }
        }
    };
}

entries! {
    initialize_encoder @ 96: fn(Handle, *mut InitializeParams) -> Status;
    create_input_buffer @ 104: fn(Handle, *mut CreateInputBuffer) -> Status;
    destroy_input_buffer @ 112: fn(Handle, Handle) -> Status;
    create_bitstream_buffer @ 120: fn(Handle, *mut CreateBitstreamBuffer) -> Status;
    destroy_bitstream_buffer @ 128: fn(Handle, Handle) -> Status;
    encode_picture @ 136: fn(Handle, *mut PicParams) -> Status;
    lock_bitstream @ 144: fn(Handle, *mut LockBitstream) -> Status;
    unlock_bitstream @ 152: fn(Handle, Handle) -> Status;
    lock_input_buffer @ 160: fn(Handle, *mut LockInputBuffer) -> Status;
    unlock_input_buffer @ 168: fn(Handle, Handle) -> Status;
    destroy_encoder @ 224: fn(Handle) -> Status;
    open_encode_session_ex @ 240: fn(*mut OpenSessionParams, *mut Handle) -> Status;
    reconfigure_encoder @ 264: fn(Handle, *mut ReconfigureParams) -> Status;
    last_error_string @ 304: fn(Handle) -> *const c_char;
    preset_config_ex @ 320: fn(Handle, Guid, Guid, u32, *mut PresetConfig) -> Status;
}

/// The loaded entry points; the libraries stay open beside them.
pub struct Api {
    pub cu_init: unsafe extern "C" fn(c_uint) -> c_int,
    pub cu_device_get: unsafe extern "C" fn(*mut c_int, c_int) -> c_int,
    pub cu_ctx_create: unsafe extern "C" fn(*mut Handle, c_uint, c_int) -> c_int,
    pub cu_ctx_destroy: unsafe extern "C" fn(Handle) -> c_int,
    pub cu_ctx_push: unsafe extern "C" fn(Handle) -> c_int,
    pub cu_ctx_pop: unsafe extern "C" fn(*mut Handle) -> c_int,
    /// The newest API the driver offers, as `major << 4 | minor`.
    pub max_version: u32,
    pub session: Session,
    _libs: [Library; 2],
}

type CreateInstance = unsafe extern "C" fn(*mut FunctionList) -> Status;
type MaxVersion = unsafe extern "C" fn(*mut u32) -> Status;

impl Api {
    pub fn load() -> Result<Self, String> {
        let open = |name: &str| {
            unsafe { Library::new(name) }.map_err(|e| format!("{name} would not load ({e}); this needs an NVIDIA GPU"))
        };
        let (cuda, nvenc) = (open("libcuda.so.1")?, open("libnvidia-encode.so.1")?);
        fn get<T: Copy>(lib: &Library, symbol: &str) -> Result<T, String> {
            unsafe { lib.get::<T>(symbol.as_bytes()) }.map(|s| *s).map_err(|e| format!("{symbol}: {e}"))
        }
        let max_version: MaxVersion = get(&nvenc, "NvEncodeAPIGetMaxSupportedVersion")?;
        let create_instance: CreateInstance = get(&nvenc, "NvEncodeAPICreateInstance")?;
        let mut version = 0;
        let status = unsafe { max_version(&mut version) };
        if status != SUCCESS {
            return Err(format!("NvEncodeAPIGetMaxSupportedVersion failed ({})", status_name(status)));
        }
        if version < ((API_MAJOR << 4) | API_MINOR) {
            return Err(format!(
                "the NVIDIA driver offers NVENC API {}.{}; this needs {API_MAJOR}.{API_MINOR} (driver 570 or newer)",
                version >> 4,
                version & 0xf
            ));
        }
        // SAFETY: every field is an integer, a pointer or an Option of a function pointer.
        let mut list: Box<FunctionList> = Box::new(unsafe { std::mem::zeroed() });
        list.version = FUNCTION_LIST_VER;
        let status = unsafe { create_instance(&mut *list) };
        if status != SUCCESS {
            return Err(format!("NvEncodeAPICreateInstance failed ({})", status_name(status)));
        }
        Ok(Self {
            cu_init: get(&cuda, "cuInit")?,
            cu_device_get: get(&cuda, "cuDeviceGet")?,
            cu_ctx_create: get(&cuda, "cuCtxCreate_v2")?,
            cu_ctx_destroy: get(&cuda, "cuCtxDestroy_v2")?,
            cu_ctx_push: get(&cuda, "cuCtxPushCurrent_v2")?,
            cu_ctx_pop: get(&cuda, "cuCtxPopCurrent_v2")?,
            max_version: version,
            session: Session::from_list(&list)?,
            _libs: [cuda, nvenc],
        })
    }
}

/// A status as the header names it, with its number.
pub fn status_name(status: Status) -> String {
    match usize::try_from(status).ok().and_then(|i| STATUS_NAMES.get(i)) {
        Some(name) => format!("{name} ({status})"),
        None => format!("status {status}"),
    }
}

// Sizes and offsets from `nvEncodeAPI.h` 13.0, printed by a C program that
// includes it (gcc, x86-64).
const _: () = {
    use std::mem::{offset_of, size_of};
    assert!(OPEN_SESSION_VER == 0x7001_000d);
    assert!(CONFIG_VER == 0xf009_000d);
    assert!(PRESET_CONFIG_VER == 0xf005_000d);
    assert!(INITIALIZE_PARAMS_VER == 0xf007_000d);
    assert!(RECONFIGURE_PARAMS_VER == 0xf002_000d);
    assert!(CREATE_INPUT_BUFFER_VER == 0x7002_000d);
    assert!(CREATE_BITSTREAM_BUFFER_VER == 0x7001_000d);
    assert!(LOCK_INPUT_BUFFER_VER == 0x7001_000d);
    assert!(PIC_PARAMS_VER == 0xf007_000d);
    assert!(LOCK_BITSTREAM_VER == 0xf002_000d);
    assert!(FUNCTION_LIST_VER == 0x7002_000d);

    assert!(size_of::<Guid>() == 16);
    assert!(size_of::<OpenSessionParams>() == 1552);
    assert!(offset_of!(OpenSessionParams, device) == 8);
    assert!(offset_of!(OpenSessionParams, api_version) == 24);
    assert!(offset_of!(OpenSessionParams, reserved2) == 1040);

    assert!(size_of::<RcParams>() == 128);
    assert!(offset_of!(RcParams, average_bit_rate) == 20);
    assert!(offset_of!(RcParams, max_bit_rate) == 24);
    assert!(offset_of!(RcParams, vbv_buffer_size) == 28);
    assert!(offset_of!(RcParams, vbv_initial_delay) == 32);
    assert!(offset_of!(RcParams, flags) == 36);
    assert!(offset_of!(RcParams, qps_and_quality) == 40);
    assert!(offset_of!(RcParams, multi_pass) == 100);

    assert!(size_of::<Vui>() == 112);
    assert!(offset_of!(Vui, video_signal_type_present) == 8);
    assert!(offset_of!(Vui, video_format) == 12);
    assert!(offset_of!(Vui, video_full_range) == 16);
    assert!(offset_of!(Vui, colour_description_present) == 20);
    assert!(offset_of!(Vui, colour_primaries) == 24);
    assert!(offset_of!(Vui, transfer_characteristics) == 28);
    assert!(offset_of!(Vui, colour_matrix) == 32);

    assert!(size_of::<HevcConfig>() == 1560);
    assert!(offset_of!(HevcConfig, flags) == 16);
    assert!(offset_of!(HevcConfig, idr_period) == 20);
    assert!(offset_of!(HevcConfig, vui) == 64);
    assert!(offset_of!(HevcConfig, output_bit_depth) == 200);
    assert!(offset_of!(HevcConfig, input_bit_depth) == 204);
    assert!(offset_of!(HevcConfig, reserved2) == 1048);

    assert!(size_of::<Config>() == 3584);
    assert!(offset_of!(Config, profile) == 4);
    assert!(offset_of!(Config, gop_length) == 20);
    assert!(offset_of!(Config, frame_interval_p) == 24);
    assert!(offset_of!(Config, rc) == 40);
    assert!(offset_of!(Config, hevc) == 168);
    assert!(offset_of!(Config, reserved) == 1960);
    assert!(offset_of!(Config, reserved2) == 3072);

    assert!(size_of::<PresetConfig>() == 5128);
    assert!(offset_of!(PresetConfig, config) == 8);
    assert!(offset_of!(PresetConfig, reserved1) == 3592);

    assert!(size_of::<InitializeParams>() == 1800);
    assert!(offset_of!(InitializeParams, encode_guid) == 4);
    assert!(offset_of!(InitializeParams, preset_guid) == 20);
    assert!(offset_of!(InitializeParams, encode_width) == 36);
    assert!(offset_of!(InitializeParams, frame_rate_num) == 52);
    assert!(offset_of!(InitializeParams, enable_encode_async) == 60);
    assert!(offset_of!(InitializeParams, enable_ptd) == 64);
    assert!(offset_of!(InitializeParams, priv_data) == 80);
    assert!(offset_of!(InitializeParams, encode_config) == 88);
    assert!(offset_of!(InitializeParams, max_encode_width) == 96);
    assert!(offset_of!(InitializeParams, max_me_hint_counts_per_block) == 104);
    assert!(offset_of!(InitializeParams, tuning_info) == 136);
    assert!(offset_of!(InitializeParams, reserved2) == 1288);

    assert!(size_of::<ReconfigureParams>() == 1816);
    assert!(offset_of!(ReconfigureParams, params) == 8);
    assert!(offset_of!(ReconfigureParams, flags) == 1808);

    assert!(size_of::<CreateInputBuffer>() == 776);
    assert!(offset_of!(CreateInputBuffer, buffer_format) == 16);
    assert!(offset_of!(CreateInputBuffer, input_buffer) == 24);
    assert!(offset_of!(CreateInputBuffer, reserved2) == 272);

    assert!(size_of::<CreateBitstreamBuffer>() == 776);
    assert!(offset_of!(CreateBitstreamBuffer, bitstream_buffer) == 16);
    assert!(offset_of!(CreateBitstreamBuffer, reserved2) == 264);

    assert!(size_of::<LockInputBuffer>() == 1544);
    assert!(offset_of!(LockInputBuffer, input_buffer) == 8);
    assert!(offset_of!(LockInputBuffer, buffer_data) == 16);
    assert!(offset_of!(LockInputBuffer, pitch) == 24);
    assert!(offset_of!(LockInputBuffer, reserved2) == 1032);

    assert!(size_of::<PicParams>() == 3360);
    assert!(offset_of!(PicParams, input_time_stamp) == 24);
    assert!(offset_of!(PicParams, input_buffer) == 40);
    assert!(offset_of!(PicParams, output_bitstream) == 48);
    assert!(offset_of!(PicParams, buffer_format) == 64);
    assert!(offset_of!(PicParams, picture_struct) == 68);
    assert!(offset_of!(PicParams, picture_type) == 72);
    assert!(offset_of!(PicParams, codec) == 80);
    assert!(offset_of!(PicParams, rest) == 1624);

    assert!(size_of::<LockBitstream>() == 1544);
    assert!(offset_of!(LockBitstream, output_bitstream) == 8);
    assert!(offset_of!(LockBitstream, bitstream_size) == 36);
    assert!(offset_of!(LockBitstream, output_time_stamp) == 40);
    assert!(offset_of!(LockBitstream, bitstream) == 56);
    assert!(offset_of!(LockBitstream, picture_type) == 64);
    assert!(offset_of!(LockBitstream, frame_avg_qp) == 72);

    assert!(size_of::<FunctionList>() == 2552);
    assert!(offset_of!(FunctionList, entries) == 8);
    assert!(offset_of!(FunctionList, reserved2) == 352);
};
