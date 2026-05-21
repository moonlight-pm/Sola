//! sola-browser-wpe library surface.
//!
//! Binary entry points (`src/main.rs`, the phase-0 probes under
//! `src/bin/`) consume these modules. The crate stays binary-first —
//! lib.rs only exists so the modules can be shared without giving
//! each binary its own copy of the FFI bindings.

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]
#![allow(unsafe_op_in_unsafe_fn)]

pub mod wpe_sys;
pub mod wpe;
pub mod wgpu_import;
pub mod shader;
