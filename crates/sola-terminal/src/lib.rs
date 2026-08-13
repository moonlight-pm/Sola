//! Terminal engine: alacritty grid, PTY + tmux, iced canvas view.
//!
//! The `sola-terminal` binary is the untitled-shell app. Other kit apps
//! (notably `sola-agent-terminal`) reuse this crate as a library. Call
//! [`tmux::configure`] before any PTY if you must not share socket `sola`.

pub mod emulator;
pub mod extkeys;
pub mod input;
pub mod links;
pub mod perf;
pub mod pty;
pub mod state;
pub mod term_view;
pub mod tmux;
