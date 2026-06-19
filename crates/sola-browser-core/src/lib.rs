//! Shared iced chrome for the Sola browsers, generic over `Engine`.
pub mod app;
pub mod engine;
pub mod util;
// Added in Task 2:
// pub mod input;
// pub mod integration;
// pub mod run;

pub use engine::{
    ActiveHandle, Cmd, CursorHandle, Engine, FrameReceiver, FrameSlot, InputEvent, NavCmd, TabId,
    TabInfo, TabsHandle, TaggedFrame,
};
pub use app::Msg;
// pub use run::run;   // re-enable in Task 2
