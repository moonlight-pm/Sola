//! Wayland **content plane** — product paint path (freeze 2026-08-11).
//!
//! Web pixels are presented on a `wl_subsurface` under the iced toplevel.
//! River composites them; iced chrome does not sample dma-bufs.
//!
//! Mode: `SOLA_BROWSER_CONTENT=plane|import|wayland`.
//! See `docs/specs/2026-08-11-sola-browser-content-plane-design.md` and
//! `docs/plans/2026-08-11-browser-present-architecture-deep-dive.md`.

mod plane;

pub use plane::{
    global_sender, parent_ptrs, ContentPlane, ContentPlaneCmd, PlaneError,
};

/// How web content is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentMode {
    /// Legacy: dma-buf → wgpu import → iced sample.
    Import,
    /// Product: attach dma-buf to Wayland content subsurface (custom present).
    Plane,
    /// Stock **WPEDisplayWayland** present (upstream WPEViewWayland). Opens a
    /// real Wayland surface for content — deepest quality path for tile/FrameDone
    /// scheduling. Env: `SOLA_BROWSER_CONTENT=wayland`.
    Wayland,
}

impl ContentMode {
    pub fn from_env() -> Self {
        // Default **plane**. Stock Wayland: SOLA_BROWSER_CONTENT=wayland.
        match std::env::var("SOLA_BROWSER_CONTENT")
            .unwrap_or_else(|_| "plane".into())
            .to_ascii_lowercase()
            .as_str()
        {
            "import" | "iced" | "legacy" | "0" | "false" => Self::Import,
            "wayland" | "wpe-wayland" | "stock" => Self::Wayland,
            _ => Self::Plane,
        }
    }

    pub fn is_plane(self) -> bool {
        matches!(self, Self::Plane)
    }

    pub fn is_wayland(self) -> bool {
        matches!(self, Self::Wayland)
    }

    /// Custom plane or stock Wayland — not iced import sampling.
    pub fn is_native_present(self) -> bool {
        matches!(self, Self::Plane | Self::Wayland)
    }
}

/// Process-wide mode (read once at boot).
pub fn mode() -> ContentMode {
    use std::sync::OnceLock;
    static M: OnceLock<ContentMode> = OnceLock::new();
    *M.get_or_init(ContentMode::from_env)
}
