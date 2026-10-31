//! Settings → Keybindings (`docs/specs/2026-10-30-keybindings-design.md`):
//! every owner's actions and their keys now, each changed in place.
//!
//! Owners declare their actions (`Topic::KeyActions`); what you change is
//! an override (`Topic::Keybindings`), and the kit's `keys::resolve` says
//! what every action has now. While a row waits for a key, the page says
//! so on the bus (`Topic::KeyCapture`) and the shell lets every key
//! through to this window.

use iced::keyboard::Modifiers;
use iced::keyboard::key::Physical;
use iced::widget::{column, container, text};
use iced::{Element, Event, Length, Subscription, Task, event, keyboard};
use sola_bus::topics::{KeyActions, KeyCapture, Keybindings, Topic};
use sola_core::{KeyChord, KeyCode};
use sola_kit::app::bus;
use sola_kit::components::button;
use sola_kit::components::text as kit_text;
use sola_kit::fonts;
use sola_kit::keys::{self, Binding, COMMON, SYSTEM};

use crate::page;

/// The page's state: every declaration, your overrides, the row open.
#[derive(Default)]
pub struct KeysState {
    pub decls: Vec<KeyActions>,
    pub keys: Keybindings,
    /// The row whose editor is open: (owner, action).
    open: Option<(String, String)>,
    /// That row waits for a key.
    capturing: bool,
    /// A key that another action holds, waiting for "Move it here".
    pending: Option<KeyChord>,
    note: Option<String>,
}

#[derive(Debug, Clone)]
pub enum KeysMsg {
    Toggle(String, String),
    Change,
    Pressed(Physical, Modifiers),
    Cancel,
    Move,
    Remove,
    Reset,
    /// A zone's row: zone keys are set on the Zones page.
    OpenZones,
}

impl KeysState {
    /// A declaration arrived (`None`: `owner`'s was retracted).
    pub fn declared(&mut self, owner: &str, decl: Option<KeyActions>) {
        self.decls.retain(|d| d.owner != owner);
        if let Some(d) = decl {
            self.decls.push(d);
        }
        self.decls.sort_by_key(|d| (rank(&d.owner), d.label.to_lowercase()));
    }

    pub fn bindings(&self) -> Vec<Binding> {
        keys::resolve(&self.decls, &self.keys)
    }

    /// Whether a row waits for a key (fold [`capture_keys`] in).
    pub fn capturing(&self) -> bool {
        self.capturing
    }
}

fn rank(owner: &str) -> u8 {
    match owner {
        SYSTEM => 0,
        COMMON => 1,
        _ => 2,
    }
}

/// Key presses while a row waits for one.
pub fn capture_keys() -> Subscription<(Physical, Modifiers)> {
    event::listen_with(|event, _status, _window| match event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            physical_key,
            modifiers,
            ..
        }) => Some((physical_key, modifiers)),
        _ => None,
    })
}

/// Tell the shell to let keys through (`on`), or to grab them again.
pub fn set_capture(on: bool) {
    let capture = KeyCapture {
        app_id: crate::APP_ID.to_string(),
    };
    send(|b| {
        if on {
            b.emit(Topic::KeyCapture(capture))
        } else {
            b.retract(Topic::KeyCapture(capture))
        }
    });
}

/// Store your overrides.
pub fn save(keys: &Keybindings) {
    let keys = keys.clone();
    send(|b| b.emit(Topic::Keybindings(keys)));
}

fn send(write: impl FnOnce(&mut sola_bus::BusClient) -> std::io::Result<()>) {
    let sent = bus()
        .lock()
        .map_err(|_| "bus unavailable".to_string())
        .and_then(|mut client| write(&mut client).map_err(|e| e.to_string()));
    if let Err(e) = sent {
        tracing::warn!(error = %e, "keybindings: bus write failed");
    }
}

/// What `owner`'s `action` is called, for "⌘L is Lock Screen".
fn name_of(state: &KeysState, owner: &str, action: &str) -> String {
    let decl = state.decls.iter().find(|d| d.owner == owner);
    let label = decl
        .and_then(|d| d.actions.iter().find(|a| a.id == action))
        .map_or(action.to_string(), |a| a.label.clone());
    match decl {
        Some(d) if owner != SYSTEM => format!("{label} ({})", d.label),
        _ => label,
    }
}

pub fn update(msg: KeysMsg, state: &mut KeysState) -> Task<KeysMsg> {
    match msg {
        KeysMsg::Toggle(owner, action) => {
            let row = (owner, action);
            let closing = state.open.as_ref() == Some(&row);
            end_capture(state);
            state.pending = None;
            state.note = None;
            state.open = (!closing).then_some(row);
        }
        KeysMsg::Change => {
            state.pending = None;
            state.note = None;
            state.capturing = true;
            set_capture(true);
        }
        KeysMsg::Pressed(physical, modifiers) => {
            if !state.capturing {
                return Task::none();
            }
            let Some(chord) = keys::chord_from_iced(&physical, modifiers) else {
                return Task::none();
            };
            let plain = !chord.meta && !chord.ctrl && !chord.alt && !chord.shift;
            if chord.keycode == KeyCode::ESCAPE && plain {
                end_capture(state);
                return Task::none();
            }
            end_capture(state);
            let Some((owner, action)) = state.open.clone() else {
                return Task::none();
            };
            if needs_super(state, &owner, &action) && !chord.meta {
                state.note = Some(if owner == SYSTEM {
                    format!(
                        "{} has no Super: a system key needs it, so it never takes a key apps type with.",
                        chord.display()
                    )
                } else {
                    format!(
                        "{} has no Super: the Shell hands an app its menu keys only with Super.",
                        chord.display()
                    )
                });
                return Task::none();
            }
            take(state, &owner, &action, chord);
        }
        KeysMsg::Cancel => {
            end_capture(state);
            state.pending = None;
            state.note = None;
        }
        KeysMsg::Move => {
            let (Some((owner, action)), Some(chord)) = (state.open.clone(), state.pending.take())
            else {
                return Task::none();
            };
            let bindings = state.bindings();
            if let Some(h) = keys::holder(&bindings, &owner, &action, chord).cloned() {
                keys::set(&mut state.keys, &h.owner, &h.action, h.default, None);
            }
            assign(state, &owner, &action, Some(chord));
        }
        KeysMsg::Remove => {
            if let Some((owner, action)) = state.open.clone() {
                assign(state, &owner, &action, None);
            }
        }
        KeysMsg::OpenZones => {}
        KeysMsg::Reset => {
            if let Some((owner, action)) = state.open.clone() {
                keys::reset(&mut state.keys, &owner, &action);
                state.note = None;
                save(&state.keys);
            }
        }
    }
    Task::none()
}

/// Whether `owner`'s `action` takes only keys with Super: every system
/// key, and an app's menu keys (the Shell grabs and hands those over),
/// except actions whose own default has none (keys the app reads itself,
/// as Mail's letters).
fn needs_super(state: &KeysState, owner: &str, action: &str) -> bool {
    if owner == SYSTEM {
        return true;
    }
    state
        .decls
        .iter()
        .find(|d| d.owner == owner)
        .and_then(|d| d.actions.iter().find(|a| a.id == action))
        .and_then(|a| a.default)
        .is_none_or(|d| d.meta)
}

/// Give the open row `chord`, or ask first when another action holds it.
fn take(state: &mut KeysState, owner: &str, action: &str, chord: KeyChord) {
    let bindings = state.bindings();
    if keys::key_of(&bindings, owner, action) == Some(chord) {
        return;
    }
    if keys::holder(&bindings, owner, action, chord).is_some() {
        state.pending = Some(chord);
        return;
    }
    assign(state, owner, action, Some(chord));
}

fn assign(state: &mut KeysState, owner: &str, action: &str, key: Option<KeyChord>) {
    let default = state
        .decls
        .iter()
        .find(|d| d.owner == owner)
        .and_then(|d| d.actions.iter().find(|a| a.id == action))
        .and_then(|a| a.default);
    keys::set(&mut state.keys, owner, action, default, key);
    state.note = None;
    save(&state.keys);
}

fn end_capture(state: &mut KeysState) {
    if std::mem::take(&mut state.capturing) {
        set_capture(false);
    }
}

/// Leave no capture behind (the page or the app is going away).
pub fn leave(state: &mut KeysState) {
    end_capture(state);
}

fn chip<'a>(chord: Option<KeyChord>) -> Element<'a, KeysMsg> {
    match chord {
        Some(c) => text(c.display()).font(fonts::ui()).size(13).into(),
        None => kit_text::body("No key").style(kit_text::muted).into(),
    }
}

pub fn view(state: &KeysState) -> Element<'_, KeysMsg> {
    let bindings = state.bindings();
    if bindings.is_empty() {
        return page::page(vec![page::section(
            "Keys",
            None,
            page::group(vec![page::empty("Waiting for the shell's keys…")]),
            None,
        )]);
    }
    let mut sections: Vec<Element<'_, KeysMsg>> = Vec::new();
    for decl in &state.decls {
        let mine: Vec<&Binding> = bindings.iter().filter(|b| b.owner == decl.owner).collect();
        let mut groups: Vec<String> = Vec::new();
        for b in &mine {
            if !groups.contains(&b.group) {
                groups.push(b.group.clone());
            }
        }
        for g in groups {
            let rows: Vec<Element<'_, KeysMsg>> = mine
                .iter()
                .filter(|b| b.group == g)
                .flat_map(|b| row(state, b))
                .collect();
            let title = if g.is_empty() || g == decl.label {
                decl.label.clone()
            } else if decl.owner == SYSTEM {
                g.clone()
            } else {
                format!("{} · {g}", decl.label)
            };
            sections.push(page::section(title, None, page::group(rows), None));
        }
    }
    page::page(sections)
}

fn row<'a>(state: &'a KeysState, b: &Binding) -> Vec<Element<'a, KeysMsg>> {
    let open = state.open.as_ref() == Some(&(b.owner.clone(), b.action.clone()));
    let subtitle = match (&b.lost_to, b.changed) {
        (Some((o, a)), _) => format!(
            "{} is {}",
            b.default.map_or_else(String::new, |d| d.display()),
            name_of(state, o, a)
        ),
        (None, true) => "Changed".to_string(),
        (None, false) => String::new(),
    };
    // Zone keys are set where the zones are drawn.
    let zone = b.owner == SYSTEM && b.group == "Zones";
    let press = if zone {
        KeysMsg::OpenZones
    } else {
        KeysMsg::Toggle(b.owner.clone(), b.action.clone())
    };
    let mut out: Vec<Element<'a, KeysMsg>> = vec![
        page::item(b.label.clone())
            .subtitle(if zone && subtitle.is_empty() { "Set on the Zones page".to_string() } else { subtitle })
            .trailing(chip(b.key))
            .open(open)
            .on_press(press)
            .into(),
    ];
    if open {
        out.push(page::editor(editor(state, b)));
    }
    out
}

fn editor<'a>(state: &'a KeysState, b: &Binding) -> Element<'a, KeysMsg> {
    if state.capturing {
        let hint = if needs_super(state, &b.owner, &b.action) {
            "Press the new key, with Super. Esc cancels."
        } else {
            "Press the new key. Esc cancels."
        };
        return column![
            kit_text::body(hint),
            page::footer(vec![], vec![page::action("Cancel", button::secondary, KeysMsg::Cancel)]),
        ]
        .spacing(10)
        .into();
    }
    if let Some(chord) = state.pending {
        let bindings = state.bindings();
        let who = keys::holder(&bindings, &b.owner, &b.action, chord)
            .map_or(String::new(), |h| name_of(state, &h.owner, &h.action));
        return column![
            kit_text::body(format!(
                "{} is {who}. Move it here? {who} is left without a key.",
                chord.display()
            )),
            page::footer(
                vec![page::action("Move It Here", button::primary, KeysMsg::Move)],
                vec![page::action("Cancel", button::secondary, KeysMsg::Cancel)],
            ),
        ]
        .spacing(10)
        .into();
    }
    let mut left = vec![page::action("Change", button::primary, KeysMsg::Change)];
    if b.key.is_some() {
        left.push(page::action("Remove", button::secondary, KeysMsg::Remove));
    }
    let mut right = Vec::new();
    if b.changed {
        let default = b.default.map_or("no key".to_string(), |d| d.display());
        right.push(page::action(
            format!("Reset to {default}"),
            button::secondary,
            KeysMsg::Reset,
        ));
    }
    let mut col = column![page::footer(left, right)].spacing(10);
    if let Some(note) = &state.note {
        col = col.push(container(kit_text::caption(note.clone()).style(kit_text::muted)).width(Length::Fill));
    }
    col.into()
}
