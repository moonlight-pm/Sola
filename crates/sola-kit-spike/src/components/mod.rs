//! Kit components for HTML/CSS apps.
//!
//! Consumer labs (`sola-settings-lab`, `sola-monitor-lab`, `sola-mail-lab`,
//! storybook) compose these builders. Do not copy kit markup into an app.

pub mod badge;
pub mod button;
pub mod card;
pub mod field;
pub mod icon;
pub mod json;
pub mod list_item;
pub mod pane;
pub mod prose;
pub mod select;
pub mod sidebar;
pub mod split;
pub mod text;
pub mod titlebar;
pub mod toast;
pub mod toolbar;

pub use badge::badge;
pub use button::button;
pub use json::{TokenKind, pretty as json_pretty, preview as json_preview, tokenize};
pub use sidebar::{Sidebar, SidebarItem, sidebar};
pub use titlebar::titlebar;
