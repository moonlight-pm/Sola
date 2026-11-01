//! Caps Lock does nothing on the desk (Joshua, 2026-11-01: "Caps lock as a
//! function should be completely disabled. I don't want to see the caps
//! lock light come on .. caps lock by itself should be a no-op for now");
//! Super+CapsLock still shows the next workspace. Every keyboard gets the
//! keymap River runs by default (US: the desk sets no XKB options) with the
//! Caps key's lock action taken off (`keymaps/desk.xkb`): the key still
//! sends `Caps_Lock`, so the binding matches, but it never locks and its
//! light never comes on. Setting a keymap resets the keyboard's state, so
//! NumLock goes back on after it. Should Caps Lock come on anyway (a
//! keyboard River could not give the keymap), it is turned off at once.

use std::os::fd::AsFd;

use tracing::{error, info};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use crate::client::AppData;
use crate::protocol::river_xkb_config_v1::{
    river_xkb_config_v1::{KeymapFormat, RiverXkbConfigV1},
    river_xkb_keyboard_v1::RiverXkbKeyboardV1,
    river_xkb_keymap_v1::{self, RiverXkbKeymapV1},
};

const KEYMAP: &str = include_str!("../../keymaps/desk.xkb");

#[derive(Default)]
pub struct DeskKeymap {
    keymap: Option<RiverXkbKeymapV1>,
    ready: bool,
    failed: bool,
    /// Keyboards that appeared before River built the keymap.
    waiting: Vec<RiverXkbKeyboardV1>,
}

impl DeskKeymap {
    /// A keyboard appeared: give it the desk's keymap, asking River to
    /// build it the first time.
    pub fn keyboard(
        &mut self,
        cfg: &RiverXkbConfigV1,
        keyboard: &RiverXkbKeyboardV1,
        qh: &QueueHandle<AppData>,
    ) {
        if self.failed {
            return;
        }
        if self.ready {
            if let Some(keymap) = &self.keymap {
                set(keymap, keyboard);
            }
            return;
        }
        self.waiting.push(keyboard.clone());
        if self.keymap.is_some() {
            return;
        }
        match crate::client::virtual_keyboard::make_keymap_fd("sola-desk-keymap", KEYMAP) {
            Ok((fd, _)) => {
                self.keymap = Some(cfg.create_keymap(fd.as_fd(), KeymapFormat::TextV1, qh, ()));
            }
            Err(e) => {
                error!(%e, "desk keymap: could not make its fd; Caps Lock keeps its lock");
                self.failed = true;
                self.waiting.clear();
            }
        }
    }

    /// A keyboard went away.
    pub fn removed(&mut self, keyboard: &RiverXkbKeyboardV1) {
        self.waiting.retain(|k| k.id() != keyboard.id());
    }
}

fn set(keymap: &RiverXkbKeymapV1, keyboard: &RiverXkbKeyboardV1) {
    keyboard.set_keymap(keymap);
    keyboard.numlock_enable();
}

impl Dispatch<RiverXkbKeymapV1, ()> for AppData {
    fn event(
        state: &mut Self,
        keymap: &RiverXkbKeymapV1,
        event: <RiverXkbKeymapV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let desk = &mut state.desk_keymap;
        match event {
            river_xkb_keymap_v1::Event::Success => {
                desk.ready = true;
                for keyboard in desk.waiting.drain(..) {
                    set(keymap, &keyboard);
                }
                info!("desk keymap set: Caps Lock does nothing");
            }
            river_xkb_keymap_v1::Event::Failure { error_msg } => {
                error!(%error_msg, "River refused the desk keymap; Caps Lock keeps its lock");
                desk.failed = true;
                desk.waiting.clear();
            }
        }
    }
}
