//! Shared iced chrome for the Sola browsers, generic over `Engine`.
pub mod app;
pub mod engine;
pub mod input;
pub mod integration;
pub mod run;
pub mod shader;
pub mod util;

pub use engine::{
    ActiveHandle, ClipboardHandle, Cmd, CursorHandle, EditCmd, Engine, FrameReceiver, FrameSlot,
    NavCmd, TabId, TabInfo, TabsHandle, TaggedFrame,
};
pub use app::Msg;
pub use input::CursorKind;
pub use run::run;
pub use shader::{FrameImport, ImportedTexture, SamplePipeline};
