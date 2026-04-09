/// Input handling — keybindings and related logic.
///
/// Hardware input plumbing (libinput setup) lives in `backend/input.rs`.
/// This module contains the compositor-level logic that decides what
/// key events mean.
pub mod binding;
