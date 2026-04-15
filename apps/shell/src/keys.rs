use std::cell::RefCell;
use std::rc::Weak;

use gtk4::prelude::*;
use sola_app::{AppRuntime, SolaApp, WindowHandle};
use sola_bus::topics::{FocusTarget, FrameUpdate, Topic};

use crate::app::ShellApp;

mod keycode {
    pub const TAB: u32 = 23;
    pub const LEFT: u32 = 113;
    pub const RIGHT: u32 = 114;
    pub const SUPER_L: u32 = 133;
}

pub fn install(menubar: WindowHandle, runtime: Weak<RefCell<AppRuntime<ShellApp>>>) {
    let key_ctrl = gtk4::EventControllerKey::new();
    key_ctrl.set_propagation_phase(gtk4::PropagationPhase::Capture);

    key_ctrl.connect_key_pressed({
        let runtime = runtime.clone();
        move |_, _keyval, keycode, gtk_modifiers| {
            let shift = gtk_modifiers.contains(gdk4::ModifierType::SHIFT_MASK);
            let Some(runtime) = runtime.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let mut rt = runtime.borrow_mut();
            let AppRuntime { app, ctx } = &mut *rt;
            handle_key_pressed(app, ctx, keycode, shift)
        }
    });

    key_ctrl.connect_key_released({
        let runtime = runtime.clone();
        move |_, _keyval, keycode, _modifiers| {
            if keycode != keycode::SUPER_L {
                return;
            }
            let Some(runtime) = runtime.upgrade() else { return };
            let mut rt = runtime.borrow_mut();
            let AppRuntime { app, ctx } = &mut *rt;
            handle_super_released(app, ctx);
        }
    });

    menubar.gtk_window().add_controller(key_ctrl);
}

fn handle_key_pressed(
    app: &mut ShellApp,
    ctx: &mut sola_app::AppCtx,
    keycode: u32,
    shift_held: bool,
) -> glib::Propagation {
    // Shell system shortcuts (highest priority).
    if let Some(action) = app.menus.lookup_shortcut(keycode, shift_held, ShellApp::APP_ID) {
        tracing::info!(action_id = %action.action_id, "shell shortcut");
        if action.action_id == "exit" {
            ctx.emit(Topic::Shutdown);
        }
        return glib::Propagation::Stop;
    }

    // Super+Tab: activate switcher.
    if keycode == keycode::TAB && !app.switcher.active {
        tracing::info!("activating switcher");
        app.switcher.apps = app.rebuild_switcher_apps();
        app.switcher.active = true;
        app.switcher.selected = if app.switcher.apps.len() > 1 { 1 } else { 0 };
        let json = serde_json::to_string(&app.switcher.apps).unwrap_or_default();
        app.switcher_win
            .eval_js(&format!("renderSwitcher({}, {})", json, app.switcher.selected));

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
        match keycode {
            keycode::TAB | keycode::RIGHT => {
                app.switcher.select_next();
                let sel = app.switcher.selected;
                app.switcher_win.eval_js(&format!("setSelection({sel})"));
                return glib::Propagation::Stop;
            }
            keycode::LEFT => {
                app.switcher.select_prev();
                let sel = app.switcher.selected;
                app.switcher_win.eval_js(&format!("setSelection({sel})"));
                return glib::Propagation::Stop;
            }
            _ => {}
        }
    }

    // Zone snapping (Super+Numpad).
    if let Some(frame) = app.zoning.handle_key(keycode) {
        ctx.emit(Topic::Frame(frame));
        return glib::Propagation::Stop;
    }

    // Focused app menu shortcut lookup.
    if let Some(focused) = app.focused_app_id.clone() {
        if let Some(action) = app.menus.lookup_shortcut(keycode, shift_held, &focused) {
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

fn handle_super_released(app: &mut ShellApp, ctx: &mut sola_app::AppCtx) {
    if !app.switcher.active {
        return;
    }

    let app_id = app.switcher.selected_app_id().map(String::from);
    tracing::info!(app_id = ?app_id, "deactivating switcher");

    app.switcher.active = false;
    app.switcher_win.eval_js("clear()");

    if let Some(ref app_id) = app_id {
        app.set_focus(app_id);
        ctx.emit(Topic::Focus(FocusTarget {
            app_id: app_id.clone(),
            title: None,
        }));
    }
    app.emit_composition(ctx);
}
