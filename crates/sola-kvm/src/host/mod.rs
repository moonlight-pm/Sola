//! Remote view, the host's half on Linux: this machine's screen to a paired
//! viewer (`sola-kvm view`), and the viewer's input back. A Mac's half is
//! its agent's (`apps/sola-kvm-mac/src/view/`); the wire is
//! [`view_wire`](crate::view_wire).
//!
//! The output through wlr-screencopy into shared memory, HEVC from NVENC,
//! input through the KVM's own Wayland injector, the clipboard through its
//! own. The picture is always this machine's own screen: a Linux host makes
//! no screen of the window's size, and says so (`Down::CanExtend`).

mod capture;
pub mod encode;
pub mod flow;
mod nvenc;
pub mod rate;
mod server;

pub use server::spawn;
