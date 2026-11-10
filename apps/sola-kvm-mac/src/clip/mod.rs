//! Clipboard TCP side channel (CLIP1).

mod platform;
mod proto;
mod worker;

pub use platform::{read_text, write_text};
pub use worker::{spawn, ClipHandle};
