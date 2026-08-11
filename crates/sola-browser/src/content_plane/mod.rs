//! Browser content present modes.
//!
//! **Default (product):** stock **WPEDisplayWayland** + river lockstep
//! (`ContentMode::Wayland`). Content plane / iced import remain as
//! emergency overrides via `SOLA_BROWSER_CONTENT=plane|import`.
//!
//! See Option A freeze + lockstep plan, and the interim content-plane
//! freeze for the demoted hybrid path.

mod plane;

pub use plane::{
    global_sender, parent_ptrs, ContentPlane, ContentPlaneCmd, PlaneError,
};

/// How web content is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentMode {
    /// Legacy: dma-buf → wgpu import → iced sample.
    Import,
    /// Debug/emergency: headless WPE → custom content subsurface (demoted).
    /// Env: `SOLA_BROWSER_CONTENT=plane`.
    Plane,
    /// **Product default:** stock **WPEDisplayWayland** present + river
    /// lockstep under iced chrome hole (`Topic::BrowserContentScissor`).
    ///
    /// **Input:** content surface uses the stock WPE seat when the pointer is
    /// over the companion; chrome continues iced hit-test → WPE inject only
    /// for chrome chrome (omnibox/tabs). Do not double-route both into the
    /// same view.
    Wayland,
}

impl ContentMode {
    pub fn from_env() -> Self {
        // Default **wayland** (Option A product path). Overrides:
        // plane | import for demoted hybrids.
        match std::env::var("SOLA_BROWSER_CONTENT")
            .unwrap_or_else(|_| "wayland".into())
            .to_ascii_lowercase()
            .as_str()
        {
            "import" | "iced" | "legacy" => Self::Import,
            "plane" | "subsurface" | "hybrid" => Self::Plane,
            "wayland" | "wpe-wayland" | "stock" | "1" | "true" | "" => Self::Wayland,
            other => {
                tracing::warn!(
                    %other,
                    "unknown SOLA_BROWSER_CONTENT; using wayland default"
                );
                Self::Wayland
            }
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
