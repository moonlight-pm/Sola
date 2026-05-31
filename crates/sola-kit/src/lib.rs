//! Iced-based Sola app kit.
//!
//! An app's `main` composes the kit's building blocks (it builds its own
//! `iced::application`/`daemon`). The kit handles:
//!
//! - bus connection + subscription
//! - app-menu publishing (so `Cmd+Q` quits without per-app glue)
//! - font registration from `/opt/sola/share/fonts/`
//! - window settings (no decorations, correct `xdg_toplevel.app_id`)
//! - theme construction from the shared sola palette
//!
//! `sola-monitor` is the canonical first consumer; consult it for
//! a worked example. New apps should follow the same shape.
//!
//! ## Theme protocol
//!
//! The kit ships a default palette via [`theme::default_theme`]. The
//! bus-driven theme protocol (`Topic::Theme` + `sola_core::theme`) is
//! shared with the legacy kit and the WebView apps — both engines
//! eventually resolve the same token vocabulary. Wiring iced apps to
//! the live bus theme is a v0.2 task; today every kit app reads the
//! hardcoded default at startup.
//!
//! ## Components
//!
//! See [`components`]. We grow this surface as real apps need shared
//! pieces — no speculative widgets.

pub mod app;
pub mod components;
pub mod fonts;
pub mod theme;

pub use app::{BusSetup, QUIT_ACTION_ID, apply_theme_update, is_self_quit};
pub use theme::default_theme;

/// Re-export so consumers don't need a separate `iced` direct dep
/// just to spell out trait bounds or `Element<'_, Self::Message>`.
pub use iced;

/// Re-export so consumers can reference bus types without taking
/// their own dep — convenience only, not load-bearing.
pub use sola_bus;
