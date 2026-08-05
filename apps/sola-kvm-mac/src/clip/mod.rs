//! Clipboard TCP side channel (CLIP1).

mod platform;
mod proto;
mod worker;

pub use worker::{spawn, ClipHandle};
