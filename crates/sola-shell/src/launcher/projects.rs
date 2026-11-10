//! Projects, as the launcher knows it: its app id and call-plane owner.
//! Its Projects row asks Projects for a window to pick a project in
//! ([`crate::app::projects`]); the list of `~/Projects` is Projects' own
//! (its navigator) since 2026-10-31. Design's row asks Design the same way.

/// Projects' app id, and its call-plane owner.
pub const APP_ID: &str = "sola-projects";
pub const CALL_OWNER: &str = "projects";

/// The apps whose launcher row asks them for a window to pick in
/// (`window.navigate`, a navigator), by app id with their call owners:
/// Projects, and Design since 2026-11-10 (Joshua: "When i open a new design
/// window, i'm then presented (like projects) the list of project/design i
/// can open to view").
const NAVIGATING: &[(&str, &str)] = &[(APP_ID, CALL_OWNER), ("sola-design", "design")];

/// `app_id`'s call owner, when its launcher row asks for a navigator.
pub fn navigating(app_id: &str) -> Option<&'static str> {
    NAVIGATING
        .iter()
        .find(|(id, _)| *id == app_id)
        .map(|(_, owner)| *owner)
}
