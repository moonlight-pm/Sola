use std::cell::RefCell;
use std::rc::Weak;

use gtk4::prelude::*;
use sola_app::{AppRuntime, SolaApp, WindowHandle};
use sola_bus::topics::{FocusTarget, FrameUpdate, Topic};
use sola_core::{KeyChord, KeyCode};

use crate::app::ShellApp;

pub fn install(menubar: WindowHandle, runtime: Weak<RefCell<AppRuntime<ShellApp>>>) {
    let key_ctrl = gtk4::EventControllerKey::new();
    key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);

    key_ctrl.connect_key_pressed({
        let runtime = runtime.clone();
        move |_, _keyval, keycode, gtk_modifiers| {
            // On Linux/Wayland the physical Super key produces SUPER_MASK
            // (xkb Mod4). META_MASK only fires when an explicit Meta key is
            // mapped. We treat either as "meta" since Sola's convention is
            // Meta = Super.
            let chord = KeyChord {
                keycode: KeyCode::from(keycode),
                meta: gtk_modifiers.contains(gdk4::ModifierType::SUPER_MASK)
                    || gtk_modifiers.contains(gdk4::ModifierType::META_MASK),
                alt: gtk_modifiers.contains(gdk4::ModifierType::ALT_MASK),
                ctrl: gtk_modifiers.contains(gdk4::ModifierType::CONTROL_MASK),
                shift: gtk_modifiers.contains(gdk4::ModifierType::SHIFT_MASK),
            };
            tracing::debug!(
                keycode = chord.keycode.raw(),
                meta = chord.meta,
                ctrl = chord.ctrl,
                alt = chord.alt,
                shift = chord.shift,
                "shell key pressed"
            );
            let Some(runtime) = runtime.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let mut rt = runtime.borrow_mut();
            let AppRuntime { app, ctx } = &mut *rt;
            handle_key_pressed(app, ctx, chord)
        }
    });

    key_ctrl.connect_key_released({
        let runtime = runtime.clone();
        move |_, _keyval, keycode, _modifiers| {
            if keycode != KeyCode::LEFT_META.raw() && keycode != KeyCode::RIGHT_META.raw() {
                return;
            }
            let Some(runtime) = runtime.upgrade() else {
                return;
            };
            let mut rt = runtime.borrow_mut();
            let AppRuntime { app, ctx } = &mut *rt;
            handle_meta_released(app, ctx);
        }
    });

    menubar.gtk_window().add_controller(key_ctrl);
}

fn handle_key_pressed(
    app: &mut ShellApp,
    ctx: &mut sola_app::AppCtx,
    chord: KeyChord,
) -> glib::Propagation {
    // Shell system shortcuts (highest priority).
    if let Some(action) = app.menus.lookup_shortcut(&chord, ShellApp::APP_ID) {
        tracing::info!(action_id = %action.action_id, "shell shortcut");
        if action.action_id == "exit" {
            ctx.emit(Topic::Shutdown);
        }
        return glib::Propagation::Stop;
    }

    // Meta+Space: toggle launcher.
    if chord.meta && chord.keycode == KeyCode::SPACE {
        if app.launcher.active {
            app.close_launcher(ctx);
        } else {
            app.open_launcher(ctx);
        }
        return glib::Propagation::Stop;
    }

    // Meta+Tab: activate switcher. Close launcher first if open.
    if chord.keycode == KeyCode::TAB && !app.switcher.active {
        if app.launcher.active {
            app.close_launcher(ctx);
        }
        tracing::info!("activating switcher");
        app.switcher.apps = app.rebuild_switcher_apps();
        app.switcher.active = true;
        app.switcher.selected = if app.switcher.apps.len() > 1 { 1 } else { 0 };
        let json = app.switcher_apps_json();
        app.windows.switcher.eval_js(&format!(
            "renderSwitcher({}, {})",
            json, app.switcher.selected
        ));

        if let Some((ow, oh)) = app.zoning.output_size {
            ctx.emit(Topic::Frame(FrameUpdate {
                app_id: ShellApp::APP_ID.into(),
                title: Some("switcher".into()),
                x: (ow - 800) / 2,
                y: (oh - 400) / 2,
                width: 800,
                height: 400,
            }));
        }
        app.emit_composition(ctx);
        return glib::Propagation::Stop;
    }

    // Switcher navigation.
    if app.switcher.active {
        match chord.keycode {
            code if code == KeyCode::TAB || code == KeyCode::RIGHT => {
                app.switcher.select_next();
                let sel = app.switcher.selected;
                app.windows
                    .switcher
                    .eval_js(&format!("setSelection({sel})"));
                return glib::Propagation::Stop;
            }
            code if code == KeyCode::LEFT => {
                app.switcher.select_prev();
                let sel = app.switcher.selected;
                app.windows
                    .switcher
                    .eval_js(&format!("setSelection({sel})"));
                return glib::Propagation::Stop;
            }
            _ => {}
        }
    }

    // Zone snapping (Meta+Numpad).
    if let Some(frame) = app.zoning.handle_key(chord.keycode.raw()) {
        ctx.emit(Topic::Frame(frame));
        return glib::Propagation::Stop;
    }

    // Focused app menu shortcut lookup.
    if let Some(focused) = app.focused_app_id.clone() {
        if let Some(action) = app.menus.lookup_shortcut(&chord, &focused) {
            tracing::info!(
                app_id = %action.app_id,
                action_id = %action.action_id,
                "menu shortcut matched"
            );
            ctx.emit(Topic::MenuAction(action));
            return glib::Propagation::Stop;
        }
    }

    glib::Propagation::Proceed
}

fn handle_meta_released(app: &mut ShellApp, ctx: &mut sola_app::AppCtx) {
    if !app.switcher.active {
        return;
    }

    let app_id = app.switcher.selected_app_id().map(String::from);
    tracing::info!(app_id = ?app_id, "deactivating switcher");

    app.switcher.active = false;
    app.windows.switcher.eval_js("clear()");

    if let Some(ref app_id) = app_id {
        app.set_focus(app_id);
        ctx.emit(Topic::Focus(FocusTarget {
            app_id: app_id.clone(),
            title: None,
        }));
    }
    app.emit_composition(ctx);
}
