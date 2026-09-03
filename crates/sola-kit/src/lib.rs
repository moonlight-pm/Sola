//! Iced-based Sola app kit.
//!
//! An app's `main` composes the kit's building blocks (it builds its own
//! `iced::application`/`daemon`). Scaffolding is [`app::startup`] +
//! [`BusSetup`] + an app-owned iced builder — there is no generic
//! `run::<A>()` wrapper. The kit handles:
//!
//! - bus connection + subscription ([`BusSetup`], [`app::bus_subscription`])
//! - call-plane advertise + subscription ([`CallSetup`], [`call::call_subscription`])
//! - call-plane observer ([`install_observer`], [`observe_subscription`])
//! - app-menu publishing (so `Cmd+Q` quits without per-app glue)
//! - system font resolution (no bundled fonts; see `fonts::ensure_system_fonts`)
//! - window settings (no decorations, correct `xdg_toplevel.app_id`)
//! - theme construction from the shared sola palette
//!
//! Active consumers: `sola-monitor`, `sola-settings`, `sola-shell`,
//! `sola-terminal`, agent, browser-core, `sola-paint`, and this crate's
//! storybook.
//!
//! ## Theme protocol
//!
//! The bus theme is process-wide via `Topic::Theme` (`sola_core::theme`).
//! Iced consumers map it with [`theme::theme_from_bus`]. Live bus theme
//! is the steady state: apps store an `iced::Theme`, subscribe via
//! [`app::bus_subscription`], and apply updates with
//! [`apply_theme_update`] (or shell's `on_theme` + [`theme::ShellStyle`]).
//! [`default_theme`] is the pre-replay / offline default. The old
//! WebView host (`apocrypha/sola-app`) is out of kit scope.
//!
//! ## Components
//!
//! See [`components`]. We grow this surface as real apps need shared
//! pieces — no speculative widgets.

pub mod app;
pub mod call;
pub mod clipboard;
pub mod components;
pub mod float;
pub mod fonts;
pub mod menu;
pub mod theme;

pub use app::{BusSetup, QUIT_ACTION_ID, apply_theme_update, is_self_quit};
pub use call::{CallSetup, call_subscription, install_observer, observe_subscription};
pub use float::{
    FloatState, close_app, drag, drag_resize, theme_for, window_ready_task, wrap_if_floating,
};
pub use menu::{
    WINDOW_MENU_LABEL, WindowAction, ensure_window_menu, parse_window_action, window_menu,
};
pub use theme::default_theme;

/// Re-export so consumers don't need a separate `iced` direct dep
/// just to spell out trait bounds or `Element<'_, Self::Message>`.
pub use iced;

/// Re-export so consumers can reference bus types without taking
/// their own dep — convenience only, not load-bearing.
pub use sola_bus;
pub use sola_call;
