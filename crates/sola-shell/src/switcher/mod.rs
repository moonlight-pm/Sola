//! Switcher subsystem.
//!
//! `state` holds `SwitcherState` and `SwitcherApp` — pure data.
//! Window management (iced surface, key handling, focus commit) lands in Task 6.
pub mod state;
