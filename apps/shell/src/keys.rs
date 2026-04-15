use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use sola_bus::topics::{FocusTarget, FrameUpdate, Topic};

use crate::state::ShellState;
use crate::util::eval_js;

mod keycode {
    pub const TAB: u32 = 23;
    pub const LEFT: u32 = 113;
    pub const RIGHT: u32 = 114;
    pub const SUPER_L: u32 = 133;
}

pub fn setup_key_controller(
    window: &gtk4::ApplicationWindow,
    state: &Rc<RefCell<ShellState>>,
    bus: &Rc<RefCell<sola_bus::BusClient>>,
) {
    let key_ctrl = gtk4::EventControllerKey::new();
    key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);

    key_ctrl.connect_key_pressed({
        let state = state.clone();
        let bus = bus.clone();
        move |_, _keyval, keycode, gtk_modifiers| {
            let mut s = state.borrow_mut();
            let shift_held = gtk_modifiers.contains(gdk4::ModifierType::SHIFT_MASK);
            let emit = |topic: Topic| {
                let _ = bus.borrow_mut().emit(topic);
            };

            // Shell system shortcuts (highest priority).
            if let Some(action) = s.menus.lookup_shortcut(keycode, shift_held, "sola-shell") {
                tracing::info!(action_id = %action.action_id, "shell shortcut");
                if action.action_id == "exit" {
                    emit(Topic::Shutdown);
                }
                return glib::Propagation::Stop;
            }

            // Super+Tab: activate switcher.
            if keycode == keycode::TAB && !s.switcher.active {
                tracing::info!("activating switcher");
                s.switcher.apps = s.rebuild_switcher_apps();
                s.switcher.active = true;
                s.switcher.selected = if s.switcher.apps.len() > 1 { 1 } else { 0 };
                let json = serde_json::to_string(&s.switcher.apps).unwrap_or_default();
                let script = format!("renderSwitcher({}, {})", json, s.switcher.selected);
                if let Some(ref wv) = s.switcher_webview {
                    eval_js(wv, &script);
                }

                // Emit switcher frame (centered on screen).
                if let Some((ow, oh)) = s.zoning.output_size {
                    emit(Topic::Frame(FrameUpdate {
                        app_id: "sola-shell".into(),
                        title: Some("switcher".into()),
                        x: (ow - 800) / 2,
                        y: (oh - 400) / 2,
                        width: 800,
                        height: 400,
                    }));
                }
                s.emit_composition(&emit);
                return glib::Propagation::Stop;
            }

            // Switcher navigation.
            if s.switcher.active {
                match keycode {
                    keycode::TAB | keycode::RIGHT => {
                        s.switcher.select_next();
                        let sel = s.switcher.selected;
                        if let Some(ref wv) = s.switcher_webview {
                            eval_js(wv, &format!("setSelection({sel})"));
                        }
                        return glib::Propagation::Stop;
                    }
                    keycode::LEFT => {
                        s.switcher.select_prev();
                        let sel = s.switcher.selected;
                        if let Some(ref wv) = s.switcher_webview {
                            eval_js(wv, &format!("setSelection({sel})"));
                        }
                        return glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }

            // Zone snapping (Super+Numpad).
            if let Some(frame) = s.zoning.handle_key(keycode) {
                emit(Topic::Frame(frame));
                return glib::Propagation::Stop;
            }

            // Focused app menu shortcut lookup.
            if let Some(focused) = s.focused_app_id.clone() {
                if let Some(action) = s.menus.lookup_shortcut(keycode, shift_held, &focused) {
                    tracing::info!(
                        app_id = %action.app_id,
                        action_id = %action.action_id,
                        "menu shortcut matched"
                    );
                    emit(Topic::MenuAction(action));
                    return glib::Propagation::Stop;
                }
            }

            glib::Propagation::Proceed
        }
    });

    key_ctrl.connect_key_released({
        let state = state.clone();
        let bus = bus.clone();
        move |_, _keyval, keycode, _modifiers| {
            if keycode != keycode::SUPER_L {
                return;
            }

            let emit = |topic: Topic| {
                let _ = bus.borrow_mut().emit(topic);
            };

            let mut s = state.borrow_mut();
            if !s.switcher.active {
                return;
            }

            let app_id = s.switcher.selected_app_id().map(String::from);
            tracing::info!(app_id = ?app_id, "deactivating switcher");

            s.switcher.active = false;
            if let Some(ref wv) = s.switcher_webview {
                eval_js(wv, "clear()");
            }

            if let Some(ref app_id) = app_id {
                s.set_focus(app_id);
                emit(Topic::Focus(FocusTarget {
                    app_id: app_id.clone(),
                    title: None,
                }));
            }
            s.emit_composition(&emit);
        }
    });

    window.add_controller(key_ctrl);
}
