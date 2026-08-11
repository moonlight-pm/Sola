//! sola-browser — WPE WebKit engine + iced chrome.
//!
//! Chrome (tabs, omnibox, bus, shader scaffolding) lives at the crate root.
//! WPE FFI / worker / dma-buf import lives under [`wpe`].

pub mod app;
pub mod content_plane;
pub mod engine;
pub mod input;
pub mod integration;
pub mod profiles;
pub mod run;
pub mod session;
pub mod shader;
pub mod util;
#[cfg(feature = "bitwarden")]
pub mod vault;
pub mod wpe;

pub use engine::{
    ActiveHandle, ClipboardHandle, Cmd, CursorHandle, EditCmd, Engine, FrameReceiver, FrameSlot,
    NavCmd, PendingFrame, TabId, TabInfo, TabsHandle, TaggedFrame,
};
pub use app::Msg;
pub use input::CursorKind;
pub use run::run;
pub use shader::{FrameImport, ImportedTexture, SamplePipeline};
pub use wpe::engine::WpeEngine;
