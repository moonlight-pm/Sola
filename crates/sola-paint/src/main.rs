//! sola-paint — default image viewer/editor for Sola.
//!
//! THESIS: open images land here, not in a throwaway preview. Left tab
//! strip of open files, graphite well with a checker stage, crop/rotate/flip
//! on the picture itself.
//! OWN-WORLD: sola-kit graphite chrome (`SidebarPanel` Large + chrome strip),
//! checker well as the only subject texture.
//! STORY: drop a path or open a file; crop on the canvas; save.
//! FIRST VIEWPORT: left tabs, top tool strip, image on a dark well.
//! FORM: kit Operate surface inside the established Sola world.

mod app;
mod doc;
mod geom;
mod stage;

use iced::keyboard;
use iced::{Point, Size};
use std::sync::Arc;

use sola_bus::Message;

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    Select(u64),
    Close(u64),
    HoverTab(Option<String>),
    OpenDialog,
    SaveAsDialog,
    Picker(sola_kit::components::file_picker::Message),
    Save,
    ToggleCrop,
    CropPress(Point, Size),
    StageMove(Point, Size),
    CropRelease,
    ApplyCrop,
    CancelCrop,
    RotateCw,
    RotateCcw,
    FlipH,
    FlipV,
    Undo,
    KeyPressed(keyboard::Key, keyboard::Modifiers),
    WindowReady(Option<iced::window::Id>),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
}

fn main() -> iced::Result {
    crate::app::run()
}
