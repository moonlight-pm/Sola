//! Wayland **content plane** — product paint path (freeze 2026-08-11).
//!
//! Web pixels are presented on a `wl_subsurface` under the iced toplevel.
//! River composites them; iced chrome does not sample dma-bufs.
//!
//! Mode: `SOLA_BROWSER_CONTENT=plane|import` (default **`import`** until
//! dogfood flips it). See
//! `docs/specs/2026-08-11-sola-browser-content-plane-design.md`.

mod plane;

pub use plane::{
    global_sender, parent_ptrs, ContentPlane, ContentPlaneCmd, PlaneError,
};

/// How web content is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentMode {
    /// Legacy: dma-buf → wgpu import → iced sample.
    Import,
    /// Product: attach dma-buf to Wayland content subsurface.
    Plane,
}

impl ContentMode {
    pub fn from_env() -> Self {
        match std::env::var("SOLA_BROWSER_CONTENT")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "plane" | "wayland" | "1" | "true" => Self::Plane,
            _ => Self::Import,
        }
    }

    pub fn is_plane(self) -> bool {
        matches!(self, Self::Plane)
    }
}

/// Process-wide mode (read once at boot).
pub fn mode() -> ContentMode {
    use std::sync::OnceLock;
    static M: OnceLock<ContentMode> = OnceLock::new();
    *M.get_or_init(ContentMode::from_env)
}
