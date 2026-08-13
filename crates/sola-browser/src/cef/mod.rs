//! CEF engine backend (CPU OSR). Product path for sola-browser.
//!
//! Engine code lives here as-is from the former `sola-browser-cef` crate.
//! Chrome (tabs, omnibox, session, vault) is engine-agnostic at the crate root.

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]
#![allow(unsafe_op_in_unsafe_fn)]

pub mod cpu_import;
pub mod engine;
pub mod frame;
pub mod host;
pub mod input;
pub mod ipc;
pub mod page_ime;
pub mod paint;
pub mod router;

pub use engine::CefEngine;
