//! What only the shell reads back, in its own files rather than on the bus
//! (`docs/specs/2026-11-10-bus-state-design.md`): the tab bar's places,
//! each floating app's rectangle, and the apps hidden with Super+H.
//!
//! The first two are kept across reboots; hidden apps only while logged in
//! (they came back after a desk restart when the bus held them). The bus's
//! old copies are read until the shell has saved its own.

use std::collections::HashMap;

use sola_bus::topics::{FloatGeometry, TabLayout};
use sola_core::state_file::StateFile;

pub struct ShellFiles {
    tabs: StateFile,
    floats: StateFile,
    hidden: StateFile,
}

impl ShellFiles {
    pub fn new() -> Self {
        Self {
            tabs: StateFile::new("shell", "tabs.json"),
            floats: StateFile::new("shell", "floats.json"),
            hidden: StateFile::runtime("shell", "hidden.json"),
        }
    }

    /// The saved tab layout, if there is one.
    pub fn tabs(&self) -> Option<TabLayout> {
        self.tabs
            .load()
            .or_else(|| sola_bus::state::legacy_file("shell/tabs.yaml"))
    }

    pub fn save_tabs(&self, layout: &TabLayout) {
        self.tabs.save(layout);
    }

    /// Each floating app's remembered rectangle, by app id.
    pub fn floats(&self) -> HashMap<String, FloatGeometry> {
        let list: Vec<FloatGeometry> = self
            .floats
            .load()
            .or_else(|| sola_bus::state::legacy_section("FloatGeometry"))
            .unwrap_or_default();
        list.into_iter().map(|f| (f.app_id.clone(), f)).collect()
    }

    pub fn save_floats(&self, floats: &HashMap<String, FloatGeometry>) {
        let mut list: Vec<&FloatGeometry> = floats.values().collect();
        list.sort_by(|a, b| a.app_id.cmp(&b.app_id));
        self.floats.save(&list);
    }

    /// Hidden apps: lowercased app id → the app id as given.
    pub fn hidden(&self) -> HashMap<String, String> {
        self.hidden.load().unwrap_or_default()
    }

    pub fn save_hidden(&self, hidden: &HashMap<String, String>) {
        self.hidden.save(hidden);
    }
}
