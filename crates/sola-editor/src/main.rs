//! sola-editor — Sola's text editor. Emacs keys (`keymap`), a window per
//! file, and `edit <file>…` to open one from anywhere.
//!
//! The editing is plain data (`buffer`, `editor`, `search`, `grid`), with no
//! window in it. `view` draws it on an iced canvas; `app` is the rest: windows,
//! menus, the bus, files.

mod app;
mod buffer;
mod command;
mod editor;
mod files;
mod grid;
mod instance;
mod keymap;
mod lang;
mod overlay;
mod search;
mod session;
mod syntax;
mod view;
mod watch;
mod win;

use std::path::PathBuf;
use std::sync::Arc;

use iced::{event, keyboard, window};
use sola_bus::Message;

use crate::buffer::Pos;
use crate::command::Command;

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    /// A call: `open` from Sola's opener and `edit`.
    Call(sola_kit::sola_call::Incoming),
    /// A message from a window's view: that window is the one acted on.
    At(window::Id, Box<Msg>),
    Window(window::Id, window::Event),
    Key {
        window: window::Id,
        key: keyboard::Key,
        modifiers: keyboard::Modifiers,
        text: Option<String>,
        status: event::Status,
    },
    Modifiers(keyboard::Modifiers),
    Loaded(Result<files::Loaded, String>),
    Picker(sola_kit::components::file_picker::Message),
    Pasted(window::Id, Option<String>),
    DiskChanged(PathBuf),
    /// Unsaved text goes to the recovery files.
    RecoverTick,
    /// The highlighter has more to do than one frame's worth.
    HighlightMore,

    Press {
        at: Pos,
        clicks: view::Clicks,
        shift: bool,
    },
    DragTo(Pos),
    Wheel {
        lines: f32,
        columns: f32,
    },
    ScrollTo(f32),

    MiniInput(String),
    MiniSubmit,
    PaletteInput(String),
    PaletteSubmit,
    PaletteRun(Command),
    PaletteClose,
    Answer(overlay::Answer),

    TitleDrag,
    TitleResize(window::Direction),
    TitleClose,
}

fn main() -> iced::Result {
    crate::app::run()
}
