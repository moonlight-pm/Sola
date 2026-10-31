//! Projects, as the launcher knows it: its app id and call-plane owner.
//! Its Projects row asks Projects for a window to pick a project in
//! ([`crate::app::projects`]); the list of `~/Projects` is Projects' own
//! (its navigator) since 2026-10-31.

/// Projects' app id, and its call-plane owner.
pub const APP_ID: &str = "sola-projects";
pub const CALL_OWNER: &str = "projects";
