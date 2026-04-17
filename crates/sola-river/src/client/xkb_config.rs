//! Driver for `river_xkb_config_v1`.
//!
//! Owns the manager proxy, tracks live `river_xkb_keyboard_v1` instances,
//! and applies named xkb profiles in response to `Topic::XkbProfile` bus
//! events. Profiles are static keymap files embedded at compile time.
//!
//! Used by sola-shell to swap between the default keymap (Sola apps
//! focused) and a Meta→Ctrl remapped keymap (non-Sola apps focused),
//! giving Mac-style Cmd-C/V/T/W behavior inside foreign clients without
//! needing input synthesis.

use std::collections::HashMap;
use std::io::Write;
use std::os::fd::{AsFd, OwnedFd};

use rustix::fs::{MemfdFlags, memfd_create};
use tracing::{info, warn};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, backend::ObjectId, event_created_child,
};

use crate::client::AppData;
use crate::protocol::river_input_management_v1::river_input_device_v1::RiverInputDeviceV1;
use crate::protocol::river_xkb_config_v1::{
    river_xkb_config_v1::{self, RiverXkbConfigV1},
    river_xkb_keyboard_v1::{self, RiverXkbKeyboardV1},
    river_xkb_keymap_v1::{self, RiverXkbKeymapV1},
};

const KEYMAP_DEFAULT: &str = include_str!("../../keymaps/default.xkb");
const KEYMAP_META_AS_CTRL: &str = include_str!("../../keymaps/meta-as-ctrl.xkb");

// Event opcodes from the river-xkb-config-v1 protocol XML.
// `river_xkb_config_v1.xkb_keyboard` is the second event (after `finished`).
const EVT_XKB_KEYBOARD_OPCODE: u16 = 1;
// `river_xkb_keyboard_v1.input_device` is the second event (after `removed`).
const EVT_INPUT_DEVICE_OPCODE: u16 = 1;

#[derive(Default)]
pub struct XkbConfigState {
    pub manager: Option<RiverXkbConfigV1>,
    /// All live keyboards River has told us about. Re-keymapped on every
    /// profile switch.
    pub keyboards: HashMap<ObjectId, RiverXkbKeyboardV1>,
    /// Keymaps we've submitted but haven't yet pushed (waiting on
    /// `success`/`failure`). Value is the profile name for logging.
    pub pending_keymaps: HashMap<ObjectId, &'static str>,
    /// Most recently applied profile, to dedupe redundant switches.
    pub current_profile: Option<&'static str>,
}

/// Apply a named profile. Logs and no-ops on unknown profiles or if the
/// xkb_config global wasn't bound (River wasn't built with input mgmt).
pub fn apply_profile(state: &mut AppData, profile: &str) {
    let Some(qh) = state.qh.clone() else {
        warn!(profile, "xkb apply_profile before queue handle ready");
        return;
    };
    let Some(manager) = state.xkb_config.manager.as_ref() else {
        warn!(
            profile,
            "Topic::XkbProfile received but river_xkb_config_v1 not bound"
        );
        return;
    };

    let (text, name) = match profile {
        "default" => (KEYMAP_DEFAULT, "default"),
        "meta-as-ctrl" => (KEYMAP_META_AS_CTRL, "meta-as-ctrl"),
        other => {
            warn!(profile = other, "unknown xkb profile, ignoring");
            return;
        }
    };

    if state.xkb_config.current_profile == Some(name) {
        // Nothing to do — keymap unchanged.
        return;
    }

    let fd = match make_keymap_fd(text) {
        Ok(fd) => fd,
        Err(e) => {
            warn!(%e, profile = name, "failed to create keymap memfd");
            return;
        }
    };
    let keymap: RiverXkbKeymapV1 = manager.create_keymap(
        fd.as_fd(),
        river_xkb_config_v1::KeymapFormat::TextV1,
        &qh,
        (),
    );
    state.xkb_config.pending_keymaps.insert(keymap.id(), name);
    info!(profile = name, "submitted xkb keymap to river");
}

fn make_keymap_fd(text: &str) -> std::io::Result<OwnedFd> {
    let fd = memfd_create(
        "sola-keymap",
        MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
    )?;
    let mut file = std::fs::File::from(fd);
    file.write_all(text.as_bytes())?;
    // The protocol mandates a NUL-terminated keymap string.
    file.write_all(&[0])?;
    Ok(OwnedFd::from(file))
}

impl Dispatch<RiverXkbConfigV1, ()> for AppData {
    fn event(
        state: &mut Self,
        _: &RiverXkbConfigV1,
        event: river_xkb_config_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            river_xkb_config_v1::Event::XkbKeyboard { id } => {
                info!(id = ?id.id(), "river_xkb_config: new xkb keyboard");
                state.xkb_config.keyboards.insert(id.id(), id);
                // If a profile was already requested before this keyboard
                // appeared, no automatic re-apply — the shell drives all
                // profile switches and will issue another XkbProfile when
                // focus next changes.
            }
            river_xkb_config_v1::Event::Finished => {}
        }
    }

    event_created_child!(AppData, RiverXkbConfigV1, [
        EVT_XKB_KEYBOARD_OPCODE => (RiverXkbKeyboardV1, ()),
    ]);
}

impl Dispatch<RiverXkbKeyboardV1, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverXkbKeyboardV1,
        event: river_xkb_keyboard_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let river_xkb_keyboard_v1::Event::Removed = event {
            state.xkb_config.keyboards.remove(&proxy.id());
        }
        // input_device, layout, capslock, numlock events: not needed here.
    }

    event_created_child!(AppData, RiverXkbKeyboardV1, [
        EVT_INPUT_DEVICE_OPCODE => (RiverInputDeviceV1, ()),
    ]);
}

impl Dispatch<RiverXkbKeymapV1, ()> for AppData {
    fn event(
        state: &mut Self,
        proxy: &RiverXkbKeymapV1,
        event: river_xkb_keymap_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = proxy.id();
        match event {
            river_xkb_keymap_v1::Event::Success => {
                let profile = state
                    .xkb_config
                    .pending_keymaps
                    .remove(&id)
                    .unwrap_or("?");
                let count = state.xkb_config.keyboards.len();
                for kb in state.xkb_config.keyboards.values() {
                    kb.set_keymap(proxy);
                }
                state.xkb_config.current_profile = Some(profile);
                info!(profile, count, "applied xkb profile to all keyboards");
                proxy.destroy();
            }
            river_xkb_keymap_v1::Event::Failure { error_msg } => {
                let profile = state
                    .xkb_config
                    .pending_keymaps
                    .remove(&id)
                    .unwrap_or("?");
                warn!(profile, %error_msg, "river rejected xkb keymap");
                proxy.destroy();
            }
        }
    }
}

// The xkb-config protocol delivers river_input_device_v1 references
// inside river_xkb_keyboard_v1 events; we don't act on them, but the
// dispatcher needs a Dispatch impl for the type.
impl Dispatch<RiverInputDeviceV1, ()> for AppData {
    fn event(
        _: &mut Self,
        _: &RiverInputDeviceV1,
        _: <RiverInputDeviceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
