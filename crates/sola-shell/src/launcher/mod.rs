//! Launcher subsystem.
//!
//! `state` holds `LauncherState` and the filter logic — pure data.
//! Window management (iced surface, input handling) lands in Task 5.
pub mod state;
