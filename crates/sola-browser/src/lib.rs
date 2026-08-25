//! sola-browser — iced chrome + CEF engine.
//!
//! Chrome (tabs, omnibox, session, profiles, vault, bus) lives at the crate
//! root and is generic over [`Engine`]. The CEF backend lives under [`cef`].

pub mod app;
pub mod cef;
pub mod chrome_wake;
pub mod downloads;
pub mod engine;
pub mod groups;
pub mod input;
pub mod instance;
pub mod integration;
pub mod page_menu;
pub mod paste_js;
pub mod profiles;
pub mod run;
pub mod session;
pub mod shader;
pub mod tab_cache;
pub mod util;
#[cfg(feature = "bitwarden")]
pub mod vault;

pub use app::Msg;
pub use cef::CefEngine;
pub use engine::{
    ActiveHandle, ClipboardHandle, Cmd, CursorHandle, DownloadsHandle, EditCmd, Engine,
    FrameReceiver, FrameSlot, HistoryEntry, ImeCaret, ImeHandle, NavCmd, PageContext,
    PageMenusHandle, PasskeysHandle, PendingFrame, TabId, TabInfo, TabsHandle, TaggedFrame,
};
pub use input::CursorKind;
pub use run::run;
pub use shader::{FrameImport, ImportedTexture, SamplePipeline};
