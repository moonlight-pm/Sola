//! sola-paint — default image viewer/editor for Sola.
//!
//! THESIS: open images land here, not in a throwaway preview. A window per
//! open image (a tab in the shell's bar), graphite well with a checker
//! stage, crop/rotate/flip on the picture itself.
//! OWN-WORLD: sola-kit graphite chrome (titlebar + tool strip), checker
//! well as the only subject texture.
//! STORY: drop a path or open a file; crop on the canvas; save.
//! FIRST VIEWPORT: top tool strip, image on a dark well.
//! FORM: kit Operate surface inside the established Sola world.

mod app;
mod doc;
mod geom;
mod instance;
mod stage;
mod windows;

use iced::keyboard;
use iced::{Point, Size};
use std::sync::Arc;

use sola_bus::Message;

#[derive(Debug, Clone)]
enum Msg {
    Bus(Arc<Message>),
    /// A message from a window: that window is the one acted on.
    At(iced::window::Id, Box<Msg>),
    /// A window's focus and close events.
    Window(iced::window::Id, iced::window::Event),
    /// Boot has waited for the saved session; open the empty window if
    /// nothing opened.
    BootSettled,
    OpenDialog,
    SaveAsDialog,
    Picker(sola_kit::components::file_picker::Message),
    Save,
    ToggleCrop,
    StagePress(Point, Size),
    StageMove(Point, Size),
    StageRelease,
    ZoomAt {
        cursor: Point,
        size: Size,
        factor: f32,
    },
    ZoomFit,
    ZoomIn,
    ZoomOut,
    ApplyCrop,
    CancelCrop,
    RotateCw,
    RotateCcw,
    FlipH,
    FlipV,
    Undo,
    DocLoaded(Result<crate::doc::Loaded, String>),
    SessionLoaded {
        loaded: Vec<crate::doc::Loaded>,
        select: Option<std::path::PathBuf>,
    },
    KeyPressed(keyboard::Key, keyboard::Modifiers),
    TitleDrag,
    TitleResize(iced::window::Direction),
    TitleClose,
}

fn main() -> iced::Result {
    crate::app::run()
}
