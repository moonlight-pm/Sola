//! sola-browser-cef library surface.
//!
//! Sibling of `sola-browser-wpe`. Identical iced + wgpu + shader
//! pipeline; the only difference is the frame producer (CEF here,
//! WPE there). See `docs/specs/2026-05-21-sola-browser-cef-port-and-benchmark.md`.

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]
#![allow(unsafe_op_in_unsafe_fn)]

pub mod cef;
pub mod wgpu_import;
pub mod shader;
