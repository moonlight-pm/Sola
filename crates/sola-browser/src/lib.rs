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
pub mod http_auth;
pub mod input;
pub mod instance;
pub mod integration;
pub mod js_dialog;
pub mod media;
pub mod notify;
pub mod page_menu;
pub mod page_paste;
pub mod paste_js;
pub mod popup;
pub mod profiles;
pub mod run;
pub mod session;
pub mod shader;
pub mod tab_cache;
pub mod util;
#[cfg(feature = "bitwarden")]
pub mod vault;
pub mod visit_history;

pub use app::Msg;
pub use cef::CefEngine;
pub use engine::{
    ActiveHandle, ChromeTabRequest, ClipboardHandle, Cmd, CursorHandle, DevToolsEvent,
    DevToolsHandle, DownloadsHandle, EditCmd, Engine, FindResult, FindResultsHandle, FrameReceiver,
    FrameSlot, HistoryEntry, ImeCaret, ImeHandle, JsDialogsHandle, NavCmd, PageContext,
    PageMenusHandle, PaintSurface, PasskeysHandle, PendingFrame, TabId, TabInfo, TabsHandle,
    TaggedFrame,
};
pub use input::CursorKind;
pub use run::run;
pub use shader::{FrameImport, ImportedTexture, SamplePipeline};
