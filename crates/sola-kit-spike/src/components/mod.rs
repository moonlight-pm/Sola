//! Kit components for HTML/CSS apps.
//!
//! Consumer labs (`sola-settings-lab`, `sola-monitor-lab`, storybook)
//! compose these builders. Do not copy `.sidebar` / `.row` / `.etch`
//! markup into an app — define it here once.

pub mod json;
pub mod sidebar;

pub use json::{TokenKind, pretty as json_pretty, preview as json_preview, tokenize};
pub use sidebar::{Sidebar, SidebarItem, sidebar};
