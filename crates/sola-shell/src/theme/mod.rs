//! Sola-shell's per-component theme bindings, layered on top of the
//! kit's default theme. Each surface (menubar/launcher/menu/switcher)
//! contributes its slots → palette mappings via this module.

use std::collections::BTreeMap;

use sola_core::theme::{Binding, ComponentBindings};

/// All shell-component bindings — merged into `kit_default_theme()`'s
/// `components` map at startup.
pub fn shell_default_bindings() -> BTreeMap<String, ComponentBindings> {
    let mut map = BTreeMap::new();
    map.insert("menubar".into(), menubar());
    // T7-T9 will populate these as launcher/menu/switcher components land.
    map
}

/// Theme bindings for the `<Menubar>` component.
///
/// Every `--sola-menubar-<slot>` var referenced in menubar.css must have
/// a corresponding entry here. Slot names must be kept in sync with the CSS.
pub fn menubar() -> ComponentBindings {
    let mut comp = ComponentBindings::default();

    // Surface / background
    comp.slots.insert("bg".into(), Binding::new("surface", "bg-primary"));

    // Foreground — default text color on the bar
    comp.slots.insert("fg".into(), Binding::new("text", "text-secondary"));

    // App-name — bold, bright white
    comp.slots.insert("app-name-fg".into(), Binding::new("text", "text-primary"));

    // System-menu (logo) icon color
    comp.slots.insert("system-menu-fg".into(), Binding::new("text", "text-secondary"));

    // Active (open) state highlight background for any menu item
    comp.slots.insert("label-active-bg".into(), Binding::new("surface", "bg-tertiary"));

    // Horizontal padding inside each label / system-menu button (12px in legacy)
    comp.slots.insert("label-padding-inline".into(), Binding::new("space", "space-md"));

    // Right-side tray padding (16px in legacy)
    comp.slots.insert("tray-padding-right".into(), Binding::new("space", "space-lg"));

    // Clock color — muted gray
    comp.slots.insert("clock-fg".into(), Binding::new("text", "text-tertiary"));

    // Clock font size (12px in legacy → text-caption = 11px, closest available)
    comp.slots.insert("clock-size".into(), Binding::new("text-size", "text-caption"));

    // Toast background — dark reddish (danger-adjacent)
    comp.slots.insert("toast-bg".into(), Binding::new("status", "danger"));

    // Toast foreground — primary white
    comp.slots.insert("toast-fg".into(), Binding::new("text", "text-primary"));

    // Toast font size
    comp.slots.insert("toast-size".into(), Binding::new("text-size", "text-caption"));

    // Toast border-radius (bottom corners only, 6px in legacy → radius-lg)
    comp.slots.insert("toast-radius".into(), Binding::new("radius", "radius-lg"));

    // Font family — DejaVu Sans (system default)
    comp.slots.insert("font-family".into(), Binding::new("font-family", "font-sans"));

    // Body font size (15px in legacy → text-body-lg = 13px, closest available)
    comp.slots.insert("text-size".into(), Binding::new("text-size", "text-body-lg"));

    comp
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_kit::theme::kit_default_theme;

    #[test]
    fn shell_bindings_merge_and_validate() {
        let mut theme = kit_default_theme();
        theme.components.extend(shell_default_bindings());
        theme.validate().expect("merged theme must validate");
    }
}
