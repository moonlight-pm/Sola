//! Wayland client side. Owns surface lifecycle and translates
//! wl_seat input events into CEF input events.

pub mod client;
pub mod cursor;
pub mod input;
pub mod surface;

pub use client::WaylandClient;
pub use surface::Surface;
