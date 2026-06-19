//! Browser chrome message type + layout constants. `App<E>` and its
//! update/view methods are added in Task 2.
use std::sync::Arc;

use crate::engine::TabId;

pub const DEFAULT_URL: &str = "https://slate.auto";
pub const VIEW_W: u32 = 1280;
pub const VIEW_H: u32 = 800;
#[allow(dead_code)]
pub const CHROME_HEIGHT: f32 = 38.0;
#[allow(dead_code)]
pub const SIDEBAR_W_DEFAULT: f32 = 200.0;
#[allow(dead_code)]
pub const SIDEBAR_W_MIN: f32 = 120.0;
#[allow(dead_code)]
pub const SIDEBAR_W_MAX: f32 = 420.0;

#[derive(Debug, Clone)]
pub enum Msg {
    NewFrame,
    NavBack,
    NavForward,
    NavReload,
    UrlInput(String),
    UrlSubmit,
    CloseTab(TabId),
    ActivateTab(TabId),
    Tick,
    Bus(Arc<sola_bus::Message>),
    DividerPress,
    CursorMoved(f32),
    CursorReleased,
    TabHover(Option<usize>),
}
