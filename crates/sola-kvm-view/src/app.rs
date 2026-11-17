//! Remote Desktop: a window per machine, several at once, and the
//! Connections window to open one from, pair a new one in, or forget one.
//! Menus follow the focused window.

use std::collections::HashMap;
use std::sync::Arc;

use iced::keyboard::{self, key::Named};
use iced::widget::container;
use iced::{Element, Event, Length, Size, Subscription, Task, Theme, event, window};

use sola_bus::Message;
use sola_bus::topics::Topic;
use sola_kit::app::{apply_theme_update, bus_subscription, is_self_quit, window_settings_transparent};
use sola_kit::tabs::TabWindows;
use sola_kit::theme::default_theme;
use sola_kvm::link;

use crate::APP_ID;
use crate::connections::{self, Connections, Row};
use crate::instance;
use crate::menu;
use crate::session::{self, Session};

/// The list's tab key; a machine's is `machine/<host>`.
const LIST_TAB: &str = "connections";

/// A machine to show: its host, and the KVM port when it is not the usual.
pub type Target = (String, u16);

pub struct App {
    tabs: TabWindows,
    sessions: HashMap<window::Id, Session>,
    /// The Connections window, while it is up.
    list: Option<(window::Id, Connections)>,
    /// The machines this one is paired to view, in name order.
    machines: Vec<String>,
    /// The window menus and keys act on: the last focused.
    focused: Option<window::Id>,
    /// The menus last published.
    menu: Option<menu::Focus>,
    theme: Theme,
    float: sola_kit::FloatState,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Bus(Arc<Message>),
    Session(window::Id, session::Msg),
    List(window::Id, connections::Msg),
    /// A key, for whichever window has the keyboard.
    Key {
        window: window::Id,
        physical: keyboard::key::Physical,
        key: keyboard::Key,
        pressed: bool,
        repeat: bool,
        modifiers: keyboard::Modifiers,
    },
    Window(window::Id, window::Event),
    /// The list's titlebar.
    Drag(window::Id),
    Resize(window::Id, window::Direction),
    /// Another launch named these machines (none: the list).
    Handoff(Vec<String>),
    /// `sola-kvm pair <host> --view` finished.
    Paired(String, Result<(), String>),
    Opened(window::Id),
}

impl App {
    /// The machines named, each in its window; with none, the list.
    pub fn boot(targets: Vec<Target>) -> (Self, Task<Msg>) {
        let mut app = Self {
            tabs: TabWindows::new(APP_ID),
            sessions: HashMap::new(),
            list: None,
            machines: paired_machines(),
            focused: None,
            menu: None,
            theme: default_theme(),
            float: sola_kit::FloatState::new(APP_ID),
        };
        let task = app.open_targets(targets);
        app.publish_menu();
        (app, task)
    }

    pub fn title(&self, window: window::Id) -> String {
        self.sessions
            .get(&window)
            .map_or_else(|| "Remote Desktop".into(), |s| s.host.clone())
    }

    pub fn theme(&self, window: window::Id) -> Theme {
        sola_kit::theme_for(self.chrome(window), &self.theme)
    }

    fn chrome(&self, window: window::Id) -> sola_kit::Chrome {
        self.tabs
            .river_id(window)
            .map_or(sola_kit::Chrome::Tiled, |id| self.float.chrome(id))
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        let links = self
            .sessions
            .iter()
            .map(|(&window, session)| session.subscription().with(window).map(|(w, m)| Msg::Session(w, m)));
        Subscription::batch(
            [
                bus_subscription().map(Msg::Bus),
                instance::subscription().map(Msg::Handoff),
                event::listen_with(|event, _status, window| match event {
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        physical_key,
                        key,
                        modifiers,
                        repeat,
                        ..
                    }) => Some(Msg::Key {
                        window,
                        physical: physical_key,
                        key,
                        pressed: true,
                        repeat,
                        modifiers,
                    }),
                    Event::Keyboard(keyboard::Event::KeyReleased {
                        physical_key,
                        key,
                        modifiers,
                        ..
                    }) => Some(Msg::Key {
                        window,
                        physical: physical_key,
                        key,
                        pressed: false,
                        repeat: false,
                        modifiers,
                    }),
                    Event::Window(event) => Some(Msg::Window(window, event)),
                    _ => None,
                }),
            ]
            .into_iter()
            .chain(links),
        )
    }

    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        let task = self.on(msg);
        self.publish_menu();
        task
    }

    fn on(&mut self, msg: Msg) -> Task<Msg> {
        match msg {
            Msg::Bus(message) => return self.on_bus(&message),
            Msg::Session(window, msg) => {
                if matches!(msg, session::Msg::TitleClose) {
                    return self.close(window);
                }
                if let Some(session) = self.sessions.get_mut(&window) {
                    return session.update(msg).map(move |m| Msg::Session(window, m));
                }
            }
            Msg::List(window, msg) => return self.on_list(window, msg),
            Msg::Key {
                window,
                physical,
                key,
                pressed,
                repeat,
                modifiers,
            } => {
                if self.sessions.contains_key(&window) {
                    let key = session::Msg::Key {
                        physical,
                        pressed,
                        repeat,
                        modifiers,
                    };
                    return self.on(Msg::Session(window, key));
                }
                if let (true, keyboard::Key::Named(named)) = (pressed, key) {
                    return self.on_list(window, connections::Msg::Key(named));
                }
            }
            Msg::Drag(window) => return sola_kit::drag(Some(window)),
            Msg::Resize(window, direction) => return sola_kit::drag_resize(Some(window), direction),
            Msg::Window(window, event) => return self.on_window(window, event),
            Msg::Handoff(targets) => {
                return match targets.is_empty() {
                    true => self.show_list(),
                    false => self.open_targets(targets.iter().map(|t| target(t)).collect()),
                };
            }
            Msg::Paired(host, result) => {
                self.machines = paired_machines();
                let Some((_, list)) = self.list.as_mut() else {
                    return match result {
                        Ok(()) => self.open_machine(host, DEFAULT_KVM_PORT),
                        Err(_) => Task::none(),
                    };
                };
                list.pairing = None;
                match result {
                    Ok(()) => {
                        tracing::info!(%host, "remote desktop: paired");
                        list.set_query(String::new());
                        let list_window = self.list.as_ref().map(|(w, _)| *w);
                        let open = self.open_machine(host, DEFAULT_KVM_PORT);
                        return Task::batch([open, list_window.map_or_else(Task::none, |w| self.close(w))]);
                    }
                    Err(why) => {
                        tracing::warn!(%host, %why, "remote desktop: pairing failed");
                        list.error = Some(why);
                    }
                }
            }
            Msg::Opened(window) => {
                if let Some(session) = self.sessions.get(&window) {
                    return session.opened().map(move |m| Msg::Session(window, m));
                }
                if self.list.as_ref().is_some_and(|(w, _)| *w == window) {
                    return iced::widget::operation::focus(iced::widget::Id::new(connections::FIELD_ID));
                }
            }
        }
        Task::none()
    }

    fn on_bus(&mut self, message: &Message) -> Task<Msg> {
        self.float.update(message);
        self.tabs.update(message);
        apply_theme_update(message, &mut self.theme);
        if is_self_quit(message, APP_ID) {
            return iced::exit();
        }
        // The tab bar's stoplight: as the window's own close.
        if let Some(window) = self.tabs.close_request(message) {
            return self.close(window);
        }
        let Some(Topic::MenuAction(p)) = Topic::parse(message) else {
            return Task::none();
        };
        if p.app_id != APP_ID {
            return Task::none();
        }
        let action = p.action_id.as_str();
        match action {
            "quit" => return iced::exit(),
            menu::CONNECTIONS => return self.show_list(),
            menu::CONNECT => {
                if let Some(machine) = self.list_selection() {
                    return self.pick_machine(machine);
                }
            }
            menu::FORGET => {
                if let Some(machine) = self.list_selection() {
                    self.forget(&machine);
                }
            }
            _ => {}
        }
        let Some(session) = self.focused.and_then(|w| self.sessions.get_mut(&w)) else {
            return Task::none();
        };
        match action {
            menu::STATISTICS => session.statistics = !session.statistics,
            menu::MIRROR | menu::EXTEND => session.set_mirror(action == menu::MIRROR),
            _ => {}
        }
        Task::none()
    }

    fn on_window(&mut self, window: window::Id, event: window::Event) -> Task<Msg> {
        let to_session = match event {
            window::Event::Focused => {
                self.focused = Some(window);
                if self.list.as_ref().is_some_and(|(w, _)| *w == window) {
                    self.machines = paired_machines();
                }
                Some(session::Msg::Focused)
            }
            window::Event::Unfocused => Some(session::Msg::Unfocused),
            window::Event::Rescaled(scale) => Some(session::Msg::Scale(scale)),
            window::Event::CloseRequested => return self.close(window),
            window::Event::Closed => {
                self.forget_window(window);
                if self.sessions.is_empty() && self.list.is_none() {
                    return iced::exit();
                }
                None
            }
            _ => None,
        };
        match (to_session, self.sessions.get_mut(&window)) {
            (Some(msg), Some(session)) => session.update(msg).map(move |m| Msg::Session(window, m)),
            _ => Task::none(),
        }
    }

    fn on_list(&mut self, window: window::Id, msg: connections::Msg) -> Task<Msg> {
        let Some((list_window, list)) = self.list.as_mut() else {
            return Task::none();
        };
        if *list_window != window {
            return Task::none();
        }
        let len = list.rows(&self.machines).len();
        match msg {
            connections::Msg::Query(query) => {
                list.set_query(query);
                list.error = None;
            }
            connections::Msg::Key(Named::ArrowUp) => list.step(-1, len),
            connections::Msg::Key(Named::ArrowDown) => list.step(1, len),
            connections::Msg::Key(Named::Escape) => return self.close(window),
            connections::Msg::Key(_) => {}
            connections::Msg::Pick(i) => {
                list.selected = i;
                match list.rows(&self.machines).into_iter().nth(i) {
                    Some(Row::Machine(machine)) => return self.pick_machine(machine),
                    Some(Row::Pair(host)) if list.pairing.is_none() => {
                        list.pairing = Some(host.clone());
                        list.error = None;
                        return Task::perform(pair(host.clone()), move |result| Msg::Paired(host.clone(), result));
                    }
                    Some(Row::Pair(_)) | None => {}
                }
            }
        }
        Task::none()
    }

    /// A machine picked in the list: its window, and the list goes.
    fn pick_machine(&mut self, machine: String) -> Task<Msg> {
        let list = self.list.as_ref().map(|(w, _)| *w);
        let open = self.open_machine(machine, DEFAULT_KVM_PORT);
        Task::batch([open, list.map_or_else(Task::none, |w| self.close(w))])
    }

    /// This machine no longer views `machine` (its key here goes; the other
    /// still knows this one's until it is paired again).
    fn forget(&mut self, machine: &str) {
        let forgot = link::forget_view_peer(&link::dir(), machine);
        self.machines = paired_machines();
        if let Some((_, list)) = self.list.as_mut() {
            list.error = forgot.err().map(|e| format!("{machine} could not be forgotten: {e}"));
            let len = list.rows(&self.machines).len();
            list.selected = list.selected.min(len.saturating_sub(1));
        }
        tracing::info!(%machine, "remote desktop: forgot a machine");
    }

    fn list_selection(&self) -> Option<String> {
        let (_, list) = self.list.as_ref()?;
        list.selected_machine(&self.machines)
    }

    fn open_targets(&mut self, targets: Vec<Target>) -> Task<Msg> {
        if targets.is_empty() {
            return self.show_list();
        }
        Task::batch(
            targets
                .into_iter()
                .map(|(host, port)| self.open_machine(host, port))
                .collect::<Vec<_>>(),
        )
    }

    /// `host`'s window: brought up when it is open, else opened.
    fn open_machine(&mut self, host: String, port: u16) -> Task<Msg> {
        if let Some((&window, _)) = self.sessions.iter().find(|(_, s)| s.host == host) {
            self.tabs.show(window);
            return Task::none();
        }
        let mut settings = window_settings_transparent(APP_ID);
        settings.size = Size::new(1512.0, 1010.0);
        let (window, task) = self.tabs.open(&format!("machine/{host}"), settings, false);
        tracing::info!(%host, port, "remote desktop: a machine's window");
        self.sessions.insert(window, Session::new(host, port, window));
        self.focused = Some(window);
        task.map(Msg::Opened)
    }

    /// The Connections window: brought up when it is open, else opened.
    fn show_list(&mut self) -> Task<Msg> {
        self.machines = paired_machines();
        if let Some((window, _)) = &self.list {
            self.tabs.show(*window);
            return iced::widget::operation::focus(iced::widget::Id::new(connections::FIELD_ID));
        }
        let mut settings = window_settings_transparent(APP_ID);
        settings.size = Size::new(720.0, 520.0);
        let (window, task) = self.tabs.open(LIST_TAB, settings, false);
        self.list = Some((window, Connections::default()));
        self.focused = Some(window);
        task.map(Msg::Opened)
    }

    fn close(&mut self, window: window::Id) -> Task<Msg> {
        self.forget_window(window);
        window::close(window)
    }

    /// A window going: its connection ends with its session.
    fn forget_window(&mut self, window: window::Id) {
        self.sessions.remove(&window);
        if self.list.as_ref().is_some_and(|(w, _)| *w == window) {
            self.list = None;
        }
        self.tabs.close(window);
        if self.focused == Some(window) {
            self.focused = None;
        }
    }

    /// The menus for the focused window, sent when they change.
    fn publish_menu(&mut self) {
        let focus = match self.focused {
            Some(window) if self.list.as_ref().is_some_and(|(w, _)| *w == window) => {
                menu::Focus::Connections(self.list_selection())
            }
            Some(window) if self.sessions.contains_key(&window) => menu::Focus::Machine(self.sessions[&window].menu()),
            _ => match self.list {
                Some(_) => menu::Focus::Connections(self.list_selection()),
                None => menu::Focus::Machine(self.sessions.values().next().map_or(
                    menu::View {
                        statistics: false,
                        mirrored: false,
                        can_mirror: true,
                        can_extend: true,
                    },
                    Session::menu,
                )),
            },
        };
        if self.menu.as_ref() != Some(&focus) {
            sola_kit::menu::publish(menu::app_menu(&focus));
            self.menu = Some(focus);
        }
    }

    pub fn view(&self, window: window::Id) -> Element<'_, Msg> {
        if let Some(session) = self.sessions.get(&window) {
            return session.view(self.chrome(window)).map(move |m| Msg::Session(window, m));
        }
        let Some((_, list)) = self.list.as_ref().filter(|(w, _)| *w == window) else {
            return container(iced::widget::Space::new()).width(Length::Fill).height(Length::Fill).into();
        };
        let open: Vec<&str> = self.sessions.values().map(|s| s.host.as_str()).collect();
        let body = connections::view(list, &self.machines, &open).map(move |m| Msg::List(window, m));
        sola_kit::window_frame(
            self.chrome(window),
            "Remote Desktop",
            Msg::Drag(window),
            Msg::Window(window, window::Event::CloseRequested),
            move |direction| Msg::Resize(window, direction),
            body,
        )
    }
}

pub const DEFAULT_KVM_PORT: u16 = 4242;

/// `host` or `host:port`.
pub fn target(arg: &str) -> Target {
    match arg.rsplit_once(':') {
        Some((host, port)) => (host.to_string(), port.parse().unwrap_or(DEFAULT_KVM_PORT)),
        None => (arg.to_string(), DEFAULT_KVM_PORT),
    }
}

fn paired_machines() -> Vec<String> {
    link::view_hosts(&link::dir()).unwrap_or_else(|e| {
        tracing::warn!(%e, "remote desktop: the paired machines cannot be read");
        Vec::new()
    })
}

/// `sola-kvm pair <host> --view`, off the UI's threads: what it said went
/// wrong, in its words.
async fn pair(host: String) -> Result<(), String> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let bin = std::env::current_exe()
            .ok()
            .and_then(|me| Some(me.parent()?.join("sola-kvm")))
            .filter(|path| path.exists())
            .unwrap_or_else(|| "sola-kvm".into());
        let out = std::process::Command::new(&bin)
            .args(["pair", &host, "--view"])
            .stdin(std::process::Stdio::null())
            .output();
        let _ = tx.send(match out {
            Ok(out) if out.status.success() => Ok(()),
            // Its last line says what failed; ssh's own lines come before.
            Ok(out) => {
                let said = String::from_utf8_lossy(&out.stderr);
                Err(match said.lines().map(str::trim).rfind(|l| !l.is_empty()) {
                    Some(line) => line.trim_start_matches("sola-kvm pair: ").to_string(),
                    None => format!("Pairing with {host} failed ({})", out.status),
                })
            }
            Err(e) => Err(format!("{}: {e}", bin.display())),
        });
    });
    rx.await.unwrap_or_else(|_| Err("the pairing stopped".into()))
}
