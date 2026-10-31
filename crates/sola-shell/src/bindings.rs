//! The shell's actions and their keys now
//! (`docs/specs/2026-10-30-keybindings-design.md`).
//!
//! One table: what River grabs (`Shell::shell_key_chords`), what a chord
//! does (`on_chord`), what Super+K, the flower and the Workspaces menu
//! show, and what Settings → Keybindings lists (`Topic::KeyActions`,
//! owner `system`). Your changes come in as `Topic::Keybindings`; other
//! owners' declarations as `Topic::KeyActions`, so a key you gave an app
//! can take one from the system.

use std::collections::HashMap;

use sola_bus::topics::{KeyAction, KeyActions, Keybindings, Zone};
use sola_core::{KeyChord, KeyCode};
use sola_kit::keys::{self, Binding, SYSTEM};

/// What a system key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Launcher,
    Shortcuts,
    Lock,
    CloseApp,
    Hide,
    NextTab,
    PrevTab,
    NextWorkspace,
    PrevWorkspace,
    /// Show workspace N (0-based).
    ShowWorkspace(usize),
    /// Send the focused window to workspace N (0-based).
    SendToWorkspace(usize),
    ScreenshotFull,
    ScreenshotRegion,
    ScreenshotWindow,
    /// Move the focused tab to a zone.
    Zone(Zone),
}

impl Act {
    pub fn is_screenshot(self) -> bool {
        matches!(
            self,
            Act::ScreenshotFull | Act::ScreenshotRegion | Act::ScreenshotWindow
        )
    }
}

/// A system action: its id (what an override names), heading, label and
/// default key.
#[derive(Clone, Debug)]
pub struct SysAction {
    pub act: Act,
    pub id: String,
    pub group: &'static str,
    pub label: String,
    pub default: Option<KeyChord>,
}

fn sys(act: Act, id: &str, group: &'static str, label: &str, default: KeyChord) -> SysAction {
    SysAction {
        act,
        id: id.to_string(),
        group,
        label: label.to_string(),
        default: Some(default),
    }
}

/// Workspaces 1…9 sit on the digit row.
pub const WORKSPACE_KEYS: [KeyCode; 9] = [
    KeyCode::KEY_1,
    KeyCode::KEY_2,
    KeyCode::KEY_3,
    KeyCode::KEY_4,
    KeyCode::KEY_5,
    KeyCode::KEY_6,
    KeyCode::KEY_7,
    KeyCode::KEY_8,
    KeyCode::KEY_9,
];

/// The zone actions: Move to each zone, with its default key.
pub fn zone_actions() -> Vec<SysAction> {
    let kp = |k: KeyCode| k.meta();
    vec![
        sys(Act::Zone(Zone::Fullscreen), "zone.full", "Zones", "Move to Full", kp(KeyCode::KP_0)),
        sys(Act::Zone(Zone::Left), "zone.left", "Zones", "Move to Left", kp(KeyCode::KP_4)),
        sys(Act::Zone(Zone::Right), "zone.right", "Zones", "Move to Right", kp(KeyCode::KP_6)),
        sys(Act::Zone(Zone::Cinema), "zone.cinema", "Zones", "Move to Cinema", kp(KeyCode::KP_ENTER)),
        sys(Act::Zone(Zone::Float), "zone.float", "Zones", "Float", kp(KeyCode::KP_MULTIPLY)),
    ]
}

/// Every system action, in the order Settings and Super+K list them.
pub fn system_actions(zones: Vec<SysAction>) -> Vec<SysAction> {
    let mut all = vec![
        sys(Act::Launcher, "launcher", "Shell", "Launch Application", KeyCode::SPACE.meta()),
        sys(Act::Shortcuts, "shortcuts", "Shell", "Keyboard Shortcuts", KeyCode::K.meta()),
        sys(Act::Lock, crate::lock::ACTION_LOCK, "Shell", "Lock Screen", KeyCode::L.meta()),
        sys(Act::CloseApp, "close-app", "Shell", "Close App", KeyCode::Q.meta()),
        sys(Act::Hide, "hide", "Shell", "Hide App", KeyCode::H.meta()),
        sys(Act::NextTab, "tab.next", "Shell", "Next Tab", KeyCode::GRAVE.meta()),
        sys(Act::PrevTab, "tab.prev", "Shell", "Previous Tab", KeyCode::GRAVE.meta_shift()),
        sys(Act::NextWorkspace, "workspace.next", "Workspaces", "Next Workspace", KeyCode::TAB.meta()),
        sys(
            Act::PrevWorkspace,
            "workspace.prev",
            "Workspaces",
            "Previous Workspace",
            KeyCode::TAB.meta().shift(),
        ),
    ];
    for (i, k) in WORKSPACE_KEYS.iter().enumerate() {
        all.push(sys(
            Act::ShowWorkspace(i),
            &format!("workspace.show.{}", i + 1),
            "Workspaces",
            &format!("Show Workspace {}", i + 1),
            k.meta(),
        ));
    }
    for (i, k) in WORKSPACE_KEYS.iter().enumerate() {
        all.push(sys(
            Act::SendToWorkspace(i),
            &format!("workspace.send.{}", i + 1),
            "Workspaces",
            &format!("Send Window to Workspace {}", i + 1),
            k.meta().ctrl(),
        ));
    }
    all.extend([
        sys(Act::ScreenshotFull, "screenshot.screen", "Capture", "Screenshot Screen", KeyCode::KEY_3.meta_shift()),
        sys(
            Act::ScreenshotRegion,
            "screenshot.selection",
            "Capture",
            "Screenshot Selection",
            KeyCode::KEY_4.meta_shift(),
        ),
        sys(Act::ScreenshotWindow, "screenshot.window", "Capture", "Screenshot Window", KeyCode::KEY_5.meta_shift()),
    ]);
    all.extend(zones);
    all
}

/// The system's actions, everyone else's declarations, your overrides,
/// and what that makes each system key now.
pub struct Bindings {
    actions: Vec<SysAction>,
    others: HashMap<String, KeyActions>,
    overrides: Keybindings,
    resolved: Vec<Binding>,
    /// Settings is waiting for a key: grab nothing.
    pub capturing: bool,
}

impl Default for Bindings {
    fn default() -> Self {
        Self::new(system_actions(zone_actions()))
    }
}

impl Bindings {
    pub fn new(actions: Vec<SysAction>) -> Self {
        let mut b = Self {
            actions,
            others: HashMap::new(),
            overrides: Keybindings::default(),
            resolved: Vec::new(),
            capturing: false,
        };
        b.resolve();
        b
    }

    /// The system's declaration, for `Topic::KeyActions`.
    pub fn declaration(&self) -> KeyActions {
        KeyActions {
            owner: SYSTEM.to_string(),
            label: "System".to_string(),
            actions: self
                .actions
                .iter()
                .map(|a| KeyAction {
                    id: a.id.clone(),
                    label: a.label.clone(),
                    group: a.group.to_string(),
                    default: a.default,
                })
                .collect(),
        }
    }

    /// Replace the system's actions (the zones changed).
    pub fn set_actions(&mut self, actions: Vec<SysAction>) {
        self.actions = actions;
        self.resolve();
    }

    /// Another owner's declaration arrived (`None`: retracted). Returns
    /// whether a system key changed.
    pub fn set_declared(&mut self, owner: &str, decl: Option<KeyActions>) -> bool {
        if owner == SYSTEM {
            return false;
        }
        match decl {
            Some(d) => self.others.insert(owner.to_string(), d),
            None => self.others.remove(owner),
        };
        self.resolve()
    }

    /// Your overrides arrived. Returns whether a system key changed.
    pub fn set_overrides(&mut self, keys: Keybindings) -> bool {
        self.overrides = keys;
        self.resolve()
    }

    fn resolve(&mut self) -> bool {
        let mut decls = vec![self.declaration()];
        decls.extend(self.others.values().cloned());
        let before: Vec<Option<KeyChord>> = self.system_keys().collect();
        self.resolved = keys::resolve(&decls, &self.overrides);
        let after: Vec<Option<KeyChord>> = self.system_keys().collect();
        before != after
    }

    fn system_keys(&self) -> impl Iterator<Item = Option<KeyChord>> + '_ {
        self.resolved
            .iter()
            .filter(|b| b.owner == SYSTEM)
            .map(|b| b.key)
    }

    /// `act`'s key now.
    pub fn key(&self, act: Act) -> Option<KeyChord> {
        let id = &self.actions.iter().find(|a| a.act == act)?.id;
        keys::key_of(&self.resolved, SYSTEM, id)
    }

    /// The system action a chord runs.
    pub fn act_for(&self, chord: &KeyChord) -> Option<Act> {
        self.actions
            .iter()
            .find(|a| keys::key_of(&self.resolved, SYSTEM, &a.id) == Some(*chord))
            .map(|a| a.act)
    }

    /// Every system key now.
    pub fn chords(&self) -> Vec<KeyChord> {
        self.system_keys().flatten().collect()
    }

    /// Every system action and its key now, in order: (group, label, key, act).
    pub fn rows(&self) -> Vec<(&'static str, String, Option<KeyChord>, Act)> {
        self.actions
            .iter()
            .map(|a| (a.group, a.label.clone(), keys::key_of(&self.resolved, SYSTEM, &a.id), a.act))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sola_bus::topics::KeyOverride;

    #[test]
    fn defaults_are_today_s_keys() {
        let b = Bindings::default();
        assert_eq!(b.act_for(&KeyCode::SPACE.meta()), Some(Act::Launcher));
        assert_eq!(b.act_for(&KeyCode::KEY_3.meta().ctrl()), Some(Act::SendToWorkspace(2)));
        assert_eq!(b.act_for(&KeyCode::KP_4.meta()), Some(Act::Zone(Zone::Left)));
        assert_eq!(b.key(Act::Lock), Some(KeyCode::L.meta()));
    }

    #[test]
    fn an_override_moves_a_key() {
        let mut b = Bindings::default();
        assert!(b.set_overrides(Keybindings {
            overrides: vec![KeyOverride {
                owner: SYSTEM.into(),
                action: "hide".into(),
                key: Some(KeyCode::J.meta()),
            }],
        }));
        assert_eq!(b.act_for(&KeyCode::J.meta()), Some(Act::Hide));
        assert_eq!(b.act_for(&KeyCode::H.meta()), None);
    }

    #[test]
    fn ids_are_unique() {
        let all = system_actions(zone_actions());
        for (i, a) in all.iter().enumerate() {
            assert!(all[i + 1..].iter().all(|o| o.id != a.id), "{} twice", a.id);
        }
    }
}
